use drover::events::STATES;
use drover::flow::chart::{edges, markdown};
use drover::flow::facts::Fact;
use drover::flow::table::{parse, table, Timeout};
use drover::flow::Cond;

const OUTSIDE: [(&str, &str, &str); 3] = [
    (
        "queued",
        "closed",
        "the issue closed on GitHub: the fold's own move, from any state",
    ),
    ("impl", "queued", "the owner's `kill`: the fold's own move"),
    (
        "ready",
        "blocked",
        "adr.md: blocked is no longer entered; a refused merge is a failed try of ready/merge",
    ),
];

fn base(state: &str) -> &str {
    state.split('(').next().unwrap_or(state)
}

#[test]
fn every_state_and_condition_has_one_row_with_an_owner_and_a_timeout() {
    let t = table().unwrap();
    let conds: Vec<Cond> = Cond::every().collect();
    assert_eq!(conds.len(), 16_800);
    let mut held = vec![];
    for flow in &t.flows {
        let states: Vec<&String> = flow.files.iter().flat_map(|f| &f.states).collect();
        for f in &flow.files {
            let mut hit = vec![false; f.rows.len()];
            for c in &conds {
                let i = f.rows.iter().position(|r| r.when.matches(c));
                let i = i.unwrap_or_else(|| panic!("{}/{}: no row for {c}", flow.name, f.name));
                hit[i] = true;
            }
            for (r, hit) in f.rows.iter().zip(hit) {
                assert!(hit, "{}: no condition reaches it", r.id);
                assert!(
                    r.to.iter().all(|s| states.contains(&s)),
                    "{}: `to` names no state of {}",
                    r.id,
                    flow.name
                );
                if r.timeout == Timeout::OwnerHeld {
                    held.push(r.id.clone());
                }
            }
            for &fact in &f.reads {
                for v in fact.values() {
                    let named = f.rows.iter().any(|r| r.when.names(fact, v));
                    assert!(named, "{}/{}: no row names {} = {v}", flow.name, f.name, fact.name());
                }
            }
        }
    }
    assert_eq!(held, ["issue/parked/parked", "pass/paused/paused"]);
    assert_eq!(Fact::ALL.len(), 6);
}

#[test]
fn the_rows_cover_every_edge_of_todays_states() {
    let issue = table().unwrap().flow("issue").unwrap();
    let rows = edges(issue);
    let covered = |from: &str, to: &str| rows.iter().any(|e| e.0 == base(from) && e.1 == base(to));
    for (from, out) in STATES {
        for (to, _) in *out {
            let outside = OUTSIDE.iter().any(|o| o.0 == *from && o.1 == *to);
            assert!(
                covered(from, to) || outside,
                "{from} -> {to}: no row's `to` leads along it"
            );
        }
    }
    for (from, to, why) in OUTSIDE {
        assert!(
            !covered(from, to),
            "{from} -> {to} has a row now; drop it from OUTSIDE ({why})"
        );
        let known = STATES
            .iter()
            .any(|(f, out)| *f == from && out.iter().any(|o| o.0 == to));
        assert!(known, "{from} -> {to} is no edge of today's STATES");
    }
    let states: Vec<&str> = issue.files.iter().flat_map(|f| &f.states).map(String::as_str).collect();
    assert!(STATES.iter().all(|(s, _)| states.contains(&base(s))));
}

const ROW: &str = r#"
flow = "card"
states = ["posted"]
reads = []
ignores = ["lane", "host", "pr", "verdict", "ask", "turn"]

[[row]]
id = "waiting"
when = {}
do = "wait"
owner = "owner"
timeout = "card_ttl_minutes"
say = "waits"
"#;

fn refused(text: &str) -> String {
    format!("{:#}", parse(&[("card/posted.toml", text)]).unwrap_err())
}

#[test]
fn a_row_with_no_owner_or_timeout_or_an_unknown_value_does_not_load() {
    assert!(parse(&[("card/posted.toml", ROW)]).is_ok());
    assert!(refused(&ROW.replace("owner = \"owner\"\n", "")).contains("missing field `owner`"));
    assert!(refused(&ROW.replace("timeout = \"card_ttl_minutes\"\n", "")).contains("missing field `timeout`"));
    assert!(refused(&ROW.replace("card_ttl_minutes", "card_ttl")).contains("no [limits] key"));
    assert!(refused(&ROW.replace("do = \"wait\"", "do = \"loiter\"")).contains("loiter"));
    assert!(refused(&ROW.replace("say = ", "colour = \"red\"\nsay = ")).contains("colour"));
    let lane = ROW
        .replace("reads = []", "reads = [\"lane\"]")
        .replace("[\"lane\", \"host\"", "[\"host\"");
    assert!(refused(&lane).contains("exactly the facts in `reads`"));
    assert!(refused(&lane.replace("when = {}", "when = { lane = [\"runing\"] }")).contains("card/posted.toml"));
    assert!(parse(&[(
        "card/posted.toml",
        &lane.replace("when = {}", "when = { lane = \"*\" }")
    )])
    .is_ok());
    assert!(refused(&ROW.replace("ignores = [\"lane\", ", "ignores = [")).contains("every fact once"));
}

#[test]
fn docs_flow_md_is_the_table() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/flow.md");
    let doc = markdown(table().unwrap());
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(path, &doc).unwrap();
    }
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        on_disk == doc,
        "docs/flow.md is stale: `just gen` (BLESS=1 cargo test -p drover --test flow_table)"
    );
    assert!(doc.contains("| `pushed` |"));
}
