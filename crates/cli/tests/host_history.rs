//! #234: the Host tab's 24 h history: a row per host (the fast step for this host, the 5-minute survey for a remote
//! one), rows past 24 h dropped, the size bound, worst-per-bucket downsampling, and the cards and chart series served.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::hosts::{history, record, spark, survey, Row, BOUND};
use herdsman::serve::Server;
use serde_json::json;

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m bazzite -- ";

fn two_hosts(name: &str) -> herdsman::Ctx {
    let cfg = format!("{CONFIG}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    c
}

fn fake() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine bazzite agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", "{ 6.0 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            "df -k /",
            "Filesystem 1024-blocks Used Available\n/dev/disk3 488245288 400000000 44040192\n",
        )
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "2.0 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'df' '-k' '/'"),
            "Filesystem 1024-blocks Used Available\n/dev/sda 1048576 0 524288\n",
        )
}

fn row(ts: u64, host: &str, load: f64) -> Row {
    Row {
        ts,
        host: host.into(),
        load_cpu: Some(load),
        pressure: Some(0),
        swap_out: Some(1834.0),
        mem_free_gb: Some(9.4),
        disk_free_gb: Some(123.4),
        lanes: 3,
    }
}

#[test]
fn rows_are_appended_per_host_per_step() {
    let c = two_hosts("history-append");
    let (r, cfg) = (fake(), Config::load(&c.config).unwrap());
    // The fast step: this host, sampled now.
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    // The 5-minute survey: the remote host, from the same probe placement uses.
    survey(&c, &cfg, &r, false);
    let rows = history(&c);
    let hosts: Vec<(&str, Option<f64>)> = rows.iter().map(|r| (r.host.as_str(), r.load_cpu)).collect();
    assert_eq!(hosts, [("mini", Some(1.5)), ("bazzite", Some(0.5))], "{rows:?}");
    let disk: Vec<Option<f64>> = rows.iter().map(|r| r.disk_free_gb).collect();
    assert_eq!(disk, [Some(42.0), Some(0.5)]);
    assert!(rows.iter().all(|r| r.ts == NOW), "{rows:?}");
}

#[test]
fn rows_older_than_24h_are_dropped() {
    let mut c = two_hosts("history-drop");
    for (ts, h) in [
        (NOW - 100_000, "mini"),
        (NOW - 100_000, "bazzite"),
        (NOW - 3_600, "mini"),
    ] {
        c.now = ts;
        record(&c, &[row(ts, h, 1.0)], 2).unwrap();
    }
    c.now = NOW;
    record(&c, &[row(NOW, "mini", 1.0)], 2).unwrap();
    let ts: Vec<u64> = history(&c).iter().map(|r| r.ts).collect();
    assert_eq!(ts, [NOW - 3_600, NOW]);
}

#[test]
fn the_file_stays_under_the_size_bound() {
    let mut c = two_hosts("history-bound");
    for i in 0..1_500 {
        c.now = NOW + i * 60;
        record(&c, &[row(c.now, "mini", 1.23), row(c.now, "bazzite", 0.45)], 2).unwrap();
    }
    let p = c.state.with_file_name("host-history.jsonl");
    let size = std::fs::metadata(&p).unwrap().len() as usize;
    assert!(size <= 2 * BOUND, "{size} B over {} B", 2 * BOUND);
    let rows = history(&c);
    assert!(
        rows.len() >= 2 * 1_300 && rows.iter().all(|r| r.ts + 90_000 > c.now),
        "{} rows",
        rows.len()
    );
    assert_eq!(rows.last().unwrap().ts, c.now, "the newest rows are kept");
}

#[test]
fn one_host_keeps_24h_and_rewrites_rarely() {
    use std::os::unix::fs::MetadataExt;
    let mut c = two_hosts("history-one");
    let p = c.state.with_file_name("host-history.jsonl");
    let (host, mut ino, mut rewrites) = ("a-rather-long-hostname-mini-01", 0, 0);
    for i in 0..1_500 {
        c.now = NOW + i * 60;
        record(&c, &[row(c.now, host, 12.34)], 0).unwrap();
        let now = std::fs::metadata(&p).unwrap().ino();
        rewrites += usize::from(ino != 0 && now != ino);
        ino = now;
    }
    let size = std::fs::metadata(&p).unwrap().len() as usize;
    assert!(size <= BOUND, "{size} B over {BOUND} B");
    assert!(rewrites <= 1, "{rewrites} rewrites");
    let rows = history(&c);
    assert!(rows.len() >= 1_440 && rows.len() <= 1_500, "{} rows", rows.len());
    assert!(rows.first().unwrap().ts + 86_400 <= c.now + 60, "24 h kept");
}

