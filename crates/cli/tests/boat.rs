//! #635: the built-in boat recipe, run for real by `sh` against a fake `boat` with the current CLI's subcommands
//! (`new`, `list`, `resume`, `stop`, `delete`, `snapshot`); its rates and the old `medium` size; the cloud key under the
//! host's fleet; the ssh files it writes; doctor's boat rows.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;

const NOW: u64 = 1_000_000;
const KEY: &str = "boat-key-635";

/// A fake `boat`: sandboxes in `<bin>/sandboxes` (`id name state type`), each call in `<bin>/calls`; an unknown
/// subcommand fails as clap does. `ssh-keyscan` answers for the endpoint it gives.
const BOAT: &str = r#"#!/bin/sh
d=$(dirname "$0"); s="$d/sandboxes"; touch "$s"; echo "$*" >> "$d/calls"
# As boat does, an empty id is the current sandbox (the one boat runs in): recorded, never an error.
id_ok() { [ -n "$1" ] || { echo "ACTED ON THE CURRENT SANDBOX" >> "$d/calls"; exit 0; }
  grep -q "^$1 " "$s" || { echo "error: no sandbox $1" >&2; exit 1; }; }
set_state() { sed "s/^$1 \([^ ]*\) [^ ]*/$1 \1 $2/" "$s" > "$s.new" && mv "$s.new" "$s"; }
# #707: boat owns its login (`boat login`); herdsman hands it no key.
[ "$1" = --help ] || [ -f "$d/login" ] || { echo "error: not logged in" >&2; exit 1; }
[ -z "$BOAT_API_KEY" ] || [ "$BOAT_API_KEY" = "boat-key-635" ] || { echo "error: bad key" >&2; exit 1; }
case "$1" in
--help) printf 'Usage: boat [OPTIONS] [COMMAND]\n\nCommands:\n  new       Create\n  info      Show\n  list      List\n  delete    Delete\n  stop      Stop\n  resume    Resume\n  ssh       SSH\n  snapshot  Save\n  help      Help\n' ;;
new) shift; t=default; n=
  while [ $# -gt 0 ]; do case "$1" in --type) t=$2; shift ;; --name) n=$2; shift ;; --from) shift ;; esac; shift; done
  case "$t" in small|default|large|xlarge) ;; *) echo "error: invalid value '$t' for '--type'" >&2; exit 2 ;; esac
  i="bx_$(wc -l < "$s" | tr -d ' ')"; echo "$i $n running $t" >> "$s"; echo "$i" ;;
list) all=; for a in "$@"; do [ "$a" = --all ] && all=1; done
  printf '{"sandboxes":['; sep=
  while read -r i n st t; do
    [ -n "$all" ] || [ "$st" = running ] || continue
    e=null; [ "$st" = running ] && e="\"$(cat "$d/endpoint" 2>/dev/null || echo 10.0.0.7:2222)\""
    printf '%s{"id":"%s","name":"%s","state":"%s","type":"%s","sshEndpoint":%s}' "$sep" "$i" "$n" "$st" "$t" "$e"; sep=,
  done < "$s"; printf ']}\n' ;;
resume) id_ok "$2"; set_state "$2" running ;;
stop) id_ok "$2"; set_state "$2" stopped ;;
delete) id_ok "$2"; grep -v "^$2 " "$s" > "$s.new"; mv "$s.new" "$s" ;;
snapshot) id_ok "$2" ;;
*) echo "error: unrecognized subcommand '$1'" >&2; exit 2 ;;
esac
"#;
const KEYSCAN: &str =
    "#!/bin/sh\necho \"[x]:1 ssh-ed25519 $(cat \"$(dirname \"$0\")/hostkey\" 2>/dev/null || echo AAAAboatkey)\"\n";

/// A recipe's `sh -c` runs for real with the fake `boat` first on PATH; everything else goes to the fake runner.
struct Boat(Fake, PathBuf);

impl Runner for Boat {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if prog != "sh" {
            return self.0.run_in(prog, args, input);
        }
        let path = format!("{}:{}", self.1.display(), std::env::var("PATH").unwrap_or_default());
        let mut p = std::process::Command::new("sh")
            .args(args)
            .env("PATH", path)
            .env_remove("BOAT_API_KEY")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        p.stdin.take().unwrap().write_all(input.as_bytes())?;
        let o = p.wait_with_output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

/// One fleet (sendmeter) and boat1 on the built-in recipe at `size`; the fake boat CLI in `<dir>/bin`.
fn setup(name: &str, size: &str) -> (herdsman::Ctx, PathBuf) {
    let dir = tmpdir(name);
    let host =
        format!("\n[hosts.boat1]\nmachine = \"boat1\"\ncloud = \"boat\"\nsize = \"{size}\"\nbudget_usd_month = 10\n");
    let c = ctx(&dir, &format!("{CONFIG}{host}"));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("login"), "").unwrap();
    for (f, text) in [("boat", BOAT), ("ssh-keyscan", KEYSCAN)] {
        std::fs::write(bin.join(f), text).unwrap();
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(bin.join(f))
            .status()
            .unwrap();
    }
    (c, bin)
}

