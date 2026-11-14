use crate::config::{Limits, Role};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub name: String,
    /// build (pushes code) | check (PASS or FAIL) | gate (drover's own: CI, serial, mergeable)
    pub kind: String,
    pub role: Option<String>,
    /// outcome -> a stage, `merge` or `park`; `<build>:update` is an update round.
    #[serde(default)]
    pub next: BTreeMap<String, String>,
    /// A check's review rounds, a gate's update rounds; out of rounds goes to park.
    pub rounds: Option<u32>,
    /// The fleet.toml line of its `[[…stage]]` header; 0 for the built-in pipeline.
    #[serde(skip)]
    pub line: usize,
    /// #417: the check whose round count this one's FAILs park on (the built-in verify: rev's).
    #[serde(skip)]
    pub share: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    #[serde(default)]
    pub stage: Vec<Stage>,
    #[serde(skip)]
    pub builtin: bool,
}

/// One broken rule: its word, the sentence the board shows, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub rule: &'static str,
    pub msg: String,
    pub stage: String,
    pub line: usize,
}

/// The outcomes a stage of `kind` must map.
pub fn outcomes(kind: &str) -> &'static [&'static str] {
    match kind {
        "build" => &["push"],
        "check" => &["PASS", "FAIL"],
        "gate" => &["ok", "ci_red", "conflict"],
        _ => &[],
    }
}

fn goes(outcome: &str) -> Option<&'static [&'static str]> {
    match outcome {
        "push" => Some(&["check"]),
        "PASS" => Some(&["check", "gate", "merge"]),
        "ok" => Some(&["merge"]),
        "FAIL" | "ci_red" | "conflict" => Some(&["build", "park"]),
        _ => None,
    }
}

fn stage(name: &str, kind: &str, role: Option<&str>, rounds: Option<u32>, next: &[(&str, &str)]) -> Stage {
    Stage {
        name: name.into(),
        kind: kind.into(),
        role: role.map(String::from),
        next: next.iter().map(|(o, t)| ((*o).into(), (*t).into())).collect(),
        rounds,
        line: 0,
        share: None,
    }
}

impl Pipeline {
    /// Today's loop: `[limits] review_rounds` on rev, `update_rounds` on the gate; #417: with a `verify` role, rev's PASS goes to a verify check whose FAILs count against rev's rounds.
    pub fn builtin(l: &Limits, verify: bool) -> Pipeline {
        let pass = if verify { "verify" } else { "gates" };
        let mut v = stage(
            "verify",
            "check",
            Some("verify"),
            Some(l.review_rounds),
            &[("PASS", "gates"), ("FAIL", "impl")],
        );
        v.share = Some("rev".into());
        let mut stage = vec![
            stage("impl", "build", Some("impl"), None, &[("push", "rev")]),
            stage(
                "rev",
                "check",
                Some("rev"),
                Some(l.review_rounds),
                &[("PASS", pass), ("FAIL", "impl")],
            ),
            stage(
                "gates",
                "gate",
                None,
                Some(l.update_rounds),
                &[("ok", "merge"), ("ci_red", "impl"), ("conflict", "impl:update")],
            ),
        ];
        if verify {
            stage.insert(2, v);
        }
        Pipeline { stage, builtin: true }
    }

    /// #417: the check whose rounds a FAIL of check `stage` counts toward: its `share`, else itself.
    pub fn counts<'a>(&'a self, stage: &'a str) -> &'a str {
        self.get(stage).and_then(|s| s.share.as_deref()).unwrap_or(stage)
    }

    pub fn get(&self, name: &str) -> Option<&Stage> {
        self.stage.iter().find(|s| s.name == name)
    }

    /// The first stage that runs `role`.
    pub fn of_role(&self, role: &str) -> Option<&Stage> {
        self.stage.iter().find(|s| s.role.as_deref() == Some(role))
    }

    /// The start: the first stage's role (a build).
    pub fn start(&self) -> &str {
        self.stage.first().and_then(|s| s.role.as_deref()).unwrap_or("impl")
    }

    /// Where `outcome` of stage `from` goes: (stage or end, an update round).
    pub fn next(&self, from: &str, outcome: &str) -> Option<(&str, bool)> {
        let t = self.get(from)?.next.get(outcome)?;
        Some(t.strip_suffix(":update").map_or((t.as_str(), false), |b| (b, true)))
    }

    /// The role a stage name runs, if it is a stage with one.
    pub fn role(&self, stage: &str) -> Option<&str> {
        self.get(stage)?.role.as_deref()
    }

    pub fn is_check(&self, role: &str) -> bool {
        self.of_role(role).is_some_and(|s| s.kind == "check")
    }

