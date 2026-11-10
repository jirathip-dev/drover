//! #618: a thread's id after its title (the bar, the rows); each whole-word `t<n>` naming a thread in a message, a
//! handoff row, a system line or an ask is a ref that opens it in place (back returns, scrolled where it was); an
//! archived thread opens read-only with [restore]. API v1 gives each thread page its `id` and `archived`.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// Threads t1…t27 on sendmeter (t27 archived); in t1 `extra` orchestrator lines, then a message naming t27, t999 (no
/// such thread) and `t27` in inline code, a handoff to t27, a system line and an ask whose option names t27.
fn setup(name: &str, extra: u64) -> herdsman::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let r = Fake::default();
    for i in 1..=27 {
        let title = format!("Thread {i}");
        assert_eq!(
            run(&c, &r, &["thread", "new", "sendmeter", &title]).1,
            format!("t{i}\n")
        );
    }
    let ev = |k: &str, d: Value| append(&c.state, &Event::new(T, "sendmeter", None, k, d)).unwrap();
    for i in 1..=extra {
        ev(
            "thread-msg",
            json!({"thread": "t1", "from": "orchestrator", "text": format!("m{i}")}),
        );
    }
    ev(
        "thread-msg",
        json!({"thread": "t1", "from": "orchestrator", "text": "see t27, t999 and `t27`"}),
    );
    ev(
        "thread-handoff",
        json!({"thread": "t1", "title": "Grill", "target": "t27"}),
    );
    ev(
        "thread-msg",
        json!({"thread": "t1", "from": "system", "text": "t27 is asleep"}),
    );
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Go on in t27?",
        "--option",
        "move to t27",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &r, &ask).1, "q1\n");
    for i in 1..=extra {
        ev(
            "thread-msg",
            json!({"thread": "t1", "from": "orchestrator", "text": format!("n{i}")}),
        );
    }
    assert_eq!(run(&c, &r, &["thread", "archive", "t27"]).0, 0);
    c
}

#[test]
fn a_known_thread_id_is_a_ref_and_an_unknown_or_coded_one_stays_text() {
    let c = setup("thread-refs", 0);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let [_, _, chat] = parts(&s, ADDR, &cookie, T + 5);
    let tl = "<span class=tl data-go=\"t27\">t27</span>";
    for want in [
        // A message: t27 a ref, t999 (no such thread) and the inline code one plain.
        format!("<p>see {tl}, t999 and <code>t27</code></p>"),
        // The handoff row, the system line, the ask's question and option.
        format!("class=qs>· Grill → {tl}</p>"),
        format!("class=\"qs note\">{tl} is asleep</p>"),
        format!("<p>Go on in {tl}?</p>"),
        format!("<span>1. move to {tl}</span>"),
        // The bar and the rows: the id after the title.
        "<div class=tn><b>Thread 1</b><small class=tid>· t1</small></div>".into(),
        "<span class=tt><span>Thread 1</span><small class=tid>· t1</small></span>".into(),
        // The archived thread: read-only, [restore], its composer off.
        "<p class=tlv>archived · read-only <button type=button class=rst data-t=\"t27\">restore</button></p>".into(),
    ] {
        assert!(chat.contains(&want), "{want}\n{chat}");
    }
    assert!(!chat.contains("data-go=\"t999\""), "{chat}");
    let view = &chat[chat.find("<div class=cvp data-t=\"t27\">").unwrap()..];
    assert!(
        view.contains("<textarea name=text rows=1 aria-label=message placeholder=\"message the conductor\" disabled>"),
        "{view}"
    );
    let one =
        &chat[chat.find("<div class=cvp data-t=\"t1\">").unwrap()..chat.find("<div class=cvp data-t=\"t2\">").unwrap()];
    assert!(
        !one.contains("class=rst") && !one.contains("conductor\" disabled>"),
        "{one}"
    );

    // API v1: each thread page's id and archived flag, in `chat` and `thread/<t>`.
    let tab: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 5, "chat")).unwrap();
    let pages: Vec<(String, bool)> = tab["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["page"]["id"].as_str().unwrap().into(), t["page"]["archived"] == true))
        .collect();
    assert_eq!(pages.len(), 27);
    assert!(pages.iter().all(|(id, a)| *a == (id == "t27")), "{pages:?}");
    let p: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 5, "thread/t27?before=99999")).unwrap();
    assert_eq!(
        (p["id"].as_str(), p["archived"].as_bool()),
        (Some("t27"), Some(true)),
        "{p}"
    );
}

