//! #205: an open Queue card lists its issue's lanes newest first (5, then `+N older`); a lane herdr lists
//! (and the card's current lane name) links to its Lanes sheet, a gone one is muted text with no tap.
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
    let page = String::from_utf8_lossy(&Server::new(&c, &r, None).respond("GET / HTTP/1.1\r\n\r\n", T)).into_owned();
    let card = &page[page.find("<div class=qls>").expect("lanes list")..];
    let card = &card[..card.find("</div>").unwrap()];
    let at = |a: &str| card.find(&format!(" {a} ")).unwrap_or_else(|| panic!("no {a}: {card}"));
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
        "<div class=qls><a href=#lanes class=\"ql nw el\" data-agent=\"impl-3-u1\"><span class=g>●</span> impl-3-u1 working 54m</a>\
         <a href=#lanes class=\"ql nw el\" data-agent=\"rev-3-r3\"><span class=g>○</span> rev-3-r3 done 55m</a>"
    ), "{card}");
    assert!(
        card.contains("<span class=\"ql nw el m\"><span class=m>○</span> rev-3-r2 gone 57m</span>"),
        "{card}"
    );
    assert!(
        !card.contains("data-agent=\"rev-3-r2\""),
        "a gone lane has no tap target: {card}"
    );
    // The card's current lane name opens the same sheet.
    assert!(
        page.contains("<p><a href=#lanes class=\"ql nw el\" data-agent=\"impl-3-u1\">impl-3-u1</a></p>"),
        "{page}"
    );
}
