//! #375: the JSON shapes the page reads and posts: the one definition of each, `assets/types/api.d.ts` is made from these
//! (`just types`), API v1's (#265) too, with the core rows the board shows (`Event`, `IssueState`, `Waiting`, `hosts::Seen`, `hosts::Row`).
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

/// A lane in the Herd; #423: `grow`, its issue's growth stage (a theme's `stages`).
pub struct HerdLane {
    pub name: String,
    pub act: String,
    pub grow: String,
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

/// `/live`'s `ev`: an event's seq, time and theme moment (`serve::moment`); #370 its fleet and issue, for row moments.
pub struct Moment {
    pub seq: u64,
    pub ts: u64,
    pub k: Option<String>,
    pub f: String,
    pub n: Option<u64>,
}

/// #269: API v1's `needs`, the cards above every tab as this device sees them: the snapshot's time `snap` (0: none;
/// #450: the strip under the header shows its age once over 2 min), the open asks, each blocked lane's card, the
/// `needs-human` issues, and `choosing` (`[chat·N]`). `rw`: the board takes forms (the answer boxes); `live`: and is no
/// export; `paired`: the device may answer a lane or an issue; `pair`: the page offers Pair this phone.
#[allow(clippy::struct_excessive_bools)] // flags the cards' markup reads, one each
pub struct NeedsTab {
    pub now: u64,
    pub rw: bool,
    pub live: bool,
    pub paired: bool,
    pub pair: bool,
    pub mtime: String,
    pub snap: u64,
    pub choosing: usize,
    pub asks: Vec<NeedAsk>,
    pub blocked: Vec<Blocked>,
    pub issues: Vec<NeedIssue>,
}

/// An open ask's card: `who` (fleet · #issue · agent · age), its `kind` line, the host it was imported `from`; #412: a
/// design's PNGs by name (`/design/<fleet>/<n>/<i>` serves the `i`th); #517: the `thread` it was asked in, its `title`.
pub struct NeedAsk {
    pub ask: Ask,
    pub who: String,
    pub kind: String,
    pub from: Option<String>,
    pub pngs: Vec<String>,
    pub thread: Option<String>,
    pub title: Option<String>,
}

/// A blocked lane's card (`who`: fleet · #issue · lane; `push` 0 while a countdown runs): unpaired, its `activity`;
/// else a harness `menu`, or the pane's last lines (`tail`).
pub struct Blocked {
    pub name: String,
    pub who: String,
    pub push: bool,
    pub activity: Option<String>,
    pub menu: Option<Menu>,
    pub tail: Option<String>,
}

/// A harness menu: its title, reason and command, the `countdown` (s) before it auto-denies, the masked question and
/// options, `h` the hash of the menu the card shows (`POST /answer`).
pub struct Menu {
    pub title: String,
    pub reason: String,
    pub command: String,
    pub countdown: Option<u64>,
    pub question: String,
    pub options: Vec<String>,
    pub h: String,
}

/// #130: a `needs-human` issue's card (`pending`: one more round recorded, no lane yet): `who`, title, `why`, the last
/// verdict (`pass`: its sha, `stale` not the head; else `fail`'s finding), the PR line (`repo`: none, the fleet is
/// not configured), `mergeable` and `merge` (merge anyway is offered); #417: the verdict's screenshot `shots`.
pub struct NeedIssue {
    pub fleet: String,
    pub issue: u64,
    pub pending: bool,
    pub who: String,
    pub title: String,
    pub why: String,
    pub pass: Option<String>,
    pub stale: bool,
    pub fail: String,
    pub pr: PrLine,
    pub repo: Option<String>,
    pub mergeable: String,
    pub merge: bool,
    pub shots: Vec<String>,
}

/// #269: API v1's `lanes`, the Lanes tab as this device sees it: state sections, repo groups inside (`error`: no
/// fleet.toml, `failed`: herdr unread). `rw`, `live`, `paired`, `pair` as `NeedsTab`'s.
#[allow(clippy::struct_excessive_bools)] // flags the rows' markup reads, one each
pub struct LanesTab {
    pub now: u64,
    pub rw: bool,
    pub live: bool,
    pub paired: bool,
    pub pair: bool,
    pub error: Option<String>,
    pub failed: bool,
    pub sections: Vec<LaneSection>,
    /// #630: each lane's workspace root and unsaved work.
    pub unsaved: Vec<LaneUnsaved>,
}

/// #630: lane `name`'s workspace root, and its worktree's uncommitted files and unpushed commits.
pub struct LaneUnsaved {
    pub name: String,
    pub root: Option<String>,
    pub dirty: Option<u64>,
    pub unpushed: Option<u64>,
}

/// A state section (`tone`) and its repo groups.
pub struct LaneSection {
    pub name: String,
    pub tone: String,
    pub repos: Vec<LaneRepo>,
}

pub struct LaneRepo {
    pub name: String,
    pub lanes: Vec<LaneRow>,
}

/// A Lanes row: its avatar (`avatar`, herdr's `state` with `act`, #423 `grow`, `atone` and `coat`), name, `state` in
/// `tone` for `ago`, the `why` line (`why_tone`), the activity (`activity`, or the `now` parts), `gone` (no pane), the
/// conductor `thread` it opens; the hidden card: `head`, `shown`, `facts`, the PR line (`issue` and `pr`, `repo` none when its
/// fleet is not configured), `dead` and the blocked card.
pub struct LaneRow {
    pub name: String,
    pub role: String,
    pub state: String,
    pub act: String,
    pub grow: String,
    pub atone: String,
    pub coat: String,
    pub tone: String,
    pub ago: String,
    pub why: Option<String>,
    pub why_tone: String,
    pub activity: String,
    pub now: Option<LaneNow>,
    pub gone: bool,
    pub thread: Option<String>,
    pub head: String,
    pub shown: String,
    pub facts: String,
    pub issue: Option<u64>,
    pub pr: Option<PrLine>,
    pub repo: Option<String>,
    pub dead: bool,
    pub blocked: Option<Blocked>,
}

/// #276: a `now:` line: the spinner word `w`, the action `n`, the elapsed time `t`, the full text as `title`.
pub struct LaneNow {
    pub title: String,
    pub w: Option<String>,
    pub n: Option<String>,
    pub t: Option<String>,
}

/// #269: API v1's `chat`, the [chat] tab (#386): a card per repo (`all` first) with its threads, then every thread's
/// view (`error`: fleet.toml unread). `rw`: a paired device on the live board writes.
pub struct ChatTab {
    pub now: u64,
    pub rw: bool,
    pub error: Option<String>,
    pub repos: Vec<ChatRepo>,
    pub threads: Vec<ThreadView>,
}

/// A repo's card in colour `k`: its threads newest first, the archived ones apart; #404: its empty thread's
/// `harness` chip.
pub struct ChatRepo {
    pub repo: String,
    pub k: String,
    pub rows: Vec<ThreadRow>,
    pub archived: Vec<ThreadRow>,
    pub harness: Chip,
}

/// #404: a composer chip: `name`'s current value `cur` (none: the default) and its choices `all`.
pub struct Chip {
    pub name: String,
    pub cur: Option<String>,
    pub all: Vec<String>,
}

/// A thread's row: unread, title, age, its `state` (choose, working, asleep, …), the threads it links to, the last line;
/// #460: the ids of the config proposals `waiting` for the owner.
pub struct ThreadRow {
    pub id: String,
    pub title: String,
    pub age: String,
    pub unread: bool,
    pub state: String,
    pub waiting: Vec<String>,
    pub links: Vec<Link>,
    pub line: String,
}

/// #413: a linked thread's chip: `from t5`, `continued in t6`.
pub struct Link {
    pub w: String,
    pub t: String,
}

/// One thread: its bar (`ds`: fleet · harness · model · state), its last messages, the live line's `state`; #407:
/// working, the turn's start `since`, and the `queued` messages above the box; #460: the config proposals `waiting`; #404:
/// its model and effort `chips`.
pub struct ThreadView {
    pub id: String,
    pub title: String,
    pub ds: String,
    pub state: String,
    pub waiting: Vec<String>,
    pub since: Option<u64>,
    pub queued: Vec<QueuedMsg>,
    pub chips: Vec<Chip>,
    pub page: ThreadPage,
}

/// #407: a message queued for the turn's end, by its id `q`.
pub struct QueuedMsg {
    pub q: u64,
    pub text: String,
}

/// #408: a page of a thread's messages; `older`: the seq to load the messages before, else the `started` line's time
/// and conductor folder `dir`; #618: the thread's `id`, and whether it is `archived` (opened read-only, [restore]).
pub struct ThreadPage {
    pub id: String,
    pub archived: bool,
    pub msgs: Vec<Msg>,
    pub older: Option<u64>,
    pub started: u64,
    pub dir: String,
}

/// A thread's message (its event's `seq` and `ts`) by `kind`, `text` its words: `note` (a system line: `retry`,
/// `signin` on `host`; #541: a conductor's answer, its `ask` and `open` while the owner may [change] it), `handoff` (the title), `msg` (`from` who wrote it, its `files`), `cmd`, `ask` (the question;
/// `answer` once decided, else `open`), `config` (the diff, `from` the proposal's id; #460: `open` unless stale), `secret` (why, `from` its name),
/// `resumed` (the session), `sleep`, #407 `fold` (a finished turn's run of commands, `cmds`); `link` the thread it
/// links to; #669 `run` (a run card, `from` its id).
pub struct Msg {
    pub seq: u64,
    pub ts: u64,
    pub kind: String,
    pub text: String,
    pub link: Option<Link>,
    pub from: String,
    pub files: Vec<FileRef>,
    pub retry: bool,
    pub signin: Option<String>,
    pub host: String,
    pub ask: Option<Ask>,
    pub routine: bool,
    pub answer: Option<Answered>,
    pub open: bool,
    pub cmds: Vec<String>,
    pub run: Option<RunCard>,
}

/// #669: a run card as its thread shows it: what it runs, why, what it touches, where, its sha; open until `exp`, then
/// `state` (`open`, `running`, `approved`, `denied`, `expired`) with `by`, the exit `code` and the output `tail`.
pub struct RunCard {
    pub id: String,
    pub cmd: String,
    pub why: String,
    pub blast: String,
    pub host: String,
    pub dir: String,
    pub sha: String,
    pub exp: u64,
    pub state: String,
    pub by: Option<String>,
    pub code: Option<i64>,
    pub tail: String,
}

pub struct FileRef {
    pub name: String,
    pub size: u64,
}

/// A decided ask: the option `n` chosen (none: an "other" answer), its text, by whom, when.
pub struct Answered {
    pub n: Option<u64>,
    pub choice: String,
    pub by: Option<String>,
    pub ts: u64,
}

/// #365: one open issue of the Queue tab's backlog (`data-bl`, by number), as `gh::backlog` reads it.
pub struct BacklogIssue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub created: String,
    pub updated: String,
    pub labels: Vec<String>,
    pub ac: bool,
    pub pr: bool,
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

/// #265: API v1's `state`, the board in one read: the newest event's `seq`, the step's snapshot age (none saved: null) and the
/// `mtime` a POST sends back (STALE when fleet.toml changed since). #266: the server's clock `now` and UTC offset `tz` (s),
/// the Events tab's time groups.
pub struct State {
    pub seq: u64,
    pub now: u64,
    pub tz: i64,
    pub age: Option<u64>,
    pub mtime: String,
    pub theme: String,
    pub fleets: Vec<Fleet>,
    pub queue: Vec<Queued>,
    pub lanes: Vec<Lane>,
    pub asks: Vec<Ask>,
    pub hosts: Vec<Row>,
    pub services: Vec<Row>,
    pub alerts: Vec<Row>,
}

pub struct Fleet {
    pub name: String,
    pub repo: String,
    pub color: String,
    pub paused: bool,
}

pub struct Queued {
    pub fleet: String,
    pub issue: u64,
    pub title: Option<String>,
    pub state: String,
    pub blocked_by: Option<u64>,
    /// #667: its blockers in another fleet or repo, as `owner/repo#N`.
    pub blocked_on: Vec<String>,
}

/// `state` is herdr's (or `dead`, `parked`); `shown` and `tone` (g, a, r, m) are the Lanes row's words.
pub struct Lane {
    pub name: String,
    pub fleet: Option<String>,
    pub issue: Option<u64>,
    pub title: Option<String>,
    pub role: String,
    pub harness: String,
    pub model: Option<String>,
    pub state: String,
    pub shown: String,
    pub tone: String,
    pub since: u64,
}

pub struct Ask {
    pub id: String,
    pub fleet: String,
    pub issue: Option<u64>,
    pub question: String,
    pub options: Vec<String>,
    pub recommend: Option<u64>,
    pub ts: u64,
}

/// A Host tab row: HOSTS' (`detail`: its state in words), SERVICES' and ALERTS' (push); `fix` clears a red or amber one.
pub struct Row {
    pub name: String,
    pub tone: String,
    pub detail: String,
    pub fix: String,
}

/// #267: API v1's `host`, the Host tab at `now`: HOSTS (`hosts`), FLEET DISK (`disk`, `orphans`, `leftovers`, `spawns`),
/// SERVICES (`services`, `theme`: `detail` its error, `accounts`, `deepseek`), FLEETS and ALERTS (`push`, and on a live
/// board its `key`, `subs` and paired `devices`). `rw`: the board takes forms; `live`: and is no export (an export's
/// forms are inert); `mtime` the forms send back.
pub struct HostTab {
    pub now: u64,
    pub rw: bool,
    pub live: bool,
    pub mtime: String,
    pub hosts: Vec<HostCard>,
    pub disk: Option<Disk>,
    pub orphans: usize,
    pub leftovers: Option<Leftovers>,
    pub spawns: Option<Row>,
    pub services: Vec<Row>,
    pub theme: Row,
    pub accounts: Vec<Account>,
    pub deepseek: Option<Balance>,
    pub fleets: Vec<FleetRow>,
    pub push: Row,
    pub key: String,
    pub subs: Vec<Sub>,
    pub devices: Vec<Paired>,
    pub notify: Vec<Notifier>,
}

/// #440: a global `[[notify]]` in words (System → Notifications): `web push` or `command`, and its events.
pub struct Notifier {
    pub kind: String,
    pub on: String,
}

/// #234: a host's card: its dot and state `words` in `tone`, the lanes' 24 h series `n`, its rows, `to` the [off]/[on]
/// its head offers (with the `lanes` running on it), and the logins its [sign in ▾] lists.
pub struct HostCard {
    pub name: String,
    pub main: bool,
    pub dot: String,
    pub tone: String,
    pub words: String,
    pub n: String,
    pub rows: Vec<Cell>,
    pub to: Option<String>,
    pub lanes: usize,
    pub setups: Vec<String>,
}

/// #234: a host card's row, status `t`: a gauge's `bar` then its value `b` (bold, in `bt`), `text`, the limit `of` and
/// muted `notes`. `key`: a metric with history: its 24 h sparkline `s` (memory: pressure's `p`), its limit `l` and, in an
/// export, its chart `x` inline.
pub struct Cell {
    pub k: String,
    pub t: String,
    pub key: String,
    pub bar: Option<Bar>,
    pub b: Option<String>,
    pub bt: String,
    pub text: String,
    pub of: String,
    pub notes: Vec<String>,
    pub s: String,
    pub p: String,
    pub l: Option<f64>,
    pub x: String,
}

/// #141: a 12-cell text gauge: its status `t`, `n` cells full, and how far past the limit (`pct`, else empty).
pub struct Bar {
    pub t: String,
    pub n: usize,
    pub pct: String,
}

/// #164: the worktree total (`gb`, `?` unread) against the budget, and how long ago it was walked.
pub struct Disk {
    pub bar: Bar,
    pub gb: String,
    pub budget: f64,
    pub ago: Option<String>,
}

/// #186: processes still running in closed lanes' worktrees, and the memory they hold.
pub struct Leftovers {
    pub n: usize,
    pub mb: String,
}

/// An account: `capped`, its cap line in `tone`, its usage `meters` and `pace` (or the `hint` why none), its lane hours.
pub struct Account {
    pub name: String,
    pub capped: bool,
    pub tone: String,
    pub cap: String,
    pub meters: Vec<Meter>,
    pub pace: Option<String>,
    pub hint: Option<String>,
    pub hours: String,
}

pub struct Meter {
    pub label: String,
    pub pct: f64,
}

/// The `DeepSeek` balance (currency and amount) and whether it is `low`, else the `hint` why none was read.
pub struct Balance {
    pub balance: Option<String>,
    pub low: bool,
    pub hint: String,
}

/// #141: a fleet's last `driven` result in words (`word` in `tone`, the plain rest `text`), its age and `raw` line.
pub struct FleetRow {
    pub name: String,
    pub tone: String,
    pub word: String,
    pub text: String,
    pub ago: String,
    pub raw: Option<String>,
}

/// #18: a webpush subscription, removable by `id`.
pub struct Sub {
    pub id: String,
    pub label: String,
    pub created: u64,
}

/// #59: a paired device, removable by `hash`.
pub struct Paired {
    pub hash: String,
    pub label: String,
    pub created: u64,
    pub used: u64,
}

/// #268: API v1's `queue`, the Queue tab at `now`: a card per fleet, the ones needing you first (`error`: fleet.toml
/// unread). `rw`: the board takes forms; `live`: and is no export (an export's forms are inert); `mtime` they send back.
pub struct QueueTab {
    pub now: u64,
    pub rw: bool,
    pub live: bool,
    pub mtime: String,
    pub error: Option<String>,
    pub fleets: Vec<QueueCard>,
}

/// #162: a fleet's card in its colour `k`: its serial groups' names and paths, `need` rows needing you, a paused one's
/// line (`paused`: since and lanes), else its drive line and red `notes`; its rows, `done` today (the newest 5 of
/// `done_n`), the backlog and the [config] sheet.
pub struct QueueCard {
    pub name: String,
    pub k: String,
    pub serial: Vec<String>,
    pub paths: Vec<String>,
    pub need: usize,
    pub paused: Option<String>,
    pub drive: Drive,
    pub notes: Vec<String>,
    pub rows: Vec<QueueRow>,
    pub done: Vec<DoneRow>,
    pub done_n: usize,
    pub backlog: Option<Backlog>,
    pub config: ConfigSheet,
    pub marks: BTreeMap<String, Marks>,
}

/// #576: a queued row's marks by its issue (`QueueCard.marks`): `design`, `verify` (`on`, `off`, none: by `ui_paths`),
/// `repro`, its serial groups and `needs` tags, and the stage `note` its row shows (`verify(n)`, `verify skipped: …`).
pub struct Marks {
    pub design: bool,
    pub verify: Option<String>,
    pub repro: bool,
    pub serial: Vec<String>,
    pub needs: Vec<String>,
    pub note: Option<String>,
}

/// #166: a fleet's drive line in `tone`: its words, a tap's `meta` lines then the last pass's actions (`pass`: one
/// ran, so none is `nothing was due`).
pub struct Drive {
    pub tone: String,
    pub line: String,
    pub meta: Vec<String>,
    pub pass: bool,
    pub acts: Vec<DriveAct>,
}

/// A pass's action: its issue (`#N`) or step, in words, its raw `code` on tap.
pub struct DriveAct {
    pub who: String,
    pub words: String,
    pub code: String,
}

/// A Queue row: `#issue` in its shown `state` (`tone`) for `age`, counting from `t`; its row sprite's `act`, #423 `grow`
/// (the growth stage) and `coat`; `queued` (with `check`: from the backlog), its serial `groups`, `blocked_by`; titles; the open card: the
/// pipeline `stage`, the current `lane` (`live`: herdr lists it), the PR line, `dead`, red `notes` (a land note, an
/// answer not delivered), the decision taken by default
/// (and its ask to change it), #517 its `open` ask (answerable here), [▲]/[▼] (`up`, `down`: the rows to move before or after) and the lanes list.
#[allow(clippy::struct_excessive_bools)] // flags the row's markup reads, one each
pub struct QueueRow {
    pub issue: u64,
    pub state: String,
    pub tone: String,
    pub age: String,
    pub t: u64,
    pub act: String,
    pub grow: String,
    pub coat: String,
    pub queued: bool,
    pub check: bool,
    pub groups: Vec<String>,
    pub blocked_by: Option<u64>,
    pub title: Option<String>,
    pub stage: Vec<Stage>,
    pub lane: String,
    pub live: bool,
    pub pr: PrLine,
    pub dead: bool,
    pub notes: Vec<String>,
    pub decided: Option<String>,
    pub ask: Option<Ask>,
    pub open: Option<NeedAsk>,
    pub mv: bool,
    pub up: Option<u64>,
    pub down: Option<u64>,
    pub lanes: Vec<QueueLane>,
}

/// #173: a pipeline chip: its kind's glyph `g` and class `k`, `at` before (`-1`), on (`0`) or after (`1`) where the
/// issue is, a check's PASS at the PR head `ok`.
pub struct Stage {
    pub name: String,
    pub k: String,
    pub g: String,
    pub at: i8,
    pub ok: bool,
}

/// #169: `[issue] [PR #M] [checks]` and CI (`green`, `failed`, else pending; none: no PR read yet), then the last
/// verdict in `vtone` and its round, `form` the form-only rounds used.
pub struct PrLine {
    pub pr: Option<u64>,
    pub run: Option<String>,
    pub ci: Option<String>,
    pub verdict: Option<String>,
    pub vtone: String,
    pub rounds: u32,
    pub form: Option<String>,
}

/// #205: a lane the issue had, newest first: herdr's `state` (`gone`) in `tone`, its spawn's age; `link`: `live`
/// opens its Lanes sheet, `gone` its stored transcript, empty none.
pub struct QueueLane {
    pub name: String,
    pub state: String,
    pub tone: String,
    pub ago: String,
    pub link: String,
}

/// A row of `done today`: its land note (else state), age and merged PR.
pub struct DoneRow {
    pub issue: u64,
    pub note: String,
    pub age: String,
    pub pr: Option<String>,
}

/// #365: the open issues not queued and with no lane, by number, as last read `ago`.
pub struct Backlog {
    pub issues: BTreeMap<String, BacklogIssue>,
    pub ago: String,
}

/// #162: a fleet's [config] sheet: the config file (`path`, `file`), GENERAL, the Pipeline group's data (`pipe`),
/// LIMITS, ROLES (with `rw`, pickers from `models`, `harnesses` and their efforts), the Conductor, ALERTS.
pub struct ConfigSheet {
    pub path: String,
    pub file: String,
    pub repo: String,
    pub local: String,
    pub integration: String,
    pub effort: Option<String>,
    pub color_set: bool,
    pub env: Vec<String>,
    pub node_modules: bool,
    pub builtin: bool,
    pub review_rounds: u32,
    pub update_rounds: u32,
    pub stall_minutes: u64,
    pub ask_minutes: u64,
    pub max_impl: Option<usize>,
    pub max_rev: Option<usize>,
    pub line_cap: Option<i64>,
    pub serial: Vec<String>,
    pub deny: Vec<String>,
    pub required: Vec<String>,
    pub merge_label: Option<String>,
    pub notify: Vec<String>,
    pub global: Vec<String>,
    pub pipe: Value,
    pub models: Vec<String>,
    pub harnesses: Vec<Harness>,
    pub roles: Vec<RoleRow>,
    pub conductor: Option<Conductor>,
}

pub struct Harness {
    pub name: String,
    pub efforts: Vec<String>,
}

/// A role as set: harness (taking `efforts`), model and effort (unset: what the lane gets, `model_default`,
/// `effort_default`).
pub struct RoleRow {
    pub name: String,
    pub profile: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub efforts: Vec<String>,
    pub model_default: String,
    pub effort_default: String,
    pub accounts: Vec<String>,
    pub mcp: Vec<String>,
}

/// #401: the fleet's conductor (`default`: impl's), its choices: models the fleet's roles name.
pub struct Conductor {
    pub default: bool,
    pub profile: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub efforts: Vec<String>,
    pub models: Vec<String>,
}

/// #259's contract: the events after `since` (#266: up to `until`) in `list`; over 500, none and `refresh` (read `state` again).
pub struct Events {
    pub seq: u64,
    pub refresh: bool,
    pub list: Vec<Ev>,
}

/// One logged event; `k` is the theme moment it plays (merged, fail, needs-you). #266: as the Events tab says it
/// (`serve::words`): `glyph`, `tone`, `text`, shown under [important] (`imp`), its row sprite's `act` and `coat` (its
/// agent's), `plain` (a fold's line), and its issue's `pr` (else the branch's checks `run`).
pub struct Ev {
    pub seq: u64,
    pub ts: u64,
    pub fleet: String,
    pub issue: Option<u64>,
    #[serde(rename = "type")]
    pub kind: String,
    pub data: serde_json::Map<String, Value>,
    pub k: Option<String>,
    pub glyph: String,
    pub tone: String,
    pub text: String,
    pub imp: bool,
    pub act: String,
    pub coat: String,
    pub plain: String,
    pub pr: Option<u64>,
    pub run: Option<String>,
}

/// #576: API v1's `issue/<fleet>/<n>`: its recorded verdicts, newest first.
pub struct IssueView {
    pub fleet: String,
    pub issue: u64,
    pub verdicts: Vec<Verdict>,
}

/// A verdict: its `role` (the stage: `rev`, `verify`, or the gate's), round, sha, `verdict` (PASS or FAIL), `findings`
/// (verdict.md after its first line); the newest one's screenshot `shots` by name (`/shot/<fleet>/<n>/<i>` serves the
/// `i`th); a `--repro` PASS's `Repro:` block.
pub struct Verdict {
    pub role: String,
    pub round: u64,
    pub sha: String,
    pub verdict: String,
    pub ts: u64,
    pub findings: String,
    pub shots: Vec<String>,
    pub repro: Option<Repro>,
}

/// #420: a `Repro:` block: `run` (the commands and both outputs) or `verify` (the quoted reason), its lines as `text`.
pub struct Repro {
    pub kind: String,
    pub text: String,
}

/// #576: API v1's `lessons`: per fleet, its `lessons` setting (`weekly`, `off`), the `last` lesson run and when `tick`
/// runs the `next` (none: `off`).
pub struct Lessons {
    pub fleets: Vec<LessonFleet>,
}

pub struct LessonFleet {
    pub fleet: String,
    pub lessons: String,
    pub last: Option<LessonRun>,
    pub next: Option<u64>,
}

/// A lesson issue: its lane's spawn time `ts` and the issue's state now.
pub struct LessonRun {
    pub issue: u64,
    pub ts: u64,
    pub state: String,
}

/// #576: `lessons/<fleet>/digest?days=N`: `herdsman lesson digest`'s text.
pub struct Digest {
    pub fleet: String,
    pub days: u64,
    pub text: String,
}

/// #555: API v1's `setup/<pane>` (the old board's `/lane?setup=`): the setup pane's masked `lines` and every
/// `https://` URL in it (a wrapped one joined), or `closed`.
pub struct SetupScreen {
    pub lines: Vec<String>,
    pub urls: Vec<String>,
    pub closed: bool,
}

/// #555: API v1's `roles?fleet=`: the fleet's [config] sheet (its roles, pickers and conductor) and `[chat]`'s
/// new-thread defaults (`POST chat` writes them).
pub struct Roles {
    pub fleet: String,
    pub config: ConfigSheet,
    pub chat: ChatDefaults,
}

/// #404: `[chat]`'s harness, model and effort (none: the conductor role's), and the `harnesses` it may name.
pub struct ChatDefaults {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub harnesses: Vec<Harness>,
}

/// A POST's answer: `code` is OK, a `REFUSED` code, or ERROR; `message` the line the board toasts.
pub struct Reply {
    pub ok: bool,
    pub code: String,
    pub message: String,
}

/// #633: `POST /api/v1/conductor-attach`'s answer (a refusal is a `Reply`): what a cooee conductor profile needs.
pub struct ConductorAttach {
    pub ok: bool,
    pub code: String,
    pub message: String,
    /// `roles/conductor.md`, the repo and the owner's conductor instructions.
    pub brief: String,
    /// The synced conductor Claude settings (`PreToolUse`: `herdsman hook`).
    pub settings: String,
    pub env: AttachEnv,
    /// #565's ACP guard holds; else `why` not.
    pub guarded: bool,
    pub why: String,
}

pub struct AttachEnv {
    #[serde(rename = "HERDSMAN_LANE")]
    pub lane: String,
    #[serde(rename = "HERDSMAN_THREAD")]
    pub thread: String,
    #[serde(rename = "HERDSMAN_PORT")]
    pub port: String,
    /// #683: the role's `folder:` account, the thread's own Claude login (none: no folder account).
    #[serde(rename = "CLAUDE_CONFIG_DIR")]
    pub config_dir: Option<String>,
}

}

/// A form body (`application/x-www-form-urlencoded`, decoded) as one of the action bodies above.
pub fn body<T: for<'de> Deserialize<'de> + Default>(f: &BTreeMap<String, String>) -> T {
    serde_json::from_value(serde_json::json!(f)).unwrap_or_default()
}
