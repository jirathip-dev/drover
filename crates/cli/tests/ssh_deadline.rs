use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn every_ssh_drover_starts_is_bounded_by_a_deadline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = vec![];
    for c in ["core/src", "server/src", "cli/src"] {
        sources(&root.join(c), &mut files);
    }
    files.sort();
    let starts = [
        "run(",
        "run_in(",
        "run_for(",
        "detach(",
        "ok(",
        "json(",
        "Command::new(",
        "stream(",
    ];
    let mut seen = vec![];
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for l in text.lines().map(str::trim) {
            if l.contains("\"ssh\", &") && starts.iter().any(|s| l.contains(s)) {
                let name = f.strip_prefix(&root).unwrap().display().to_string();
                seen.push(format!("{name}: {l}"));
            }
        }
    }
    let probe =
        "core/src/hosts.rs: let o = self.r.run_for(\"ssh\", &probe, \"\", std::time::Duration::from_secs(secs));";
    let config = "core/src/hosts.rs: let g = ok(self.r, \"ssh\", &[\"-G\", CM[1], m]).unwrap_or_default();";
    let tunnel =
        "core/src/proxy.rs: let n = r.detach(\"ssh\", &a, &ctx.state.with_file_name(format!(\"tunnel-{name}.log\")))?;";
    assert_eq!(
        seen,
        [probe, config, tunnel],
        "an ssh is started only by the host probe (its deadline), `ssh -G` (no connection) and the detached proxy tunnel; every other one goes through hosts::On::wrap, whose ssh runs by run_for"
    );
    let hosts = std::fs::read_to_string(root.join("core/src/hosts.rs")).unwrap();
    let routed = "        if p == \"ssh\" {\n            return self.r.run_for(";
    assert_eq!(
        hosts.matches(routed).count(),
        2,
        "On::run and On::run_in hand every wrapped ssh to run_for with a limit"
    );
    let pass = hosts.split("impl Runner for Pass<'_> {").nth(1).unwrap();
    let pass = &pass[..pass.find("\n}\n").unwrap()];
    assert!(
        pass.contains("self.call(prog, args, None)") && pass.contains("self.call(prog, args, Some(input))"),
        "a pass's run and run_in go through Pass::call"
    );
    let call = hosts.split("    fn call(&self").nth(1).unwrap();
    let call = &call[..call.find("\n    fn ").unwrap()];
    let remote = call.split_once("        };\n").unwrap().1;
    assert!(
        remote.contains("crate::config::limits().ssh_call_secs")
            && !remote.contains("self.r.run(")
            && !remote.contains("self.r.run_in("),
        "Pass::call runs every remote call with the ssh_call_secs deadline"
    );
}
