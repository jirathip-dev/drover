#![cfg(feature = "ts")]
use drover_server::api::*;
use drover_server::events::{fold, Event, IssueState};
use drover_server::{hosts, propose, waiting::Waiting, Ctx};
use serde_json::{json, Value};
use std::path::Path;
use ts_rs::TS;

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/types/api.d.ts");

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
            LaneChat,
            ApprovalChoice,
            Block,
            Moment,
            NeedsTab,
            NeedAsk,
            Blocked,
            Menu,
            NeedIssue,
            LanesTab,
            LaneSection,
            LaneRepo,
            LaneRow,
            LaneUnsaved,
            LaneNow,
            ShedRow,
            ChatTab,
            ChatRepo,
            Chip,
            ThreadRow,
            Link,
            ThreadView,
            QueuedMsg,
            ThreadPage,
            Msg,
            FileRef,
            Answered,
            Handoff,
            HandoffFrom,
            HandoffChoice,
            RepoThreads,
            RepoThread,
            LastLine,
            RunCard,
            BacklogIssue,
            Send,
            Answer,
            Act,
            Setup,
            State,
            LaneCounts,
            Fleet,
            Queued,
            PendingStart,
            Lane,
            Ask,
            Row,
            HostTab,
            HostCard,
            Cell,
            Bar,
            Disk,
            Leftovers,
            Balance,
            FleetRow,
            Sub,
            Paired,
            Notifier,
            Vendors,
            Vendor,
            QueueTab,
            QueueCard,
            Marks,
            Drive,
            DriveAct,
            QueueRow,
            Stage,
            PrLine,
            QueueLane,
            DoneRow,
            Backlog,
            ConfigSheet,
            Harness,
            RoleRow,
            Conductor,
            Events,
            Ev,
            IssueView,
            WhyNote,
            Verdict,
            Repro,
            Lessons,
            LessonFleet,
            LessonRun,
            Digest,
            SetupScreen,
            Roles,
            ChatDefaults,
            Reply,
            Job,
            ConductorAttach,
            AttachEnv,
            propose::ConfigView,
            propose::ConfigField,
            propose::ConfigChange,
            propose::ConfigSet,
            propose::NewFleet,
            propose::ConfigCheck,
            Event,
            IssueState,
            Waiting,
            hosts::Seen,
            drover_server::roots::Root,
            hosts::Row,
            Value
        )
    };
}

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

struct Canned(std::collections::BTreeMap<String, String>);

impl drover_server::runner::Runner for Canned {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover_server::runner::Out> {
        let line = format!("{prog} {}", args.join(" "));
        let out = self
            .0
            .iter()
            .filter(|(k, _)| line.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len());
        Ok(drover_server::runner::Out {
            code: i32::from(out.is_none()),
            stdout: out.map_or(String::new(), |o| o.1.clone()),
            stderr: String::new(),
        })
    }
}

fn demo() -> (Ctx, Vec<Event>) {
    let d = std::env::temp_dir().join(format!("drover-types-{}", std::process::id()));
    let demo = d.join("demo");
    std::fs::create_dir_all(&demo).unwrap();
    for f in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo")).unwrap() {
        let f = f.unwrap();
        if f.file_type().unwrap().is_file() {
            std::fs::copy(f.path(), demo.join(f.file_name())).unwrap();
        }
    }
    let log = std::fs::read_to_string(demo.join("events.jsonl")).unwrap();
    let events: Vec<Event> = log.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ctx = Ctx {
        config: demo.join("fleet.toml"),
        state: demo.join("events.jsonl"),
        worktrees: d.join("wt"),
        homes: [("hermes", d.join("hermes"))].into(),
        exe: "drover".into(),
        now: events.iter().map(|e| e.ts).max().unwrap_or(0) + 60,
        home: d.join("home"),
        hints: vec![],
    };
    (ctx, events)
}

fn same<T: serde::de::DeserializeOwned + serde::Serialize>(v: &Value) -> Value {
    let t: T = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(&json!(t), v, "{} drops or changes a field", std::any::type_name::<T>());
    v.clone()
}

