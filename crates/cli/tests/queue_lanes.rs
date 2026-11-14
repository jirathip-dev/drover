mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const T: u64 = 1_000_000;

#[test]
fn lanes_newest_first_capped_at_five_and_a_gone_lane_has_no_link() {
    let c = ctx(&tmpdir("queue-lanes"), CONFIG);
    let names = [
        "impl-3",
        "rev-3-r1",
        "impl-3-f1",
        "rev-3-r2",
        "impl-3-f2",
        "rev-3-r3",
        "impl-3-u1",
    ];
    for (i, a) in names.iter().enumerate() {
        let d = json!({"role": if a.starts_with("rev") { "rev" } else { "impl" }, "agent": a, "harness": "claude"});
        append(
            &c.state,
            &Event::new(T - 3600 + i as u64 * 60, "sendmeter", Some(3), "spawn", d),
        )
        .unwrap();
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    for (n, status) in [("impl-3-u1", "working"), ("rev-3-r3", "done")] {
        sh.lane(n, status, "/x");
        sh.screen(n, "");
    }
    let f = Fake::default();
    let r = sh.on(&f);
    let page = queue_tab(&Server::new(&c, &r, None), "", T);
    let card = &page[page.find("<div class=qls>").expect("lanes list")..];
    let card = &card[..card.find("</div>").unwrap()];
    let at = |a: &str| card.find(&format!(">{a}<")).unwrap_or_else(|| panic!("no {a}: {card}"));
    let mut order: Vec<&str> = names.to_vec();
    order.reverse();
    assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "newest first: {card}");
    let prior = card
        .find("<details class=qm><summary>+2 older</summary>")
        .expect("+2 older");
    assert!(
        at("impl-3-f1") < prior && prior < at("rev-3-r1"),
        "the 2 oldest under +2 older: {card}"
    );
    assert!(card.starts_with(
        "<div class=qls><a href=#lanes class=\"ql nw el\" data-agent=\"impl-3-u1\"><span class=g>●</span> <span class=qn>impl-3-u1</span> <span class=qa>working 54m</span></a>\
         <a href=#lanes class=\"ql nw el\" data-agent=\"rev-3-r3\"><span class=g>○</span> <span class=qn>rev-3-r3</span> <span class=qa>done 55m</span></a>"
    ), "{card}");
    assert!(
        card.contains("<span class=\"ql nw el m\"><span class=m>○</span> <span class=qn>rev-3-r2</span> <span class=qa>gone 57m</span></span>"),
        "{card}"
    );
    assert!(
        !card.contains("data-agent=\"rev-3-r2\""),
        "a gone lane has no tap target: {card}"
    );
    let log = drover::lanes::log(&c.state, "rev-3-r2").unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "  out\n").unwrap();
    let page2 = queue_tab(&Server::new(&c, &r, None), "", T);
    assert!(
        page2.contains("<a href=#lanes class=\"ql nw el\" data-agent=\"rev-3-r2\" data-gone><span class=m>○</span>"),
        "{page2}"
    );
    std::fs::remove_file(&log).unwrap();
    assert!(
        page.contains("<p><a href=#lanes class=\"ql nw el\" data-agent=\"impl-3-u1\">impl-3-u1</a></p>"),
        "{page}"
    );
    drop(sh);
    let page = queue_tab(&Server::new(&c, &r, None), "", T);
    assert!(page.contains("<p>impl-3-u1</p>"), "{page}");
}