    /// Every check stage: merge needs a PASS from each at the head.
    pub fn checks(&self) -> impl Iterator<Item = &Stage> {
        self.stage.iter().filter(|s| s.kind == "check")
    }

    /// The stage `stage`'s verdicts count against: itself, or for a gate the check its `ci_red` fix is reviewed by.
    pub fn loop_of<'a>(&'a self, stage: &'a str) -> &'a str {
        match self.get(stage) {
            Some(s) if s.kind == "gate" => self
                .next(stage, "ci_red")
                .and_then(|(b, _)| self.next(b, "push"))
                .map_or(stage, |c| c.0),
            _ => stage,
        }
    }

    /// The gate's update rounds (`conflict → <build>:update`).
    pub fn update_limit(&self) -> u32 {
        let g = self.stage.iter().find(|s| s.kind == "gate");
        g.and_then(|g| g.rounds).unwrap_or(u32::MAX)
    }

    /// The first gate stage (CI red and conflicts route from it).
    pub fn gate(&self) -> &str {
        self.stage
            .iter()
            .find(|s| s.kind == "gate")
            .map_or("gates", |s| s.name.as_str())
    }

    /// The stage a role's spawn enters: `role` names a stage or a role.
    pub fn stage_of<'a>(&'a self, role: &'a str) -> &'a str {
        let lesson = (role == "lesson").then(|| self.stage.first()).flatten();
        self.get(role)
            .or(self.of_role(role))
            .or(lesson)
            .map_or(role, |s| s.name.as_str())
    }

    fn edges(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.stage.iter().flat_map(|s| {
            s.next
                .iter()
                .map(move |(o, t)| (s.name.as_str(), o.as_str(), t.split(':').next().unwrap_or(t)))
        })
    }

    fn reach(&self, stop: impl Fn(&Stage) -> bool) -> BTreeSet<&str> {
        let mut seen = BTreeSet::new();
        let mut todo: Vec<&str> = self.stage.first().map(|s| s.name.as_str()).into_iter().collect();
        while let Some(n) = todo.pop() {
            if seen.insert(n) && self.get(n).is_some_and(|s| !stop(s)) {
                todo.extend(self.edges().filter(|e| e.0 == n).map(|e| e.2));
            }
        }
        seen
    }

