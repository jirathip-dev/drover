use super::plan::{Commit, Node, Old, Shape};
use crate::runner::{argv, ok, Runner};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;

const CHUNK: usize = 1000;

fn git(r: &dyn Runner, dir: &Path, a: &[&str]) -> Result<String> {
    ok(r, "git", &[&["-C", &dir.to_string_lossy()][..], a].concat())
}

fn git_in(r: &dyn Runner, dir: &Path, a: &[&str], input: &str) -> Result<String> {
    let o = r.run_in("git", &argv(&[&["-C", &dir.to_string_lossy()][..], a].concat()), input)?;
    if o.code != 0 {
        bail!("git {}: {}", a.join(" "), o.stderr.trim());
    }
    Ok(o.stdout)
}

/// Bring the bare copy at `dir` to `url`'s every ref, PR heads included; a push from it never moves its refs.
pub fn mirror(r: &dyn Runner, url: &str, dir: &Path) -> Result<()> {
    if !dir.join("HEAD").exists() {
        ok(r, "git", &["clone", "-q", "--mirror", url, &dir.to_string_lossy()])?;
        git(r, dir, &["config", "--unset-all", "remote.origin.fetch"])?;
        git(r, dir, &["config", "--unset", "remote.origin.mirror"])?;
    }
    git(r, dir, &["fetch", "-q", "--prune", "origin", "+refs/*:refs/*"]).map(drop)
}

fn objects(r: &dyn Runner, dir: &Path, ids: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = String::new();
    for chunk in ids.chunks(CHUNK) {
        out += &git_in(r, dir, &["cat-file", "--batch"], &(chunk.join("\n") + "\n"))?;
    }
    let (mut rest, mut all) = (out.as_str(), vec![]);
    while let Some((head, body)) = rest.split_once('\n') {
        let mut w = head.split(' ');
        let (id, size) = (w.next().unwrap_or_default(), w.nth(1).and_then(|n| n.parse().ok()));
        let size: usize = size.with_context(|| format!("cat-file: {head}"))?;
        all.push((
            id.to_string(),
            body.get(..size).context("cat-file: short object")?.to_string(),
        ));
        rest = body.get(size + 1..).unwrap_or_default();
    }
    Ok(all)
}

fn commit(raw: &str) -> Commit {
    let (head, message) = raw.split_once("\n\n").unwrap_or((raw, ""));
    let mut c = Commit {
        tree: String::new(),
        parents: vec![],
        author: String::new(),
        committer: String::new(),
        message: message.to_string(),
    };
    for l in head.lines() {
        match l.split_once(' ') {
            Some(("tree", t)) => c.tree = t.to_string(),
            Some(("parent", p)) => c.parents.push(p.to_string()),
            Some(("author", _)) => c.author = l.to_string(),
            Some(("committer", _)) => c.committer = l.to_string(),
            _ => {}
        }
    }
    c
}

/// The copy at `dir` read once into values: every commit of a branch, tag or PR head, the refs and the notes.
pub fn read(r: &dyn Runner, dir: &Path, open: Vec<(u64, String)>) -> Result<Old> {
    let refs = git(
        r,
        dir,
        &["for-each-ref", "--format=%(refname) %(objectname) %(objecttype)"],
    )?;
    let mut old = Old { open, ..Old::default() };
    for l in refs.lines() {
        match l.split(' ').collect::<Vec<_>>().as_slice() {
            [n, v, "commit"] => old.refs.push(((*n).to_string(), (*v).to_string())),
            [n, _, kind] => bail!("{n} is a {kind}; compact moves commits only"),
            _ => {}
        }
    }
    let ids = git(r, dir, &["rev-list", "--branches", "--tags", "--glob=refs/pull/*"])?;
    let ids: Vec<String> = ids.lines().map(String::from).collect();
    for (id, raw) in objects(r, dir, &ids)? {
        old.commits.insert(id, commit(&raw));
    }
    let notes: Vec<String> = (old.refs.iter().map(|x| x.0.clone()))
        .filter(|n| n.starts_with("refs/notes/"))
        .collect();
    for n in notes {
        let rows = git(r, dir, &["ls-tree", "-r", &n])?;
        let rows: Vec<(String, String)> = (rows.lines())
            .filter_map(|l| l.split_once('\t'))
            .filter_map(|(m, p)| Some((m.rsplit(' ').next()?.to_string(), p.replace('/', ""))))
            .collect();
        let blobs: Vec<String> = rows.iter().map(|x| x.0.clone()).collect();
        let text: HashMap<String, String> = objects(r, dir, &blobs)?.into_iter().collect();
        for (b, c) in rows {
            old.notes
                .push((n.clone(), c, text.get(&b).cloned().unwrap_or_default()));
        }
    }
    Ok(old)
}

/// The new ids by node, and each notes ref with its new commit.
pub type Written = (Vec<String>, Vec<(String, String)>);

fn raw(n: &Node, ids: &[String]) -> String {
    let mut s = format!("tree {}\n", n.tree);
    for p in &n.parents {
        s += &format!("parent {}\n", ids.get(*p).map_or("", String::as_str));
    }
    format!("{s}{}\n{}\n\n{}", n.author, n.committer, n.message)
}

/// Write the shape's commits and rebuilt notes into the copy; the new ids by node, and each notes ref's new commit.
pub fn write(r: &dyn Runner, dir: &Path, s: &Shape) -> Result<Written> {
    let mut ids: Vec<String> = Vec::with_capacity(s.nodes.len());
    for n in &s.nodes {
        let id = git_in(r, dir, &["hash-object", "-t", "commit", "-w", "--stdin"], &raw(n, &ids))?;
        ids.push(id.trim().to_string());
    }
    let mut notes = vec![];
    for (name, rows) in &s.notes {
        let mut tree = String::new();
        for (n, text) in rows {
            let blob = git_in(r, dir, &["hash-object", "-w", "--stdin"], text)?;
            tree += &format!(
                "100644 blob {}\t{}\n",
                blob.trim(),
                ids.get(*n).map_or("", String::as_str)
            );
        }
        let tree = git_in(r, dir, &["mktree"], &tree)?;
        let tip = commit(&git(r, dir, &["cat-file", "commit", name])?);
        let n = Node {
            tree: tree.trim().to_string(),
            parents: vec![],
            author: tip.author,
            committer: tip.committer,
            message: tip.message,
        };
        let id = git_in(r, dir, &["hash-object", "-t", "commit", "-w", "--stdin"], &raw(&n, &[]))?;
        notes.push((name.clone(), id.trim().to_string()));
    }
    Ok((ids, notes))
}

/// The trees of `ids` as git reads them, in order.
pub fn trees(r: &dyn Runner, dir: &Path, ids: &[&str]) -> Result<Vec<String>> {
    let mut out = String::new();
    for chunk in ids.chunks(CHUNK) {
        let input: String = chunk.iter().map(|i| format!("{i}^{{tree}}\n")).collect();
        out += &git_in(r, dir, &["cat-file", "--batch-check=%(objectname)"], &input)?;
    }
    Ok(out.lines().map(String::from).collect())
}