fn calls(bin: &std::path::Path) -> String {
    std::fs::read_to_string(bin.join("calls")).unwrap_or_default()
}

#[test]
fn bootstrap_boat1_creates_it_with_the_current_boat_cli_and_gets_past_start() {
    let (c, bin) = setup("boat-boot", "medium");
    let cfg = Config::load(&c.config).unwrap();
    let r = Boat(Fake::default().reply("herdr machine list --json", "[]"), bin.clone());
    let mut out = vec![];
    let e = herdsman::cloud::bootstrap(&c, &cfg, &r, "boat1", &mut out).unwrap_err();
    let e = format!("{e:#}");
    // Past start: the ssh probe of the new host is the next step (the fake runner has no ssh there).
    assert!(
        !e.contains("start failed") && e.contains("can't reach boat1"),
        "{e}\n{}",
        calls(&bin)
    );
    let made = calls(&bin);
    assert!(made.contains("new --type default --name boat1"), "{made}");
    let kinds: Vec<_> = read(&c.state).unwrap().into_iter().map(|e| e.kind).collect();
    assert_eq!(kinds, ["cloud-start", "cloud-up"]);
    let conf = std::fs::read_to_string(c.home.join(".ssh/config.d/herdsman")).unwrap();
    assert!(
        conf.contains("Host boat1\n  HostName 10.0.0.7\n  User user\n  Port 2222\n"),
        "{conf}"
    );
    // Stopped, it resumes the same sandbox (no second `new`).
    herdsman::cloud::stop(&c, &cfg, &r, "boat1", "stop", "test").unwrap();
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    let made = calls(&bin);
    assert!(
        made.matches("new ").count() == 1 && made.contains("resume bx_0"),
        "{made}"
    );
}

#[test]
fn the_ssh_files_are_all_valid_config_and_the_host_key_stays_pinned() {
    let (c, bin) = setup("boat-ssh", "small");
    let cfg = Config::load(&c.config).unwrap();
    let r = Boat(Fake::default(), bin.clone());
    let old = c.home.join(".ssh/config.d/herdsman-boat1.known");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, "[10.0.0.7]:2222 ssh-ed25519 OLD\n").unwrap();
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    // `Include config.d/*` reads every file there as ssh config: each line a comment or a directive.
    for f in std::fs::read_dir(c.home.join(".ssh/config.d")).unwrap() {
        let text = std::fs::read_to_string(f.unwrap().path()).unwrap();
        for l in text.lines() {
            let word = l.split_whitespace().next().unwrap_or("#");
            let known = [
                "#",
                "Host",
                "HostName",
                "User",
                "Port",
                "UserKnownHostsFile",
                "IdentityFile",
                "IdentitiesOnly",
            ];
            assert!(known.contains(&word) || word.starts_with('#'), "not ssh config: {l}");
        }
    }
    let pin = c.home.join(".ssh/herdsman/boat1.known");
    assert_eq!(
        std::fs::read_to_string(&pin).unwrap(),
        "[10.0.0.7]:2222 ssh-ed25519 AAAAboatkey\n"
    );
    let conf = std::fs::read_to_string(c.home.join(".ssh/config.d/herdsman")).unwrap();
    assert!(
        conf.contains(&format!("UserKnownHostsFile {}", pin.display())),
        "{conf}"
    );
    // Amendment D: a fresh boat sandbox authorizes only boat's own key.
    assert!(
        conf.contains("  IdentityFile ~/.ssh/ascii_box_ed25519\n  IdentitiesOnly yes\n"),
        "{conf}"
    );
    // A changed host key on a later start fails closed; the pin is kept.
    std::fs::write(
        bin.join("ssh-keyscan"),
        "#!/bin/sh\necho '[10.0.0.7]:2222 ssh-ed25519 EVIL'\n",
    )
    .unwrap();
    herdsman::cloud::stop(&c, &cfg, &r, "boat1", "stop", "test").unwrap();
    let e = herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap_err();
    assert!(format!("{e:#}").contains("host key changed"), "{e:#}");
    assert!(std::fs::read_to_string(&pin).unwrap().contains("AAAAboatkey"));
}

