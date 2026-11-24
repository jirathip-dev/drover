pub mod chart;
pub mod decide;
pub mod episode;
pub mod facts;
pub mod shadow;
pub mod table;

pub use decide::{decide, Clock, Step};
pub use episode::Episode;
pub use facts::Cond;
pub use table::{table, Table};

/// `drover waiting --flow`'s line under issue `s`: its state, condition and row, and each fact's evidence.
pub fn line(seen: Option<&facts::Seen>, s: Option<&crate::events::IssueState>) -> anyhow::Result<String> {
    let (Some(seen), Some(s)) = (seen, s) else {
        return Ok(String::new());
    };
    let (cond, why) = seen.cond(s);
    let name = s.state.split(['(', '+']).next().unwrap_or_default();
    let file = table()?
        .flow("issue")
        .and_then(|f| f.files.iter().find(|x| x.states.iter().any(|n| n == name)));
    let row = file.and_then(|f| f.row(&cond)).map_or("no row", |r| r.id.as_str());
    Ok(format!("\n  flow: {name} · {cond} · {row} ({})", why.join("; ")))
}