#[test]
fn serde_json_fits_the_types() {
    let (ctx, events) = demo();
    let d = ctx.home.parent().unwrap().to_path_buf();
    let _scratch = drover_server::db::Scratch::start();
    let canned = Canned(
        serde_json::from_str(&std::fs::read_to_string(ctx.config.with_file_name("runner.json")).unwrap()).unwrap(),
    );
    let s = drover_server::serve::Server::new(&ctx, &canned, None);
    let get = |p: &str| {
        let res = String::from_utf8(s.respond(&format!("GET /api/v1/{p} HTTP/1.1\r\n\r\n"), ctx.now)).unwrap();
        assert!(res.starts_with("HTTP/1.1 200"), "{p}: {res}");
        serde_json::from_str::<Value>(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    };
    let mut checks = vec![("Herd", same::<Herd>(&get("herd")))];
    checks.push(("HostTab", same::<HostTab>(&get("host"))));
    checks.push(("QueueTab", same::<QueueTab>(&get("queue"))));
    checks.push(("NeedsTab", same::<NeedsTab>(&get("needs"))));
    checks.push(("LanesTab", same::<LanesTab>(&get("lanes"))));
    checks.push(("ChatTab", same::<ChatTab>(&get("chat"))));
    let state = same::<State>(&get("state"));
    for l in state["lanes"].as_array().unwrap() {
        checks.push((
            "Chat",
            same::<Chat>(&get(&format!("lane/{}", l["name"].as_str().unwrap()))),
        ));
    }
    checks.push(("State", state));
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
        promise: None,
    };
    let moment = Moment {
        seq: 4,
        ts: 5,
        k: Some("merged".into()),
        f: "api".into(),
        n: Some(7),
    };
    let marks = Marks {
        verify: Some("on".into()),
        repro: true,
        serial: vec!["db".into()],
        note: Some("verify(1)".into()),
        ..Marks::default()
    };
    let verdict = Verdict {
        role: "verify".into(),
        shots: vec!["shots/home-390.png".into()],
        repro: Some(Repro {
            kind: "run".into(),
            text: "```sh\ncargo test\n```".into(),
        }),
        ..Verdict::default()
    };
    let last = Some(LessonRun {
        issue: 30,
        ts: 1,
        state: "impl".into(),
    });
    let lessons = Lessons {
        fleets: vec![LessonFleet {
            last,
            next: Some(2),
            ..LessonFleet::default()
        }],
    };
    checks.extend([
        ("Marks", json!(marks)),
        (
            "IssueView",
            json!(IssueView {
                verdicts: vec![verdict],
                why: Some(WhyNote::default()),
                ..IssueView::default()
            }),
        ),
        ("Lessons", json!(lessons)),
        ("HostRow", json!(row)),
        ("Waiting", json!(wait)),
        ("Moment", json!(moment)),
    ]);
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
        NeedsTab,
        NeedAsk,
        Blocked,
        Menu,
        NeedIssue,
        LanesTab,
        LaneSection,
        LaneRepo,
        LaneRow,
        LaneUnsaved,
        LaneNow,
        ChatTab,
        ChatRepo,
        Chip,
        ThreadRow,
        Link,
        ThreadView,
        QueuedMsg,
        ThreadPage,
        Msg,
        FileRef,
        Answered,
        Handoff,
        HandoffFrom,
        HandoffChoice,
        RepoThreads,
        RepoThread,
        LastLine,
        RunCard,
        BacklogIssue,
        Send,
        Answer,
        Act,
        Setup,
        State,
        LaneCounts,
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
        Balance,
        FleetRow,
        Sub,
        Paired,
        Notifier,
        Vendors,
        Vendor,
        QueueTab,
        QueueCard,
        Marks,
        Drive,
        DriveAct,
        QueueRow,
        Stage,
        PrLine,
        QueueLane,
        DoneRow,
        Backlog,
        ConfigSheet,
        Harness,
        RoleRow,
        Conductor,
        Events,
        Ev,
        IssueView,
        WhyNote,
        Verdict,
        Repro,
        Lessons,
        LessonFleet,
        LessonRun,
        Digest,
        Reply
    );
    checks.extend([
        ("HostSeen", json!(hosts::Seen::default())),
        ("HostRow", json!(hosts::Row::default())),
    ]);
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