    fn unlimited_cycle(&self, gone: &BTreeSet<String>) -> Option<Vec<String>> {
        fn dfs<'a>(
            p: &'a Pipeline,
            n: &'a str,
            path: &mut Vec<&'a str>,
            free: &dyn Fn(&str) -> bool,
        ) -> Option<Vec<String>> {
            if let Some(i) = path.iter().position(|x| *x == n) {
                return Some(path[i..].iter().chain([&n]).map(|s| (*s).to_string()).collect());
            }
            path.push(n);
            for (_, _, t) in p.edges().filter(|e| e.0 == n && free(e.2)) {
                if let Some(c) = dfs(p, t, path, free) {
                    return Some(c);
                }
            }
            path.pop();
            None
        }
        let free = |n: &str| {
            self.get(n)
                .is_some_and(|s| (s.kind != "check" || s.rounds.is_none()) && !gone.contains(n))
        };
        let names = self.stage.iter().map(|s| s.name.as_str()).filter(|n| free(n));
        names.into_iter().find_map(|n| dfs(self, n, &mut vec![], &free))
    }

    /// Every broken rule, in the board's order (docs/pipeline.md); empty = valid. The built-in one is not checked.
    pub fn validate(&self, fleet: &str, roles: &BTreeMap<String, Role>) -> Vec<Problem> {
        let mut out = vec![];
        let mut bad = |rule, s: &Stage, msg: String| {
            out.push(Problem {
                rule,
                msg,
                stage: s.name.clone(),
                line: s.line,
            });
        };
        if self.builtin {
            return vec![];
        }
        let Some(first) = self.stage.first() else {
            return vec![Problem {
                rule: "start",
                msg: "the pipeline has no stages".into(),
                stage: String::new(),
                line: 0,
            }];
        };
        if first.kind != "build" {
            let msg = format!(
                "the first stage, {}, is a {}: a queued issue needs code first",
                first.name, first.kind
            );
            bad("start", first, msg);
        }
        let mut names = BTreeSet::new();
        for s in &self.stage {
            if ["merge", "park"].contains(&s.name.as_str()) || !names.insert(s.name.as_str()) || s.name.contains(':') {
                bad(
                    "name",
                    s,
                    format!("stage name {} is taken: names are unique, not merge or park", s.name),
                );
            }
            if outcomes(&s.kind).is_empty() {
                bad(
                    "kind",
                    s,
                    format!("stage {}: kind {} is not build, check or gate", s.name, s.kind),
                );
            }
        }
        let reach = self.reach(|_| false);
        for s in self.stage.iter().filter(|s| !reach.contains(s.name.as_str())) {
            bad(
                "unreachable",
                s,
                format!("stage {} can't be reached: no outcome leads to it", s.name),
            );
        }
        let mut ends: BTreeSet<&str> = ["merge", "park"].into();
        while let Some(e) = self.edges().find(|e| ends.contains(e.2) && !ends.contains(e.0)) {
            ends.insert(e.0);
        }
        let stuck: Vec<&Stage> = self.stage.iter().filter(|s| !ends.contains(s.name.as_str())).collect();
        if let Some(s) = stuck.first() {
            let rest: Vec<&str> = stuck[1..].iter().map(|s| s.name.as_str()).collect();
            let nor = if rest.is_empty() {
                String::new()
            } else {
                format!(" (nor can {})", rest.join(", "))
            };
            bad(
                "no end",
                s,
                format!("stage {} can't reach an end{nor}: every path loops back", s.name),
            );
        }
        let mut gone = BTreeSet::new();
        while let Some(c) = self.unlimited_cycle(&gone) {
            let on = c
                .iter()
                .find(|n| self.get(n).is_some_and(|s| s.kind == "check"))
                .unwrap_or(&c[0]);
            let s = self.get(on).unwrap_or(first);
            bad(
                "no limit",
                s,
                format!("loop {} has no round limit: set rounds on {on}", c.join(" → ")),
            );
            gone.extend(c);
        }
        for s in &self.stage {
            let want = outcomes(&s.kind);
            for o in want.iter().filter(|o| !s.next.contains_key(**o)) {
                bad("unmapped", s, format!("{} {} has no {o} outcome", s.kind, s.name));
            }
            for (o, t) in &s.next {
                let (to, update) = t.split_once(':').map_or((t.as_str(), ""), |(a, b)| (a, b));
                if !want.contains(&o.as_str()) && !want.is_empty() {
                    bad(
                        "unmapped",
                        s,
                        format!("stage {}: outcome {o} is not one of {}", s.name, want.join(", ")),
                    );
                } else if !["merge", "park"].contains(&to) && self.get(to).is_none() {
                    bad(
                        "unmapped",
                        s,
                        format!("stage {}: {o} goes to {to}, which is not a stage", s.name),
                    );
                } else if !update.is_empty() && (update != "update" || self.get(to).is_none_or(|b| b.kind != "build")) {
                    bad(
                        "unmapped",
                        s,
                        format!("stage {}: {o} → {t}: only a build stage takes :update", s.name),
                    );
                } else if let Some(want) = goes(o).filter(|w| !w.contains(&self.get(to).map_or(to, |s| &s.kind))) {
                    bad(
                        "unmapped",
                        s,
                        format!(
                            "stage {}: {o} goes to {to}; it must go to a {}",
                            s.name,
                            want.join(" or ")
                        ),
                    );
                }
            }
        }
        for s in self.stage.iter().filter(|s| s.kind != "gate") {
            match s.role.as_deref() {
                None => bad("no role", s, format!("stage {}: a {} needs a role", s.name, s.kind)),
                Some(r @ ("design" | "architect")) => bad(
                    "no role",
                    s,
                    format!("stage {}: {r} is owner-only, never a stage", s.name),
                ),
                Some(r) if !roles.contains_key(r) => {
                    bad(
                        "no role",
                        s,
                        format!(
                            "stage {}: role {r} is not defined; add [fleet.{fleet}.role.{r}]",
                            s.name
                        ),
                    );
                }
                _ => {}
            }
        }
        let builds: BTreeSet<&str> = self
            .stage
            .iter()
            .filter(|s| s.kind == "build")
            .filter_map(|s| s.role.as_deref())
            .collect();
        for s in self.checks() {
            if let Some(r) = s.role.as_deref().filter(|r| builds.contains(r)) {
                let msg = format!(
                    "check {} uses role {r}, which also builds: a check can't review its own work",
                    s.name
                );
                bad("self-review", s, msg);
            }
        }
        let ungated = self.reach(|s| s.kind == "gate");
        if let Some((from, o, _)) = self
            .edges()
            .find(|e| e.2 == "merge" && ungated.contains(e.0) && self.get(e.0).is_some_and(|s| s.kind != "gate"))
        {
            let s = self.get(from).unwrap_or(first);
            bad(
                "no gate",
                s,
                format!("{from}'s {o} goes straight to merge: route it through a gate"),
            );
        }
        out
    }
}