#[test]
fn boat_runs_on_its_own_login_with_no_key_and_doctor_finds_a_key_file() {
    let (c, bin) = setup("boat-key", "small");
    let cfg = Config::load(&c.config).unwrap();
    let r = Boat(Fake::default(), bin.clone());
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    assert!(calls(&bin).contains("new --type small --name boat1"), "{}", calls(&bin));
    std::fs::write(c.home.join(".ssh/ascii_box_ed25519"), "k").unwrap();
    let (_, out) = run(&c, &r, &["doctor"]);
    assert!(
        out.contains("! cloud boat1 key — BOAT_API_KEY not in env or ")
            && out.contains("boat's CLI uses its own login"),
        "{out}"
    );
    assert!(
        out.contains("✓ cloud boat1 identity — ~/.ssh/ascii_box_ed25519"),
        "{out}"
    );
    // #707: the API key `[ci_runners]` calls need, in its 0600 file: found, and handed to the command.
    let f = secret_file(&c, "BOAT_API_KEY", KEY, 0o600);
    let (_, out) = run(&c, &r, &["doctor"]);
    assert!(out.contains(&format!("✓ cloud boat1 key — {}", f.display())), "{out}");
    herdsman::cloud::stop(&c, &cfg, &r, "boat1", "stop", "test").unwrap();
}

#[test]
fn the_rates_are_boats_and_medium_is_default() {
    let (c, bin) = setup("boat-rates", "medium");
    let mut cfg = Config::load(&c.config).unwrap();
    let e = Event::new(NOW - 3600, "herdsman", None, "cloud-start", json!({"host": "boat1"}));
    append(&c.state, &e).unwrap();
    let events = read(&c.state).unwrap();
    for (size, rate) in [
        ("small", 0.018),
        ("default", 0.036),
        ("large", 0.072),
        ("xlarge", 0.2),
        ("medium", 0.036),
    ] {
        cfg.hosts.get_mut("boat1").unwrap().size = Some(size.into());
        let (usd, hours) = herdsman::cloud::spend(&c, &cfg, &events, "boat1").unwrap();
        assert!(
            (usd - rate).abs() < 1e-9 && (hours - 1.0).abs() < 1e-9,
            "{size}: {usd} {hours}"
        );
    }
    let r = Boat(Fake::default(), bin);
    let (_, out) = run(&c, &r, &["doctor"]);
    assert!(
        out.contains("! cloud boat1 size — \"medium\" is boat's \"default\""),
        "{out}"
    );
    assert!(out.contains("! cloud boat1 key — BOAT_API_KEY not in env"), "{out}");
    assert!(out.contains("✓ cloud boat1 cli — boat has"), "{out}");
    assert!(
        out.contains("✗ cloud boat1 identity — ~/.ssh/ascii_box_ed25519 missing"),
        "{out}"
    );
}

#[test]
fn doctor_flags_a_recipe_subcommand_the_boat_cli_lacks_and_an_include_of_a_known_file() {
    let (c, bin) = setup("boat-doctor", "small");
    let recipe = c.config.with_file_name("recipes/boat.toml");
    std::fs::create_dir_all(recipe.parent().unwrap()).unwrap();
    let old = "key_env = \"BOAT_API_KEY\"\nstart = \"boat resume {name}\"\nstatus = \"boat ssh-info {name}\"\n";
    std::fs::write(&recipe, old).unwrap();
    let ssh = c.home.join(".ssh");
    std::fs::create_dir_all(ssh.join("config.d")).unwrap();
    std::fs::write(
        ssh.join("config.d/herdsman-boat1.known"),
        "[1.2.3.4]:22 ssh-ed25519 K\n",
    )
    .unwrap();
    std::fs::write(ssh.join("config"), "Include config.d/*\n\nHost *\n").unwrap();
    let r = Boat(Fake::default(), bin);
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("✗ cloud boat1 cli — boat has no `ssh-info` (recipe status = \"boat ssh-info {name}\")"),
        "{out}"
    );
    assert!(!out.contains("no `resume`"), "{out}");
    assert!(
        out.contains("✗ ssh include — `Include config.d/*` reads config.d/herdsman-boat1.known"),
        "{out}"
    );
}

