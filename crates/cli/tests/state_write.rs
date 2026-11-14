mod common;
use common::*;
use drover::devices;
use drover::events::read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn break_writes(c: &drover::Ctx) -> impl Fn() {
    let file = drover::db::file(&c.state);
    let db = move || rusqlite::Connection::open(&file).unwrap();
    let t = "CREATE TRIGGER full BEFORE DELETE ON devices BEGIN SELECT RAISE(ABORT, 'disk full'); END";
    db().execute(t, []).unwrap();
    move || assert_eq!(db().execute("DROP TRIGGER full", []).unwrap(), 0)
}

#[test]
fn a_failed_write_keeps_the_old_devices_and_logs_once_an_hour() {
    let mut c = ctx(&tmpdir("state-write-fail"), CONFIG);
    devices::request(&c, "iPhone", "10.0.0.2").unwrap().unwrap();
    let file = drover::db::file(&c.state);
    let before = devices::load(&c);
    let fix = break_writes(&c);
    assert!(devices::store(&c, &[]).is_err());
    c.now += 60;
    assert!(devices::store(&c, &[]).is_err());
    fix();
    assert_eq!(
        devices::load(&c),
        before,
        "the previous devices survive: the store rolled back"
    );
    let failed: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "state-write-failed")
        .collect();
    assert_eq!(failed.len(), 1, "one event an hour");
    assert_eq!(failed[0].str("path"), Some(file.to_str().unwrap()));
    assert_eq!(failed[0].str("error"), Some("other error"), "the error kind only");
    let hash = &devices::load(&c)[0].hash;
    assert!(
        !drover::events::jsonl(&c.state).unwrap().contains(hash.as_str()),
        "no token hash in the log"
    );
    assert!(drover::events::plain(&failed[0]).starts_with(&format!("drover state-write-failed: {}: ", file.display())));
    devices::store(&c, &[]).unwrap();
    assert_eq!(devices::load(&c), [] as [drover::devices::Device; 0]);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn an_existing_vapid_key_is_never_rewritten() {
    let c = ctx(&tmpdir("state-write-vapid"), CONFIG);
    let k = drover::webpush::vapid(&c.state).unwrap();
    let file = c.state.parent().unwrap().join("webpush/vapid.key");
    let before = std::fs::read(&file).unwrap();
    chmod(file.parent().unwrap(), 0o500);
    let again = drover::webpush::vapid(&c.state);
    chmod(file.parent().unwrap(), 0o700);
    assert_eq!(again.unwrap().to_bytes(), k.to_bytes());
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn last_seen_is_written_at_most_hourly() {
    let mut c = ctx(&tmpdir("state-write-seen"), CONFIG);
    let (code, token) = devices::request(&c, "iPhone", "10.0.0.2").unwrap().unwrap();
    devices::approve(&c, &code).unwrap();
    let cookie = format!("hd={token}");
    let used = |c: &drover::Ctx| devices::load(c)[0].used;
    let t0 = c.now;
    c.now = t0 + 3599;
    assert!(devices::find(&c, &cookie).is_some());
    assert_eq!(used(&c), t0, "not rewritten within the hour");
    c.now = t0 + 3600;
    assert!(devices::find(&c, &cookie).is_some());
    assert_eq!(used(&c), t0 + 3600);
    let fix = break_writes(&c);
    c.now = t0 + 7200;
    let found = devices::find(&c, &cookie);
    fix();
    assert!(found.is_some());
    assert_eq!(used(&c), t0 + 3600);
}

#[test]
fn concurrent_device_updates_all_land_without_a_write_failure() {
    let c = ctx(&tmpdir("state-write-race"), CONFIG);
    std::thread::scope(|s| {
        for _ in 0..16 {
            s.spawn(|| (0..20).for_each(|_| devices::store(&c, &[]).unwrap()));
        }
    });
    std::thread::scope(|s| {
        for i in 0..16 {
            let c = &c;
            s.spawn(move || {
                let _g = devices::lock();
                let mut all = devices::load(c);
                all.push(devices::Device {
                    hash: format!("h{i}"),
                    ..Default::default()
                });
                devices::store(c, &all).unwrap();
            });
        }
    });
    let mut hashes: Vec<_> = devices::load(&c).into_iter().map(|d| d.hash).collect();
    hashes.sort();
    let mut want: Vec<_> = (0..16).map(|i| format!("h{i}")).collect();
    want.sort();
    assert_eq!(hashes, want, "all 16 updates present");
    let log = read(&c.state).unwrap_or_default();
    let failed = log.iter().filter(|e| e.kind == "state-write-failed");
    assert_eq!(failed.count(), 0);
}
