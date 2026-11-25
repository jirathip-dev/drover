use drover::runner::{ok, Real};

#[test]
fn system_tools_answer_the_same_without_sbin_on_the_callers_path() {
    let full = drover::host::sample(&Real);
    let measured = |h: &drover::host::Host| (h.pressure.is_some(), h.swap_count.is_some(), h.cpu_idle.is_some());
    std::env::set_var("PATH", "/usr/bin:/bin");
    let narrow = drover::host::sample(&Real);
    assert_eq!(measured(&narrow), measured(&full), "{narrow:?} vs {full:?}");
    let key = if cfg!(target_os = "macos") {
        "kern.ostype"
    } else {
        "kernel.ostype"
    };
    let os = ok(&Real, "sysctl", &["-n", key]);
    assert!(os.is_ok(), "{os:?}");
}
