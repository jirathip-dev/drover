use super::{Fake, FakeShed, OverSsh};
use drover::runner::{Out, Runner};
use serde_json::{json, Value};

/// Runner `r` whose remote `hosts` pass a remote ACP lane's guard: what `r` leaves unscripted there is answered as a synced host with shed's `claude-acp` (the profile's hook, `shed agents|help|run|send|cancel|kill` on that host's server) and this host's shed lists them (`shed machine ls`).
pub struct Acp<'a, R> {
    pub r: R,
    pub hosts: Vec<(&'a str, &'a FakeShed)>,
}

impl<'a> Acp<'a, OverSsh<'a>> {
    /// `Acp` around `ssh`, on its hosts.
    pub fn over(ssh: OverSsh<'a>) -> Self {
        Acp {
            hosts: ssh.hosts.clone(),
            r: ssh,
        }
    }
}

impl<R: Runner> Acp<'_, R> {
    fn answer(&self, prog: &str, args: &[String]) -> Option<Out> {
        if prog == "shed" && args.iter().map(String::as_str).eq(["machine", "ls", "--json"]) {
            let names: Vec<Value> = self.hosts.iter().map(|(h, _)| json!({"name": h})).collect();
            return Some(out(&json!(names).to_string()));
        }
        let end = args.iter().position(|a| a == "--").filter(|_| prog == "ssh")?;
        let m = args.get(end.checked_sub(1)?)?;
        let sh = self.hosts.iter().find(|(h, _)| m == h)?.1;
        let w = words(args.last()?);
        let w: Vec<&str> = w.iter().map(String::as_str).collect();
        let stdout = match w[..] {
            ["cat", p] if p.ends_with("/settings.json") => settings(),
            [exe, "profiles", "sync", ..] if exe.ends_with("drover") => String::new(),
            ["shed", "help", "run", ..] => "--meta <JSON>".into(),
            ["shed", "agents", "--json"] => {
                json!([{"id": "claude-acp", "version": "0.101.0", "cmd": ["claude-acp"]}]).to_string()
            }
            ["shed", "run", "--acp", ..] => {
                let at = |k: &str| w.iter().position(|a| *a == k).map(|i| w[i + 1]).unwrap_or_default();
                let labels: serde_json::Map<String, Value> = w
                    .windows(2)
                    .filter(|p| p[0] == "--label")
                    .filter_map(|p| p[1].split_once('='))
                    .map(|(k, v)| (k.to_string(), json!(v)))
                    .collect();
                let t = json!({"name": at("--name"), "labels": labels, "cwd": at("--cwd"), "cmd": ["shed"], "keep": false,
                    "state": "running", "exit": null, "pid": 3, "acp": at("--acp")});
                sh.lock().terms.insert(at("--name").into(), t);
                String::new()
            }
            ["shed", "send", name, text] => {
                sh.lock()
                    .seen
                    .push(("send".into(), json!({"name": name, "text": text})));
                String::new()
            }
            ["shed", "cancel", _] => String::new(),
            ["shed", "kill", name] => {
                if let Some(t) = sh.lock().terms.get_mut(name) {
                    t["state"] = json!("exited");
                }
                String::new()
            }
            _ => return None,
        };
        Some(out(&stdout))
    }
    fn or_acp(&self, prog: &str, args: &[String], o: anyhow::Result<Out>) -> anyhow::Result<Out> {
        match o {
            Ok(o) if o.code == 1 && o.stderr.starts_with("unscripted: ") => Ok(self.answer(prog, args).unwrap_or(o)),
            o => o,
        }
    }
}

impl<R: Runner> Runner for Acp<'_, R> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.or_acp(prog, args, self.r.run(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.or_acp(prog, args, self.r.run_in(prog, args, input))
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        self.r.shed()
    }
}

/// The exports the last `shed run --acp` over ssh to `host` in `f`'s calls starts its lane with: its `sh -c` line up to the profile copy (`DROVER_*`, env, the secrets file sourced and removed).
pub fn start_line(f: &Fake, host: &str) -> Option<String> {
    let calls = f.calls.borrow();
    let c = calls
        .iter()
        .rfind(|c| c.contains(&format!(" {host} -- 'shed' 'run' '--acp'")))?;
    let w = words(c.split_once(" -- ")?.1);
    let i = w.iter().position(|x| x == "--")?;
    let line = w.get(i + 3).filter(|_| w[i + 1] == "sh" && w[i + 2] == "-c")?;
    Some(line.split(" && mkdir -p .claude").next()?.to_string())
}

fn settings() -> String {
    let hooks: Vec<Value> = ["impl", "rev", "design", "architect", "orchestrator"]
        .iter()
        .map(|f| json!({"type": "command", "command": format!("'/opt/drover/bin/drover' hook --role {f}")}))
        .collect();
    json!({"hooks": {"PreToolUse": [{"matcher": "*", "hooks": hooks}]}}).to_string()
}

fn out(stdout: &str) -> Out {
    Out {
        stdout: stdout.into(),
        ..Out::default()
    }
}

fn words(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q, mut any) = (vec![], String::new(), false, false);
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match (q, c) {
            (true, '\'') => q = false,
            (true, c) => cur.push(c),
            (false, '\'') => (q, any) = (true, true),
            (false, '\\') => cur.extend(it.next()),
            (false, ' ') if any => (out.push(std::mem::take(&mut cur)), any = false).1,
            (false, ' ') => {}
            (false, c) => (cur.push(c), any = true).1,
        }
    }
    out.extend(any.then_some(cur));
    out
}
