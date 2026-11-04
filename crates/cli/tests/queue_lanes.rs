//! #205: an open Queue card lists its issue's lanes newest first (5, then `+N older`); a lane herdr lists
//! (and the card's current lane name) links to its Lanes sheet, a gone one is muted text with no tap; #218: unless its
//! transcript is stored (`data-gone`). The name and `state age` are spans, so the name ellipsises first.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
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
    let r = Fake::default().reply(
        "herdr agent list",
        &agents(&[
            ("impl-3-u1", "working", "/x", "w1:p1"),
            ("rev-3-r3", "done", "/x", "w1:p2"),
        ]),
    );
    // #268: the Queue tab as the board draws it from API v1's `queue`.
    let page = queue_tab(&Server::new(&c, &r, None), "", T);
    let card = &page[page.find("<div class=qls>").expect("lanes list")..];
    let card = &card[..card.find("</div>").unwrap()];
    let at = |a: &str| card.find(&format!(">{a}<")).unwrap_or_else(|| panic!("no {a}: {card}"));
    let mut order: Vec<&str> = names.to_vec();
    order.reverse();
    assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "newest first: {card}");
    let older = card
        .find("<details class=qm><summary>+2 older</summary>")
        .expect("+2 older");
    assert!(
        at("impl-3-f1") < older && older < at("rev-3-r1"),
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
    // #218: a gone lane with a stored transcript links to it, marked `data-gone`.
    let log = herdsman::lanes::log(&c.state, "rev-3-r2").unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "  out\n").unwrap();
    let page2 = queue_tab(&Server::new(&c, &r, None), "", T);
    assert!(
        page2.contains("<a href=#lanes class=\"ql nw el\" data-agent=\"rev-3-r2\" data-gone><span class=m>○</span>"),
        "{page2}"
    );
    std::fs::remove_file(&log).unwrap();
    // The card's current lane name opens the same sheet.
    assert!(
        page.contains("<p><a href=#lanes class=\"ql nw el\" data-agent=\"impl-3-u1\">impl-3-u1</a></p>"),
        "{page}"
    );
    // A current lane herdr no longer lists stays plain text.
    let page = queue_tab(&Server::new(&c, &Fake::default(), None), "", T);
    assert!(page.contains("<p>impl-3-u1</p>"), "{page}");
}