#[test]
fn a_sandbox_the_default_list_omits_or_that_is_gone_is_never_acted_on_by_an_empty_id() {
    let (c, bin) = setup("boat-empty-id", "small");
    let cfg = Config::load(&c.config).unwrap();
    let r = Boat(Fake::default(), bin.clone());
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    // Stopped, boat's default list omits it: `--all` still finds it, by its id.
    herdsman::cloud::stop(&c, &cfg, &r, "boat1", "stop", "test").unwrap();
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    // Gone (deleted outside herdsman): stop and destroy are refused, boat never called with an empty id.
    std::fs::write(bin.join("sandboxes"), "").unwrap();
    for verb in ["stop", "destroy"] {
        let e = herdsman::cloud::stop(&c, &cfg, &r, "boat1", verb, "test").unwrap_err();
        assert!(format!("{e:#}").contains("no sandbox named boat1"), "{e:#}");
    }
    let made = calls(&bin);
    assert!(made.contains("stop bx_0") && made.contains("resume bx_0"), "{made}");
    assert!(!made.contains("CURRENT"), "{made}");
    // An override recipe whose own lines have no guard: herdsman refuses on the recipe's empty `id` itself.
    let recipe = c.config.with_file_name("recipes/boat.toml");
    std::fs::create_dir_all(recipe.parent().unwrap()).unwrap();
    let id = r#"boat list --all --json | jq -r 'first(.sandboxes[]|select(.name=="{name}"))|.id'"#;
    let bare = format!(
        "key_env = \"BOAT_API_KEY\"\nid = '''{id}'''\ndestroy = '''boat delete \"$({id})\" --yes'''\n\
         stop = '''boat stop \"$({id})\"'''\n[rate_usd_hour]\nsmall = 0.018\n"
    );
    std::fs::write(&recipe, bare).unwrap();
    for verb in ["stop", "destroy"] {
        let e = herdsman::cloud::stop(&c, &cfg, &r, "boat1", verb, "test").unwrap_err();
        assert!(format!("{e:#}").contains("no sandbox named boat1"), "{e:#}");
    }
    let made = calls(&bin);
    assert!(!made.contains("CURRENT") && !made.contains("delete"), "{made}");
}

#[test]
fn bootstrap_and_survey_of_a_running_host_rewrite_a_moved_address() {
    let (c, bin) = setup("boat-moved", "small");
    let cfg = Config::load(&c.config).unwrap();
    let r = Boat(Fake::default().reply("herdr machine list --json", "[]"), bin.clone());
    herdsman::cloud::start(&c, &cfg, &r, "boat1").unwrap();
    let conf = || std::fs::read_to_string(c.home.join(".ssh/config.d/herdsman")).unwrap();
    let pin = c.home.join(".ssh/herdsman/boat1.known");
    // Moved while running: bootstrap re-reads `address` before it reaches the host.
    std::fs::write(bin.join("endpoint"), "10.0.0.8:2200").unwrap();
    let _ = herdsman::cloud::bootstrap(&c, &cfg, &r, "boat1", &mut Vec::new());
    assert!(
        conf().contains("  HostName 10.0.0.8\n  User user\n  Port 2200\n"),
        "{}",
        conf()
    );
    assert_eq!(
        std::fs::read_to_string(&pin).unwrap(),
        "[10.0.0.8]:2200 ssh-ed25519 AAAAboatkey\n"
    );
    // A survey that finds it running and moved again rewrites it too.
    std::fs::write(bin.join("endpoint"), "10.0.0.9:2201").unwrap();
    herdsman::hosts::survey(&c, &cfg, &r, true);
    assert!(
        conf().contains("  HostName 10.0.0.9\n  User user\n  Port 2201\n"),
        "{}",
        conf()
    );
    // Recreated (a new host key): a survey never accepts it; bootstrap re-pins it, logged.
    std::fs::write(bin.join("endpoint"), "10.0.0.10:22").unwrap();
    std::fs::write(bin.join("hostkey"), "AAAAnewkey").unwrap();
    herdsman::hosts::survey(&c, &cfg, &r, true);
    assert!(conf().contains("HostName 10.0.0.9\n"), "{}", conf());
    assert!(std::fs::read_to_string(&pin).unwrap().contains("AAAAboatkey"));
    let _ = herdsman::cloud::bootstrap(&c, &cfg, &r, "boat1", &mut Vec::new());
    assert!(conf().contains("HostName 10.0.0.10\n"), "{}", conf());
    assert_eq!(
        std::fs::read_to_string(&pin).unwrap(),
        "10.0.0.10 ssh-ed25519 AAAAnewkey\n"
    );
    let ev = read(&c.state).unwrap();
    let repin = ev.iter().find(|e| e.kind == "cloud-repin").expect("cloud-repin logged");
    assert_eq!(repin.str("was"), Some("ssh-ed25519 AAAAboatkey"));
}