/// In WebKit at 390: the bar shows `· t1`; a tap on t1's ref opens archived t27 in place, read-only with [restore];
/// the bar's back returns to t1 scrolled where it was, the browser's back likewise; [restore] turns t27's composer on.
#[test]
fn a_tapped_ref_opens_that_thread_and_back_returns_scrolled_where_you_were() {
    if !(cfg!(target_os = "macos") && std::process::Command::new("swift").arg("--version").output().is_ok()) {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let c = setup("thread-refs-webkit", 20);
    let cookie = pair(
        &Server::new(&c, &Fake::default(), Some(ADDR.into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || herdsman::serve::serve_on(&c, &Fake::default().reply("herdr", ""), l, false));
    let js = "const v = t => $(`#chat .cvp[data-t=\"${t}\"]`), vis = t => !!v(t) && v(t).getBoundingClientRect().height > 0; \
        const until = f => new Promise(r => { const t0 = Date.now(); (function go() { f() || Date.now() - t0 > 3000 ? r() : setTimeout(go, 50); })(); }); \
        const tap = e => e.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })); \
        (async () => { const at = window.late = []; await until(() => v('t1') && v('t1').querySelector('.tl')); chatOpen('t1'); \
          await new Promise(r => setTimeout(r, 300)); \
          at.push(+(v('t1').querySelector('.th .tn').textContent === 'Thread 1· t1')); \
          const tc = v('t1').querySelector('.tc'), ref = tc.querySelector('.md .tl'); ref.scrollIntoView({ block: 'center' }); \
          await new Promise(r => setTimeout(r, 200)); const y = tc.scrollTop; at.push(+(y > 0 && y + tc.clientHeight < tc.scrollHeight - 10)); \
          tap(ref); await until(() => vis('t27')); \
          at.push(+(THR === 't27'), +vis('t27'), +!vis('t1'), +!!v('t27').querySelector('.tlv .rst'), +v('t27').querySelector('.tw textarea').disabled); \
          tap(v('t27').querySelector('.bk')); await until(() => vis('t1')); await new Promise(r => setTimeout(r, 300)); \
          at.push(+(THR === 't1'), Math.abs(tc.scrollTop - y)); \
          tap(tc.querySelector('.md .tl')); await until(() => vis('t27')); history.back(); await until(() => vis('t1')); \
          await new Promise(r => setTimeout(r, 300)); at.push(+(THR === 't1'), Math.abs(tc.scrollTop - y)); \
          tap(tc.querySelector('.md .tl')); await until(() => vis('t27')); tap(v('t27').querySelector('.rst')); \
          await until(() => v('t27') && !v('t27').querySelector('.tw textarea').disabled); \
          at.push(+!v('t27').querySelector('.tw textarea').disabled, +!v('t27').querySelector('.rst'), errs.length); })()";
    let png = std::env::var_os("HERDSMAN_SHOT").map_or_else(|| tmpdir("thread-refs-png").join("refs.png"), Into::into);
    let o = swift()
        .env("HERDSMAN_WAIT", "12")
        .env("HERDSMAN_COOKIE", cookie.trim().trim_start_matches("Cookie: "))
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift"),
            &format!("http://{addr}/"),
            "390",
            "chat",
            "scene",
            png.to_str().unwrap(),
            js,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    let m: Vec<f64> = out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect();
    // The bar's id; t1 scrolled mid-way; the tap: t27 open alone, [restore], composer off; the bar's back: t1 at the
    // same scroll; the browser's back likewise; [restore]: the composer on, no [restore]; no errors.
    assert_eq!(m, [1., 1., 1., 1., 1., 1., 1., 1., 0., 1., 0., 1., 1., 0.], "{out}");
}
