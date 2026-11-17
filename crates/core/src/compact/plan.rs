use std::collections::{BTreeMap, HashMap, HashSet};

/// One old commit: its tree, parents, author and committer lines and message, as git stores them.
#[derive(Debug, Clone)]
pub struct Commit {
    pub tree: String,
    pub parents: Vec<String>,
    pub author: String,
    pub committer: String,
    pub message: String,
}

/// The old repo, read once: commits, refs (`refs/pull/<n>/head` included), notes (ref, commit, text), open PRs.
#[derive(Debug, Default)]
pub struct Old {
    pub commits: HashMap<String, Commit>,
    pub refs: Vec<(String, String)>,
    pub notes: Vec<(String, String, String)>,
    pub open: Vec<(u64, String)>,
}

/// Where an old commit went: its own new commit (`Same`, tree-equal), its PR's (`Held`) or the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Same,
    Held,
    Root,
}

impl Kind {
    /// The word `compact-map.tsv` writes.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Same => "same",
            Kind::Held => "held",
            Kind::Root => "root",
        }
    }
}

/// What a ref is to compact: the line, at a line commit, carried on its base, detached, merged (deleted) or notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Line,
    SameTree,
    Carried,
    Detached,
    Merged,
    Notes,
}

/// A new commit before git names it: parents by index.
#[derive(Debug, Clone)]
pub struct Node {
    pub tree: String,
    pub parents: Vec<usize>,
    pub author: String,
    pub committer: String,
    pub message: String,
}

/// A ref's move: name, old value, the node it moves to (none: deleted), its class.
#[derive(Debug, Clone)]
pub struct Move {
    pub name: String,
    pub old: String,
    pub to: Option<usize>,
    pub class: Class,
}

/// `plan`'s answer: node 0 is the root, then the line after the cut, then the copies.
#[derive(Debug, Default)]
pub struct Shape {
    pub nodes: Vec<Node>,
    pub place: HashMap<String, (usize, Kind)>,
    pub line: Vec<String>,
    pub refs: Vec<Move>,
    pub notes: Vec<(String, Vec<(usize, String)>)>,
}

/// The PR number a line subject ends with: `title (#issue) (#pr)`.
pub fn pr_of(message: &str) -> Option<u64> {
    let s = message.lines().next()?.trim_end().strip_suffix(')')?;
    s.rsplit_once("(#")?.1.parse().ok()
}

type ByNode<'a> = BTreeMap<usize, Vec<(&'a str, &'a str)>>;

fn reach(old: &Old, starts: &[&str], seen: &mut HashSet<String>) -> Vec<String> {
    let (mut out, mut todo) = (vec![], starts.iter().map(ToString::to_string).collect::<Vec<_>>());
    while let Some(c) = todo.pop() {
        if !seen.insert(c.clone()) {
            continue;
        }
        if let Some(k) = old.commits.get(&c) {
            todo.extend(k.parents.iter().cloned());
            out.push(c);
        }
    }
    out
}

fn node(c: &Commit, parents: Vec<usize>) -> Node {
    Node {
        tree: c.tree.clone(),
        parents,
        author: c.author.clone(),
        committer: c.committer.clone(),
        message: c.message.clone(),
    }
}

