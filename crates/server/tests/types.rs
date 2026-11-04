//! #375: the board's TS types (`assets/types/api.d.ts`) come from the Rust payload types; `just types` writes them
//! (`export_bindings`), and `tsc --noEmit` checks serde's JSON of each type against the same text (`serde_json_fits_the_types`).
#![cfg(feature = "ts")]
use herdsman_server::api::*;
use herdsman_server::events::{fold, read, Event, IssueState};
use herdsman_server::{hosts, waiting::Waiting, Ctx};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/types/api.d.ts");

/// Every shape the board reads or posts, in file order.
macro_rules! each {
    ($m:ident) => {
        $m!(
            Herd,
            HerdFleet,
            HerdLane,
            HerdQueued,
            HerdHost,
            Chat,
            Notice,
            Moment,
            Frag,
            Patch,
            BacklogIssue,
            Send,
            Answer,
            Act,
            Setup,
            State,
            Fleet,
            Queued,
            Lane,
            Ask,
            Row,
            HostTab,
            HostCard,
            Cell,
            Bar,
            Disk,
            Leftovers,
            Account,
            Meter,
            Balance,
            FleetRow,
            Sub,
            Paired,
            Events,
            Ev,
            Reply,
            Event,
            IssueState,
            Waiting,
            hosts::Seen,
            hosts::Row,
            Value
        )
    };
}

/// The file `just types` writes: serde's numbers are JSON numbers, so u64 is `number`, not ts-rs's default `bigint`.
fn bindings() -> String {
    let cfg = ts_rs::Config::new().with_large_int("number");
    let mut out = "// Generated from the Rust payload types by `just types` (#375); never edit by hand.\n".to_string();
    macro_rules! decl {
        ($($t:ty),*) => {$(
            let docs = <$t as TS>::docs();
            out += &format!("\n{}export {}\n", docs.unwrap_or_default(), <$t as TS>::decl(&cfg));
        )*};
    }
    each!(decl);
    out
}

#[test]
fn export_bindings() {
    std::fs::create_dir_all(Path::new(OUT).parent().unwrap()).unwrap();
    std::fs::write(OUT, bindings()).unwrap();
}

/// The demo export (`demo/`), as `serve --export --demo` writes it.
fn demo() -> (PathBuf, Vec<Event>) {
    let demo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo"));
    let d = std::env::temp_dir().join(format!("herdsman-types-{}", std::process::id()));
    let ctx = Ctx {
        config: demo.join("fleet.toml"),
        state: demo.join("events.jsonl"),
        worktrees: d.join("wt"),
        hermes: d.join("hermes"),
        exe: "herdsman".into(),
        now: 1,
        home: d.join("home"),
        hints: vec![],
    };
    herdsman_server::serve::export(&ctx, &herdsman_server::runner::Real, &d.join("site"), true).unwrap();
    (d, read(&ctx.state).unwrap())
}

/// `v` read as `T` and written again is `v`: the type carries every field the server sends.
fn same<T: serde::de::DeserializeOwned + serde::Serialize>(v: &Value) -> Value {
    let t: T = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(&json!(t), v, "{} drops or changes a field", std::any::type_name::<T>());
    v.clone()
}

#[test]
fn serde_json_fits_the_types() {
    let (d, events) = demo();
    let site = d.join("site");
    let file = |p: &Path| serde_json::from_str::<Value>(&std::fs::read_to_string(p).unwrap()).unwrap();
    // (type, JSON): the demo's real payloads, then each type's default (nulls, empty lists).
    let mut checks = vec![("Herd", same::<Herd>(&file(&site.join("herd.json"))))];
    // #267: the Host tab's data, its cards' rows (gauges, sparklines, charts inline) included.
    checks.push(("HostTab", same::<HostTab>(&file(&site.join("api/v1/host")))));
    for l in std::fs::read_dir(site.join("lane")).unwrap() {
        checks.push(("Chat", same::<Chat>(&file(&l.unwrap().path()))));
    }
    checks.extend(events.iter().map(|e| ("Event", json!(e))));
    checks.push((
        "Event",
        json!(Event::new(1, "acme", None, "spawn", json!({"agent": "a"}))),
    ));
    checks.extend(fold(&events).values().map(|s| ("IssueState", json!(s))));
    let row = hosts::Row {
        load_cpu: Some(0.4),
        pressure: Some(1),
        procs: Some([1, 2, 3, 4, 5]),
        ..Default::default()
    };
    let wait = Waiting {
        fleet: "acme".into(),
        issue: 7,
        lane: "acme-impl-7".into(),
        who: "owner",
        what: "an ask".into(),
        since: 1,
        next_cmd: None,
    };
    let patch = Patch {
        id: "q".into(),
        n: 3,
        p: 1,
        set: [("2".into(), "<d>".into())].into(),
    };
    let moment = Moment {
        seq: 4,
        ts: 5,
        k: Some("merged".into()),
        f: "api".into(),
        n: Some(7),
    };
    checks.extend([
        ("HostRow", json!(row)),
        ("Waiting", json!(wait)),
        ("Patch", json!(patch)),
        ("Moment", json!(moment)),
    ]);
    // #365: a backlog row as `gh::backlog`'s jq writes it to the cache.
    let issue = json!({"number": 9, "title": "t", "state": "OPEN", "created": "2026-01-01T00:00:00Z",
        "updated": "2026-01-02T00:00:00Z", "labels": ["phone"], "ac": true, "pr": false});
    checks.push(("BacklogIssue", same::<BacklogIssue>(&issue)));
    macro_rules! defaults {
        ($($t:ident),*) => { $( checks.push((stringify!($t), json!($t::default()))); )* };
    }
    defaults!(
        Herd,
        HerdFleet,
        HerdLane,
        HerdQueued,
        HerdHost,
        Chat,
        Notice,
        Moment,
        Frag,
        Patch,
        BacklogIssue,
        Send,
        Answer,
        Act,
        Setup,
        State,
        Fleet,
        Queued,
        Lane,
        Ask,
        Row,
        HostTab,
        HostCard,
        Cell,
        Bar,
        Disk,
        Leftovers,
        Account,
        Meter,
        Balance,
        FleetRow,
        Sub,
        Paired,
        Events,
        Ev,
        Reply
    );
    checks.extend([
        ("HostSeen", json!(hosts::Seen::default())),
        ("HostRow", json!(hosts::Row::default())),
    ]);
    // An object literal per value: tsc rejects a missing field, a field the type lacks and a wrong type.
    let mut ts = "import type * as T from \"./api\";\n".to_string();
    for (i, (t, v)) in checks.iter().enumerate() {
        ts += &format!("export const v{i}: T.{t} = {v};\n");
    }
    std::fs::write(d.join("api.d.ts"), bindings()).unwrap();
    std::fs::write(d.join("check.ts"), ts).unwrap();
    let tsc = std::env::var("TSC").unwrap_or_else(|_| "tsc".into());
    let out = std::process::Command::new(&tsc)
        .args([
            "--noEmit",
            "--strict",
            "--target",
            "es2022",
            "--module",
            "es2022",
            "--moduleResolution",
            "bundler",
        ])
        .arg(d.join("check.ts"))
        .output()
        .unwrap_or_else(|e| panic!("{tsc}: {e} (install TypeScript, or set TSC)"));
    assert!(
        out.status.success(),
        "tsc:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(checks.len() > 40, "{} checks", checks.len());
}
