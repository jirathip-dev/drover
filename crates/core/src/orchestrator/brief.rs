use super::chat::Seat;
use crate::config::{expand, Config};
use crate::events::Event;

const ROLE: &str = include_str!("../../roles/orchestrator/role.md");
const CHAT: &str = include_str!("../../roles/orchestrator/chat.md");

pub fn thread(cfg: &Config, t: &Event, fleet: &str) -> String {
    let (id, title) = (t.str("thread").unwrap_or_default(), t.str("title").unwrap_or_default());
    let repo = cfg.fleet.get(&t.fleet).map_or("all repos".into(), |f| {
        format!("{} (fleet {}, checkout {})", f.repo, t.fleet, f.local)
    });
    format!(
        "# Orchestrator brief: thread {id}\n\n{}\n\nThread: {id}, {title}\nRepo: {repo}\nFleet for asks: {fleet}\n{}",
        crate::spawn::role_prompt("orchestrator").trim_end(),
        standing(cfg)
    )
}

pub fn chat(cfg: &Config, seat: &Seat) -> String {
    format!(
        "# Orchestrator brief: cooee chat of {}\n\n{}\n\n{}\n\n{}\n\nOrchestrator: {}, {}\nFleets: {}\nFleet for asks: {}\n{}",
        seat.id,
        ROLE.trim_end(),
        CHAT.trim_end(),
        crate::cli::usage_doc().trim_end(),
        seat.id,
        seat.name,
        seat.fleets.join(", "),
        seat.fleet,
        standing(cfg)
    )
}

fn standing(cfg: &Config) -> String {
    let mut s = String::new();
    let c = &cfg.orchestrator;
    if let Some(p) = &c.instructions {
        let text = std::fs::read_to_string(expand(p)).unwrap_or_else(|e| format!("({p} unreadable: {e})"));
        s += &format!("\n## Instructions ({p})\n\n{}\n", text.trim_end());
    }
    let files = |d: &Option<String>| {
        let all = d.as_deref().and_then(|d| std::fs::read_dir(expand(d)).ok());
        let mut v: Vec<_> = all.into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    for m in files(&c.memory)
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
    {
        let text = std::fs::read_to_string(m).unwrap_or_default();
        s += &format!("\n## Memory: {}\n\n{}\n", m.display(), text.trim_end());
    }
    s += "\n## Capability groups ([orchestrator] allow)\n\n";
    for (g, verbs) in crate::config::GROUPS {
        let on = ["off", "on"][usize::from(c.groups().contains(g))];
        s += &format!("- `{g}` ({on}): {verbs}\n");
    }
    s += "\nA verb of an off group is refused `GROUP_OFF`: ask the owner for an `[orchestrator] allow` change.\n";
    let skills = files(&c.skills);
    if !skills.is_empty() {
        s += "\n## Skills (read one when it applies)\n\n";
        s += &skills
            .iter()
            .map(|p| format!("- {}\n", p.display()))
            .collect::<String>();
    }
    s
}
