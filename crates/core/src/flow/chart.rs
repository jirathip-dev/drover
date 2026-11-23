use super::table::{Escalate, Flow, Since, Table};
use std::fmt::Write as _;

/// A flow's state edges from its rows' `to`, each with the ids of the rows that lead along it.
pub fn edges(flow: &Flow) -> Vec<(String, String, Vec<String>)> {
    let mut out: Vec<(String, String, Vec<String>)> = vec![];
    for f in &flow.files {
        for from in &f.states {
            for r in &f.rows {
                let short = r.id.rsplit('/').next().unwrap_or(&r.id).to_string();
                for to in &r.to {
                    match out.iter_mut().find(|e| &e.0 == from && &e.1 == to) {
                        Some(e) => e.2.push(short.clone()),
                        None => out.push((from.clone(), to.clone(), vec![short.clone()])),
                    }
                }
            }
        }
    }
    out
}

/// Every flow's rows as `docs/flow.md` holds them.
pub fn markdown(t: &Table) -> String {
    let mut m = String::from(
        "# Flow table\n\nGenerated from `crates/core/flow/` (#1649) by `just gen`; CI fails when this file differs. \
        Rows are tried top to bottom and the first match is the row; a blank fact cell is any value. \
        The design is `docs/architecture/flow-table/`.\n",
    );
    for flow in &t.flows {
        write!(m, "\n## Flow `{}`\n", flow.name).ok();
        for f in &flow.files {
            let states: Vec<String> = f.states.iter().map(|s| format!("`{s}`")).collect();
            let reads: Vec<&str> = f.reads.iter().map(|x| x.name()).collect();
            let reads = if reads.is_empty() {
                "nothing".into()
            } else {
                reads.join(", ")
            };
            let flight = if f.in_flight { " · in flight" } else { "" };
            write!(
                m,
                "\n### {} (file `{}`) · reads {reads}{flight}\n\n",
                states.join(", "),
                f.name
            )
            .ok();
            let facts: String = f.reads.iter().map(|x| format!(" {} |", x.name())).collect();
            let rule: String = f.reads.iter().map(|_| "---|").collect();
            writeln!(m, "| id |{facts} do | owner | timeout | then | esc | to | say |").ok();
            writeln!(m, "|---|{rule}---|---|---|---|---|---|---|").ok();
            for r in &f.rows {
                let cells: String = f
                    .reads
                    .iter()
                    .map(|x| match r.when.0.get(x).map(|v| v.join(", ")) {
                        Some(v) if v != "*" => format!(" {v} |"),
                        _ => " |".into(),
                    })
                    .collect();
                let since = match r.since {
                    Since::Entered => "",
                    Since::Heard => " · heard",
                    Since::HostHeard => " · host heard",
                    Since::Reset => " · reset",
                };
                let then: Vec<&str> = r.then.iter().map(|a| a.name()).collect();
                let esc = match r.escalate {
                    Escalate::Ladder => "",
                    Escalate::Orchestrator => "orch",
                    Escalate::Owner => "owner",
                };
                let short = r.id.rsplit('/').next().unwrap_or(&r.id);
                writeln!(
                    m,
                    "| `{short}` |{cells} {} | {} | `{}`{since} | {} | {esc} | {} | {} |",
                    r.act.name(),
                    r.owner.name(),
                    r.timeout.name(),
                    then.join(", "),
                    r.to.join(", "),
                    r.say
                )
                .ok();
            }
        }
    }
    m
}
