mod common;
use common::*;
use drover::config::Config;
use drover::hosts::{history, record, spark, survey, Row, BOUND};
use drover::serve::Server;
use serde_json::json;

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

fn two_hosts(name: &str) -> (drover::Ctx, FakeShed) {
    let cfg = format!("{CONFIG}\n[hosts.mini]\ntags = [\"fast\"]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn fake() -> Fake {
    Fake::default()
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
        procs: None,
    }
}

#[test]
fn rows_are_appended_per_host_per_step() {
    let (c, sh) = two_hosts("history-append");
    let (r, cfg) = (fake(), Config::load(&c.config).unwrap());
    let bazzite = FakeShed::new();
    let over = OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    };
    let (code, out) = run(&c, &over, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    survey(&c, &cfg, &sh.on(&over), false);
    let rows = history(&c);
    let hosts: Vec<(&str, Option<f64>)> = rows.iter().map(|r| (r.host.as_str(), r.load_cpu)).collect();
    assert_eq!(hosts, [("mini", Some(1.5)), ("bazzite", Some(0.5))], "{rows:?}");
    let disk: Vec<Option<f64>> = rows.iter().map(|r| r.disk_free_gb).collect();
    assert_eq!(disk, [Some(42.0), Some(0.5)]);
    assert!(rows.iter().all(|r| r.ts == NOW), "{rows:?}");
}

#[test]
fn rows_older_than_24h_are_dropped() {
    let (mut c, _sh) = two_hosts("history-drop");
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
    let (mut c, _sh) = two_hosts("history-bound");
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
    let (mut c, _sh) = two_hosts("history-one");
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
    let (c, sh) = two_hosts("history-cards");
    let st = c.state.parent().unwrap();
    let seen = json!({"bazzite": {"tags": ["os:linux"], "seen": NOW - 1980, "up": false, "load": 0.4, "disk": 300.0, "disk_total": 476.0}});
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    record(&c, &[row(NOW - 30, "mini", 3.1), row(NOW - 30, "bazzite", 0.4)], 2).unwrap();
    let r = fake();
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let get = |q: &str| {
        String::from_utf8_lossy(&s.respond(&format!("GET {q} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW)).into_owned()
    };
    let page = host_tab(&s, ADDR, NOW);
    let gs: Vec<&str> = page
        .split("<span class=gn>")
        .skip(1)
        .map(|g| g.split('<').next().unwrap())
        .collect();
    assert_eq!(gs[..2], ["HOSTS", "FLEET DISK"], "{gs:?}");
    assert!(!page.contains(">MACHINE<"));
    let (mini, bazzite) = (
        page.find("data-h=\"mini\"").unwrap(),
        page.find("data-h=\"bazzite\"").unwrap(),
    );
    assert!(mini < bazzite, "main first");
    assert!(page[mini..bazzite].contains("<span class=\"tag m\">main</span>"));
    for k in ["load", "swap", "mem", "disk", "lanes"] {
        assert_eq!(page.matches(&format!("data-k={k} ")).count(), 2, "{k} on both cards");
    }
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
    assert!(page[bazzite..].contains("<span class=\"hw m\">offline · last seen 33m</span>"));
    let chart = get("/frag?id=hist&host=mini&k=load");
    let parts: Vec<&str> = chart.split("\r\n\r\n").nth(1).unwrap().split('|').collect();
    assert_eq!(parts.len(), 4, "value, lanes, pressure, minutes ago: {chart}");
    assert!(
        parts[0].split(';').nth(2).unwrap().ends_with("3.10") && parts[3].ends_with('0'),
        "{chart}"
    );
}

fn watched_fake(len: usize) -> Fake {
    let ps = format!(
        "204800 /usr/local/bin/claude --settings {}\n102400 /opt/homebrew/bin/qemu-system-aarch64\n51200 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome\n10240 drover drive --fast\n20480 /bin/zsh\n",
        "x".repeat(len)
    );
    fake().reply("ps -axo rss=,command=", &ps)
}

#[test]
fn no_process_table_walk_while_nobody_watches() {
    let (c, _sh) = two_hosts("history-idle");
    let r = watched_fake(10);
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("ps -axo rss=,command="), 0, "{:?}", r.calls.borrow());
    let rows = history(&c);
    assert!(rows.len() == 1 && rows[0].procs.is_none(), "{rows:?}");
    std::fs::write(c.state.with_file_name("host-watch.at"), (NOW - 180).to_string()).unwrap();
    run(&c, &r, &["drive", "--fast"]);
    assert_eq!(r.count("ps -axo rss=,command="), 0);
}

#[test]
fn sampling_resumes_once_the_host_tab_is_open() {
    let (mut c, _sh) = two_hosts("history-watch");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Fake::default(), l, false));
    let live = |q: &str| {
        use std::io::Write;
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "GET /live{q} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        s
    };
    let watch = c.state.with_file_name("host-watch.at");
    let _queue = live("?tab=queue");
    assert!(!watch.exists(), "another tab is not watching the host");
    let _host = live("?tab=host");
    let at: u64 = std::fs::read_to_string(&watch).unwrap().parse().unwrap();
    c.now = at + 60;
    let r = watched_fake(10);
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("ps -axo rss=,command="), 1);
    assert_eq!(history(&c)[0].procs, Some([200, 100, 50, 10, 20]));
}

#[test]
fn the_byte_bound_holds_with_long_command_lines() {
    let (mut c, _sh) = two_hosts("history-long");
    let (r, cfg) = (watched_fake(64 * 1024), Config::load(&c.config).unwrap());
    for i in 0..1_700 {
        c.now = NOW + i * 60;
        std::fs::write(c.state.with_file_name("host-watch.at"), c.now.to_string()).unwrap();
        drover::hosts::sample(&c, &cfg, &r, &[]).unwrap();
    }
    let size = std::fs::metadata(c.state.with_file_name("host-history.jsonl"))
        .unwrap()
        .len() as usize;
    assert!(size <= 2 * BOUND, "{size} B over {} B", 2 * BOUND);
    let rows = history(&c);
    assert!(rows.len() <= 2 * 1_600 && rows.len() >= 1_440, "{} rows", rows.len());
    assert!(rows.iter().all(|r| r.procs == Some([200, 100, 50, 10, 20])));
}