/// Place every old commit and ref (`line`: the branch name, `cut`: a commit on its first-parent chain); pure.
pub fn plan(old: &Old, line: &str, cut: &str, root_message: &str) -> Result<Shape, String> {
    let get = |s: &str| {
        old.commits
            .get(s)
            .ok_or_else(|| format!("commit {s:.9} is not in the copy"))
    };
    let line_ref = format!("refs/heads/{line}");
    let tip = (old.refs.iter().find(|r| r.0 == line_ref)).ok_or_else(|| format!("no branch {line}"))?;
    let mut chain = vec![];
    let mut at = tip.1.clone();
    while at != cut {
        let first = get(&at)?.parents.first().cloned();
        chain.push(std::mem::replace(
            &mut at,
            first.ok_or(format!("{cut:.9} is not on {line}"))?,
        ));
    }
    chain.reverse();
    let c = get(cut)?;
    let mut s = Shape {
        nodes: vec![Node {
            parents: vec![],
            author: c.committer.replacen("committer ", "author ", 1),
            message: root_message.to_string(),
            ..node(c, vec![])
        }],
        line: chain.clone(),
        ..Shape::default()
    };
    let mut seen = HashSet::new();
    for a in reach(old, &[cut], &mut seen) {
        s.place.insert(a, (0, Kind::Root));
    }
    s.place.insert(cut.to_string(), (0, Kind::Same));
    let pulls: HashMap<&str, &str> = (old.refs.iter())
        .filter_map(|(n, v)| Some((n.strip_prefix("refs/pull/")?.strip_suffix("/head")?, v.as_str())))
        .collect();
    for l in &chain {
        let n = s.nodes.len();
        let k = get(l)?;
        s.nodes.push(node(k, vec![n - 1]));
        s.place.insert(l.clone(), (n, Kind::Same));
        seen.insert(l.clone());
        let pr = pr_of(&k.message).map(|p| p.to_string());
        let mut from: Vec<&str> = k.parents.iter().skip(1).map(String::as_str).collect();
        from.extend(pr.as_deref().and_then(|p| pulls.get(p)));
        for h in reach(old, &from, &mut seen) {
            s.place.insert(h, (n, Kind::Held));
        }
    }
    let mut rooted: Vec<bool> = vec![true; s.nodes.len()];
    let mut refs: Vec<&(String, String)> = old.refs.iter().collect();
    refs.sort();
    for (name, tip) in refs.iter().map(|r| (&r.0, &r.1)) {
        if name.starts_with("refs/notes/") || name.starts_with("refs/pull/") {
            continue;
        }
        let mut todo = vec![(tip.clone(), false)];
        while let Some((c, ready)) = todo.pop() {
            if s.place.contains_key(&c) {
                continue;
            }
            let k = get(&c)?;
            if !ready {
                todo.push((c, true));
                todo.extend(k.parents.iter().map(|p| (p.clone(), false)));
                continue;
            }
            let ps: Vec<usize> = (k.parents.iter())
                .filter_map(|p| s.place.get(p).filter(|x| x.1 == Kind::Same).map(|x| x.0))
                .collect();
            rooted.push(ps.iter().any(|p| rooted.get(*p) == Some(&true)));
            s.place.insert(c, (s.nodes.len(), Kind::Same));
            s.nodes.push(node(k, ps));
        }
        let (n, kind) = s.place.get(tip.as_str()).copied().unwrap_or((0, Kind::Root));
        let class = match kind {
            _ if *name == line_ref => Class::Line,
            Kind::Same if n <= chain.len() => Class::SameTree,
            Kind::Same if rooted.get(n) == Some(&true) => Class::Carried,
            Kind::Same => Class::Detached,
            Kind::Held | Kind::Root => Class::Merged,
        };
        let to = (class != Class::Merged).then_some(n);
        s.refs.push(Move {
            name: name.clone(),
            old: tip.clone(),
            to,
            class,
        });
    }
    let mut notes: BTreeMap<&str, ByNode> = (old.refs.iter())
        .filter(|r| r.0.starts_with("refs/notes/"))
        .map(|r| (r.0.as_str(), BTreeMap::new()))
        .collect();
    for (r, c, text) in &old.notes {
        let Some(&(n, kind)) = s.place.get(c).filter(|p| p.1 != Kind::Root && p.0 > 0) else {
            continue;
        };
        let when = old.commits.get(c).map_or("", |k| k.committer.as_str());
        let key = if kind == Kind::Same { "" } else { when };
        if let Some(at) = notes.get_mut(r.as_str()) {
            at.entry(n).or_default().push((key, text));
        }
    }
    for (r, by) in notes {
        let mut one = vec![];
        for (n, mut texts) in by {
            texts.sort_by_key(|t| (t.0.rsplit(' ').nth(1).and_then(|t| t.parse::<u64>().ok()), t.1));
            let own = texts.iter().find(|t| t.0.is_empty()).map(|t| t.1.trim().to_string());
            let joined = || texts.iter().map(|t| t.1.trim()).collect::<Vec<_>>().join("\n\n");
            one.push((n, own.unwrap_or_else(joined) + "\n"));
        }
        s.refs.push(Move {
            name: r.to_string(),
            old: old
                .refs
                .iter()
                .find(|x| x.0 == r)
                .map(|x| x.1.clone())
                .unwrap_or_default(),
            to: None,
            class: Class::Notes,
        });
        s.notes.push((r.to_string(), one));
    }
    Ok(s)
}
