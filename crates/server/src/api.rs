//! #375: the JSON shapes the page reads and posts: the one definition of each, `assets/types/api.d.ts` is made from these
//! (`just types`), with the core rows the board shows (`Event`, `IssueState`, `Waiting`, `hosts::Seen`, `hosts::Row`).
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Every shape: serde's JSON both ways (a missing field is its default), and `ts-rs`'s type for it under `--features ts`.
macro_rules! shapes {
    ($($s:item)*) => {$(
        #[derive(Serialize, Deserialize, Default)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
        #[serde(default)]
        $s
    )*};
}

shapes! {
/// `/herd.json` (Herd mode, docs/serve.md).
pub struct Herd {
    pub fleets: Vec<HerdFleet>,
    pub others: Vec<HerdLane>,
    pub host: HerdHost,
}

pub struct HerdFleet {
    pub name: String,
    pub color: String,
    pub lanes: Vec<HerdLane>,
    pub queue: Vec<HerdQueued>,
    pub needs: Vec<u64>,
}

pub struct HerdLane {
    pub name: String,
    pub act: String,
    pub issue: Option<u64>,
    pub title: Option<String>,
    pub role: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub account: Option<String>,
    pub host: Option<String>,
    pub state: String,
    pub shown: String,
    pub tone: String,
    pub age: String,
    pub coat: String,
    pub gone: bool,
}

pub struct HerdQueued {
    pub issue: u64,
    pub title: Option<String>,
    pub blocked_by: Option<u64>,
    pub coat: String,
}

pub struct HerdHost {
    pub load: Option<f64>,
    pub limit: f64,
    pub pressure: Option<String>,
    pub swapouts: Option<f64>,
    pub swap_limit: f64,
    pub gate: Option<String>,
}

/// `/lane?agent=`: the sheet's chat. A lane with no pane sends no `said`, `notices`, `acts`, `pane` or `status`.
pub struct Chat {
    pub said: Option<Vec<Value>>,
    pub older: bool,
    pub notices: Vec<Notice>,
    pub harness: String,
    pub acts: Vec<String>,
    pub pane: Option<String>,
    pub lines: Vec<String>,
    pub live: usize,
    pub status: Option<String>,
    pub before: usize,
    pub more: bool,
}

pub struct Notice {
    pub at: u64,
    pub text: String,
}

/// `/live`'s `ev`: an event's seq, time and theme moment (`serve::moment`).
pub struct Moment {
    pub seq: u64,
    pub ts: u64,
    pub k: Option<String>,
}

/// `/live`'s `frag`: a fragment's new HTML.
pub struct Frag {
    pub id: String,
    pub html: String,
}

/// #351: `/live`'s `patch`: a `frag` as length `n`, common prefix `p` and the changed pieces by index.
pub struct Patch {
    pub id: String,
    pub n: usize,
    pub p: usize,
    pub set: BTreeMap<String, String>,
}

/// `POST /send`: a reply to lane `agent`, or to the setup pane `setup`.
pub struct Send {
    pub agent: String,
    pub setup: Option<String>,
    pub text: String,
}

/// `POST /answer`: menu option `choice` of the menu hashed `h`.
pub struct Answer {
    pub agent: String,
    pub choice: String,
    pub h: String,
}

/// `POST /act`: stop | kill | clean | close; `confirm` `1` does what a dry run showed.
pub struct Act {
    pub agent: String,
    pub op: String,
    pub confirm: String,
}

/// `POST /setup`: start the setup pane `name` on `host`.
pub struct Setup {
    pub host: String,
    pub name: String,
}
}

/// A form body (`application/x-www-form-urlencoded`, decoded) as one of the action bodies above.
pub fn body<T: for<'de> Deserialize<'de> + Default>(f: &BTreeMap<String, String>) -> T {
    serde_json::to_value(f)
        .and_then(serde_json::from_value)
        .unwrap_or_default()
}
