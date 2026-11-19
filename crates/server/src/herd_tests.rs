use super::*;
use crate::api::HerdHost;

#[test]
fn the_herd_scene_sends_the_load_level_of_the_host() {
    for (load, gate, want) in [
        (0.4, None, "calm"),
        (0.5, None, "busy"),
        (1.3, None, "busy"),
        (0.4, Some("load"), "held"),
    ] {
        let host = HerdHost {
            load: Some(load),
            limit: 1.0,
            pressure: Some("normal".into()),
            gate: gate.map(Into::into),
            ..HerdHost::default()
        };
        let s = herd_screen(&Herd {
            host,
            ..Herd::default()
        });
        assert_eq!(s["blocks"][0]["load"], want, "load {load}, gate {gate:?}");
    }
    let host = HerdHost {
        load: Some(0.1),
        limit: 1.0,
        pressure: Some("warn".into()),
        ..HerdHost::default()
    };
    assert_eq!(
        herd_screen(&Herd {
            host,
            ..Herd::default()
        })["blocks"][0]["load"],
        "busy"
    );
}