#[test]
fn downsampling_keeps_the_worst_per_bucket() {
    let rows: Vec<Row> = (0..60u64)
        .map(|i| {
            let mut r = row(NOW - 3_600 + 60 + i * 60, "mini", if i == 17 { 3.71 } else { 0.5 });
            r.disk_free_gb = Some(if i == 40 { 9.0 } else { 50.0 });
            r
        })
        .collect();
    let load = spark(&rows, "mini", "load", (NOW, 3_600, 6));
    assert_eq!(
        load[1],
        Some((3.71, NOW - 3_600 + 60 + 17 * 60)),
        "the peak and its own minute"
    );
    assert!(load.iter().enumerate().all(|(i, v)| i == 1 || v.unwrap().0 == 0.5));
    let disk = spark(&rows, "mini", "disk", (NOW, 3_600, 6));
    assert_eq!(disk[4].unwrap().0, 9.0, "free space keeps the minimum");
    assert_eq!(spark(&rows, "bazzite", "load", (NOW, 3_600, 6)), vec![None; 6]);
}

#[test]
fn the_host_tab_has_a_card_per_host_and_serves_the_chart() {
    let c = two_hosts("history-cards");
    let st = c.state.parent().unwrap();
    let seen = json!({"bazzite": {"tags": ["os:linux"], "seen": NOW - 1980, "up": false, "load": 0.4, "disk": 300.0, "disk_total": 476.0}});
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    record(&c, &[row(NOW - 30, "mini", 3.1), row(NOW - 30, "bazzite", 0.4)], 2).unwrap();
    let r = fake();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |q: &str| {
        String::from_utf8_lossy(&s.respond(&format!("GET {q} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned()
    };
    let page = get("/?all=1");
    let gs: Vec<&str> = page
        .split("<span class=gn>")
        .skip(1)
        .map(|g| g.split('<').next().unwrap())
        .collect();
    assert_eq!(gs[..2], ["HOSTS", "FLEET DISK"], "{gs:?}");
    assert!(!page.contains(">MACHINE<"));
    let (mini, baz) = (
        page.find("data-h=\"mini\"").unwrap(),
        page.find("data-h=\"bazzite\"").unwrap(),
    );
    assert!(mini < baz, "main first");
    assert!(page[mini..baz].contains("<span class=\"tag m\">main</span>"));
    for k in ["load", "swap", "mem", "disk", "lanes"] {
        assert_eq!(page.matches(&format!("data-k={k} ")).count(), 2, "{k} on both cards");
    }
    // Load 1.5 is under 2.5, but the history keeps the 3.1 peak in the last 12-minute bucket.
    let s24 = &page[page.find("data-k=load data-s=\"").unwrap() + 20..];
    assert!(
        s24.starts_with(&",".repeat(119)) && s24[119..].starts_with("3.10\""),
        "{}",
        &s24[..140]
    );
    let i = page.find("<span class=k>disk</span>").unwrap();
    assert!(
        page[i..].contains("<b class=g>free 42 GB</b> <span class=mt>of 466</span>"),
        "used-space bar words: {}",
        &page[i..i + 400]
    );
    assert!(page[baz..].contains("<span class=\"hw m\">offline · last seen 33m</span>"));
    let chart = get("/frag?id=hist&host=mini&k=load");
    let parts: Vec<&str> = chart.split("\r\n\r\n").nth(1).unwrap().split('|').collect();
    assert_eq!(parts.len(), 4, "value, lanes, pressure, minutes ago: {chart}");
    assert!(
        parts[0].split(';').nth(2).unwrap().ends_with("3.10") && parts[3].ends_with('0'),
        "{chart}"
    );
}
