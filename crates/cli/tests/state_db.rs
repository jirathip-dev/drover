mod common;
use common::*;
use drover::db;
use drover::events::{fold, history, jsonl, read, Event};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

fn rows(evs: &[Event]) -> String {
    evs.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect()
}

fn gzip(p: &Path, text: &str) {
    let f = std::fs::File::create(p).unwrap();
    let mut c = std::process::Command::new("gzip")
        .arg("-c")
        .stdin(std::process::Stdio::piped())
        .stdout(f)
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success());
}

fn legacy(d: &Path) -> Vec<Event> {
    let e = |seq, ts, issue, kind: &str, d: Value| Event {
        seq,
        ..Event::new(ts, "app", issue, kind, d)
    };
    let all = vec![
        e(1, 10, Some(1), "spawn", json!({"role": "impl", "agent": "app-impl-1"})),
        e(2, 20, Some(1), "verdict", json!({"sha": "a", "verdict": "PASS"})),
        e(3, 30, None, "drive-pass", json!({})),
        e(4, 40, Some(1), "merged", json!({"pr": "9"})),
        e(5, 50, None, "queue", json!({"op": "add", "issues": [2]})),
        e(6, 60, Some(2), "spawn", json!({"role": "impl", "agent": "app-impl-2"})),
        e(7, 70, Some(2), "verdict", json!({"sha": "b", "verdict": "FAIL"})),
    ];
    std::fs::create_dir_all(d.join("serve")).unwrap();
    std::fs::create_dir_all(d.join("webpush")).unwrap();
    gzip(&d.join("events-197001.jsonl.gz"), &rows(&all[..3]));
    std::fs::write(d.join("events.jsonl"), rows(&all[3..])).unwrap();
    std::fs::write(d.join("serve/devices.json"), r#"[{"hash":"h1","label":"phone"}]"#).unwrap();
    let sub = r#"[{"endpoint":"https://push.example/1","p256dh":"k","auth":"a","label":"phone","created":1}]"#;
    std::fs::write(d.join("webpush/subs.json"), sub).unwrap();
    all
}

fn states(evs: &[Event]) -> Value {
    serde_json::to_value(fold(evs).into_values().collect::<Vec<_>>()).unwrap()
}

#[test]
fn migration_imports_once_with_an_equal_fold_and_the_export_is_the_old_format() {
    let d = tmpdir("db-migrate");
    let c = ctx(&d, CONFIG);
    let st = c.state.parent().unwrap().to_path_buf();
    let all = legacy(&st);
    let (code, out) = run(&c, &Fake::default(), &["migrate"]);
    assert_eq!(code, 0, "{out}");
    let back = history(&c.state).unwrap();
    assert_eq!(states(&back), states(&all), "fold before == fold after");
    assert_eq!(rows(&back), rows(&all), "every event once, seq kept");
    assert_eq!(
        read(&c.state).unwrap().iter().map(|e| e.seq).collect::<Vec<_>>(),
        [4, 5, 6, 7]
    );
    for f in [
        "events.jsonl",
        "events-197001.jsonl.gz",
        "serve/devices.json",
        "webpush/subs.json",
    ] {
        assert!(!st.join(f).exists() && st.join(format!("{f}.migrated")).exists(), "{f}");
    }
    assert_eq!(drover::devices::load(&c)[0].label, "phone");
    assert_eq!(drover::webpush::load(&c.state).len(), 1);
    assert_eq!(run(&c, &Fake::default(), &["migrate"]).0, 0);
    assert_eq!(history(&c.state).unwrap().len(), all.len());
    assert_eq!(drover::devices::load(&c).len(), 1);
    assert_eq!(jsonl(&c.state).unwrap(), rows(&all));
    let (code, out) = run(&c, &Fake::default(), &["events", "--jsonl"]);
    assert_eq!((code, out), (0, rows(&all)));
}

#[test]
fn a_log_written_after_the_import_is_imported_after_it() {
    let d = tmpdir("db-late");
    let p = d.join("events.jsonl");
    drover::events::append(&p, &Event::new(1, "app", Some(1), "spawn", json!({"role": "impl"}))).unwrap();
    std::fs::write(&p, rows(&[Event::new(2, "app", Some(1), "merged", json!({}))])).unwrap();
    let all = read(&p).unwrap();
    assert_eq!(all.iter().map(|e| (e.seq, e.ts)).collect::<Vec<_>>(), [(1, 1), (2, 2)]);
    assert!(d.join("events.jsonl.migrated").exists() && !p.exists());
}

#[test]
fn a_lease_is_held_until_it_expires_then_taken_over_with_no_stale_row_left() {
    let d = tmpdir("db-lease");
    let p = d.join("events.jsonl");
    let live = |_: &db::Lease| false;
    let a = db::take(&p, "drive.lock", 1000, 60, "a", &live).unwrap();
    assert!(
        db::take(&p, "drive.lock", 1059, 60, "b", &live).is_none(),
        "held until it expires"
    );
    let b = db::take(&p, "drive.lock", 1060, 60, "b", &live).unwrap();
    let rows = db::leases(&p, "drive");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].data.as_str(), rows[0].at), ("b", 1060));
    assert!(!db::renew(
        &p,
        &a,
        &db::Lease {
            expires_at: 9999,
            ..a.clone()
        }
    ));
    assert!(!db::release(&p, "drive.lock", Some(&a)));
    assert!(db::release(&p, "drive.lock", Some(&b)));
    assert!(db::leases(&p, "drive").is_empty());
    db::take(&p, "kick-app-1.lock", 2000, 300, "", &live).unwrap();
    assert!(db::take(&p, "kick-app-1.lock", 2001, 300, "", &|_| true).is_some());
}

#[test]
fn the_database_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmpdir("db-mode");
    drover::events::append(&d.join("events.jsonl"), &Event::new(1, "app", None, "x", json!({}))).unwrap();
    let mode = std::fs::metadata(d.join("state.db")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
