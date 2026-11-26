use drover::config::Limits;
use drover::flow::facts::{host, Host};
use drover::link::frame::Sample;
use drover::link::view::HostSeen;

const NOW: u64 = 1_000_000;

fn row(name: &str, heard: u64, idle: f64) -> HostSeen {
    HostSeen {
        host: name.into(),
        link: "down".into(),
        since: NOW - 7200,
        heard: Some(heard),
        sample: Some(Sample {
            cpu_idle: Some(idle),
            pressure: Some("normal".into()),
            at: heard,
        }),
        ..HostSeen::default()
    }
}

#[test]
fn a_loaded_main_host_leaves_another_host_up() {
    let l = Limits::default();
    let main = row("mini", NOW, 2.0);
    let late = row("bazzite", NOW - 5 * 60, 90.0);
    let (v, why) = host(Some(&late), Some(&main), &l, NOW);
    assert_eq!(v, Host::Up, "{why}");
    let idle = row("mini", NOW, 90.0);
    assert_eq!(
        host(Some(&late), Some(&idle), &l, NOW).0,
        Host::Offline,
        "late with neither end loaded"
    );
}

#[test]
fn a_late_frame_from_a_loaded_host_is_loaded() {
    let l = Limits::default();
    let late = row("bazzite", NOW - 5 * 60, 4.0);
    let (v, why) = host(Some(&late), Some(&row("mini", NOW, 90.0)), &l, NOW);
    assert_eq!(v, Host::Loaded, "{why}");
    assert!(why.contains("cpu idle 4%") && why.contains("last frame 5m"), "{why}");
}

#[test]
fn no_frame_for_host_offline_minutes_is_offline_at_any_load() {
    let l = Limits::default();
    let gone = row("bazzite", NOW - l.host_offline_minutes * 60, 1.0);
    let main = row("mini", NOW, 1.0);
    let (v, why) = host(Some(&gone), Some(&main), &l, NOW);
    assert_eq!(v, Host::Offline, "{why}");
    let live = HostSeen {
        link: "live".into(),
        ..gone
    };
    assert_eq!(host(Some(&live), Some(&main), &l, NOW).0, Host::Offline);
}
