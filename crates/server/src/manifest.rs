use crate::api::{Manifest, Place, Threads};
use serde::Serialize;

const ACTIONS: [&str; 29] = [
    "filter",
    "drive",
    "pause",
    "drive-all",
    "force",
    "later",
    "top",
    "limit",
    "answer",
    "unpark",
    "run-card",
    "needs-human",
    "save-fleets",
    "stop",
    "drop",
    "remove-fleet",
    "kill",
    "clean",
    "close",
    "send",
    "move",
    "queue",
    "config-card",
    "secret-card",
    "host",
    "reap",
    "orphans",
    "new-issue",
    "scope-drop",
];

const DESTRUCTIVE: [&str; 13] = [
    "stop",
    "drop",
    "remove-fleet",
    "run-card",
    "needs-human",
    "kill",
    "clean",
    "close",
    "config-card",
    "host",
    "reap",
    "orphans",
    "scope-drop",
];

const ATTACH: &str = "http://127.0.0.1:7777";

fn place(kind: &str, id: Option<&str>, persona: Option<&str>, label: Option<&str>, more: [Option<&str>; 2]) -> Place {
    let s = |v: Option<&str>| v.map(String::from);
    Place {
        kind: kind.into(),
        id: s(id),
        persona: s(persona),
        label: s(label),
        subtitle: s(more[0]),
        screen: s(more[1]),
    }
}

fn strip(id: &str, label: &str) -> Place {
    let screen = format!("/screens/{id}");
    place(
        "header-strip",
        Some(id),
        Some("conductor"),
        Some(label),
        [None, Some(&screen)],
    )
}

/// The manifest a client reads, `api` the base URL it reached drover on.
pub fn manifest(api: &str) -> Manifest {
    let menu = [Some("drover's fleets"), Some("/screens/fleets")];
    let own = |l: &[&str]| l.iter().map(|a| (*a).to_string()).collect();
    let p = |s: &str| s.to_string();
    Manifest {
        name: "drover".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        api: api.into(),
        screens: vec![
            strip("queue", "Queue"),
            strip("events", "Events"),
            strip("lanes", "Lanes"),
            place("chat-cards", None, Some("conductor"), None, [None, None]),
            place("menu-row", None, None, Some("drover"), menu),
        ],
        actions: own(&ACTIONS),
        destructive: own(&DESTRUCTIVE),
        threads: Threads {
            profiles: p("/cooee/profiles"),
            started: p("/cooee/thread-started"),
            attach: p("/api/v1/orchestrator-attach"),
            detach: p("/api/v1/orchestrator-detach"),
            rows: p("/api/v1/rows"),
            stream: p("/api/v1/rows/stream"),
            thread: p("/api/v1/thread/<id>/rows"),
            said: p("/api/v1/thread/<id>/said"),
        },
    }
}

/// Whether `POST /action` takes `action` from a client.
pub fn permitted(action: &str) -> bool {
    ACTIONS.contains(&action)
}

#[derive(Serialize)]
struct Plugin {
    name: String,
    version: String,
    build: &'static str,
    run: &'static str,
    needs: [&'static str; 1],
    attach: String,
    attach_token: &'static str,
    places: Vec<Place>,
    permissions: Permissions,
    threads: Threads,
}

#[derive(Serialize)]
struct Permissions {
    places: Vec<String>,
    push: bool,
    actions: Vec<String>,
    destructive: Vec<String>,
    network: [&'static str; 2],
}

/// `cooee-plugin.toml`, `just gen`'s output: the manifest in cooee's plugin format.
pub fn plugin_toml() -> anyhow::Result<String> {
    let m = manifest(ATTACH);
    let mut kinds: Vec<String> = vec![];
    for p in &m.screens {
        if !kinds.contains(&p.kind) {
            kinds.push(p.kind.clone());
        }
    }
    let plugin = Plugin {
        name: m.name,
        version: m.version,
        build: "true",
        run: "exec drover serve --addr 127.0.0.1:$PORT",
        needs: ["drover"],
        attach: m.api,
        attach_token: "~/.config/drover/cooee-token",
        places: m.screens,
        permissions: Permissions {
            places: kinds,
            push: true,
            actions: m.actions,
            destructive: m.destructive,
            network: ["api.github.com", "127.0.0.1:7777"],
        },
        threads: m.threads,
    };
    Ok(toml::to_string(&plugin)?)
}
