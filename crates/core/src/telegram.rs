//! #387: the optional Telegram bridge, no Hermes: herdsman calls the Bot API itself (curl, its token from the keychain
//! item `herdsman/all/TELEGRAM_BOT_TOKEN`, on stdin only). One forum topic is one thread; a choice is inline buttons;
//! images go both ways. `serve` long-polls it (`tick`); the board stays the primary UI.
use crate::config::Config;
use crate::events::{emit, read, Event};
use crate::runner::{argv, Runner};
use crate::usage::q;
use crate::{conductor, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// The bridge's cursor: the next update id and how many events were already relayed.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Cursor {
    offset: i64,
    sent: Option<usize>,
}

struct Bot<'a> {
    r: &'a dyn Runner,
    api: String,
    token: String,
    chat: i64,
}

impl Bot<'_> {
    /// Bot API `method` with curl config `body` lines; its `result`.
    fn call(&self, method: &str, body: &str) -> Result<Value> {
        let conf = format!("url = \"{}/bot{}/{method}\"\n{body}", q(&self.api), q(&self.token));
        let o = self.r.run_in("curl", &argv(&["-sS", "-m", "70", "-K", "-"]), &conf)?;
        let v: Value = serde_json::from_str(&o.stdout).with_context(|| format!("telegram {method}: no JSON"))?;
        if v["ok"] != true {
            bail!("telegram {method}: {}", v["description"]);
        }
        Ok(v["result"].clone())
    }

    fn json(&self, method: &str, v: &Value) -> Result<Value> {
        let body = format!(
            "header = \"Content-Type: application/json\"\ndata = \"{}\"\n",
            q(&v.to_string())
        );
        self.call(method, &body)
    }

    /// Post `text` (with `extra` fields) to `topic`.
    fn say(&self, topic: i64, text: &str, extra: &Value) -> Result<Value> {
        let mut v = json!({"chat_id": self.chat, "message_thread_id": topic, "text": text});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().cloned().unwrap_or_default());
        self.json("sendMessage", &v)
    }
}

/// Thread `id`'s topic.
fn topic_of(events: &[Event], id: &str) -> Option<i64> {
    let m = events
        .iter()
        .rfind(|e| e.kind == "telegram-topic" && e.str("thread") == Some(id));
    m.and_then(|e| e.data["topic"].as_i64())
}

/// Topic `topic`'s thread.
fn thread_of(events: &[Event], topic: i64) -> Option<String> {
    let m = events
        .iter()
        .rfind(|e| e.kind == "telegram-topic" && e.data["topic"] == topic);
    m.and_then(|e| e.str("thread")).map(String::from)
}

/// One pass: the updates since the cursor (waiting up to `wait` s for one), then the new events to their topics.
pub fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner, wait: u64) -> Result<()> {
    let Some(chat) = cfg.telegram.chat else {
        return Ok(());
    };
    let api = cfg
        .telegram
        .api
        .clone()
        .unwrap_or_else(|| "https://api.telegram.org".into());
    let bot = Bot {
        r,
        api,
        token: crate::secret::get(r, "all", "TELEGRAM_BOT_TOKEN")?,
        chat,
    };
    let path = ctx.state.with_file_name("telegram.json");
    let mut cur: Cursor = crate::usage::load(&path).unwrap_or_default();
    let get = json!({"offset": cur.offset, "timeout": wait, "allowed_updates": ["message", "callback_query"]});
    for u in bot.json("getUpdates", &get)?.as_array().into_iter().flatten() {
        cur.offset = cur.offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
        let owner = |from: &Value| cfg.telegram.owner.is_none_or(|o| from["id"] == o);
        if let Some(m) = u
            .get("message")
            .filter(|m| m["chat"]["id"] == chat && owner(&m["from"]))
        {
            inbound(ctx, cfg, &bot, m).unwrap_or_else(|e| eprintln!("herdsman telegram: {e:#}"));
        }
        if let Some(c) = u.get("callback_query").filter(|c| owner(&c["from"])) {
            let (f, id, n) = {
                let mut p = c["data"].as_str().unwrap_or_default().splitn(3, ':');
                (
                    p.next().unwrap_or_default(),
                    p.next().unwrap_or_default(),
                    p.next().unwrap_or_default(),
                )
            };
            let by = ("owner", Some("telegram"));
            let res = crate::answer(ctx, cfg, r, &mut vec![], (f, id), n, by);
            let text = res.map_or_else(|e| format!("{e:#}"), |()| format!("answered {id}"));
            bot.json(
                "answerCallbackQuery",
                &json!({"callback_query_id": c["id"], "text": text}),
            )?;
        }
    }
    let events = read(&ctx.state)?;
    for e in &events[cur.sent.unwrap_or(events.len()).min(events.len())..] {
        outbound(ctx, cfg, &bot, &events, e)?;
    }
    cur.sent = Some(read(&ctx.state)?.len());
    crate::devices::save(&path, &cur)
}

/// An owner message in a topic: its thread (a new topic starts one under `all`, or under the repo its name starts
/// with), then the text and any image typed into the conductor.
fn inbound(ctx: &Ctx, cfg: &Config, bot: &Bot, m: &Value) -> Result<()> {
    let Some(topic) = m["message_thread_id"].as_i64() else {
        return Ok(()); // the General topic: no thread
    };
    let events = read(&ctx.state)?;
    let id = if let Some(id) = thread_of(&events, topic) {
        id
    } else {
        let made = m
            .get("forum_topic_created")
            .or(m["reply_to_message"].get("forum_topic_created"));
        let name = made
            .and_then(|t| t["name"].as_str())
            .map_or(format!("topic {topic}"), String::from);
        let repo = cfg
            .fleet
            .keys()
            .find(|f| name.starts_with(f.as_str()))
            .map_or("all", String::as_str);
        let mut out = vec![];
        conductor::new(ctx, cfg, &mut out, repo, &name)?;
        let id = String::from_utf8_lossy(&out).trim().to_string();
        let d = json!({"thread": id, "topic": topic});
        emit(&ctx.state, cfg, Event::new(ctx.now, repo, None, "telegram-topic", d))?;
        id
    };
    let mut files = vec![];
    // The largest size of a photo, or an image sent as a file.
    let doc = m
        .get("document")
        .filter(|d| d["mime_type"].as_str().is_some_and(|t| t.starts_with("image/")));
    if let Some(f) = m["photo"].as_array().and_then(|p| p.last()).or(doc) {
        let path = bot.json("getFile", &json!({"file_id": f["file_id"]}))?["file_path"].clone();
        let path = path.as_str().context("telegram getFile: no file_path")?;
        let name = path.rsplit('/').next().unwrap_or("image.jpg").to_string();
        let dir = conductor::dir(ctx, cfg, &id).join("uploads");
        std::fs::create_dir_all(&dir)?;
        let to = dir.join(format!("{}-{name}", ctx.now));
        let url = format!(
            "url = \"{}/file/bot{}/{path}\"\noutput = \"{}\"\n",
            q(&bot.api),
            q(&bot.token),
            q(&to.to_string_lossy())
        );
        let o = bot.r.run_in("curl", &argv(&["-sSf", "-m", "70", "-K", "-"]), &url)?;
        if o.code != 0 {
            bail!("telegram: cannot download {name}");
        }
        files.push(conductor::file(ctx, cfg, &id, &to)?);
    }
    let text = m["text"].as_str().or(m["caption"].as_str()).unwrap_or_default();
    let text = if text.trim().is_empty() {
        files.first().and_then(|f| f["name"].as_str()).unwrap_or_default()
    } else {
        text
    };
    if !text.is_empty() {
        conductor::say(ctx, cfg, bot.r, (&id, text, &files), ("owner", false))?;
    }
    Ok(())
}

/// A new thread gets its topic; the conductor's and system lines, their images and the asks (as inline buttons) go
/// to it. The owner's own messages stay where they were written.
fn outbound(ctx: &Ctx, cfg: &Config, bot: &Bot, events: &[Event], e: &Event) -> Result<()> {
    let Some(id) = e.str("thread") else { return Ok(()) };
    if e.kind == "thread-new" && topic_of(events, id).is_none() {
        let t = bot.json(
            "createForumTopic",
            &json!({"chat_id": bot.chat, "name": e.str("title")}),
        )?;
        let d = json!({"thread": id, "topic": t["message_thread_id"]});
        return emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &e.fleet, None, "telegram-topic", d),
        );
    }
    let Some(topic) = topic_of(events, id) else {
        return Ok(());
    };
    match (e.kind.as_str(), e.str("from")) {
        ("thread-msg", Some(from @ ("conductor" | "system"))) => {
            let text = e.str("text").unwrap_or_default();
            bot.say(
                topic,
                &[format!("⚙ {text}"), text.into()][usize::from(from == "conductor")],
                &json!({}),
            )?;
            for f in e.data["files"].as_array().into_iter().flatten() {
                let name = f["name"].as_str().unwrap_or_default();
                let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
                let (method, field) = if ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.as_str()) {
                    ("sendPhoto", "photo")
                } else {
                    ("sendDocument", "document")
                };
                let path = f["path"].as_str().unwrap_or_default();
                let form = format!(
                    "form-string = \"chat_id={}\"\nform-string = \"message_thread_id={topic}\"\nform = \"{field}=@{}\"\n",
                    bot.chat,
                    q(path)
                );
                bot.call(method, &form)?;
            }
        }
        ("needs-decision", _) => {
            let rec = e.data["recommend"].as_u64();
            let opts = e.data["options"].as_array().into_iter().flatten().zip(1u64..);
            let rows: Vec<Value> = opts
                .map(|(o, n)| {
                    let mark = if Some(n) == rec { " (recommended)" } else { "" };
                    let data = format!("{}:{}:{n}", e.fleet, e.str("id").unwrap_or_default());
                    json!([{"text": format!("{n}. {}{mark}", o.as_str().unwrap_or_default()), "callback_data": data}])
                })
                .collect();
            let q = format!(
                "{} asks: {}",
                e.str("id").unwrap_or_default(),
                e.str("question").unwrap_or_default()
            );
            bot.say(topic, &q, &json!({"reply_markup": {"inline_keyboard": rows}}))?;
        }
        ("config-propose" | "secret-request", _) => {
            let what = e.str("id").or(e.str("name")).unwrap_or_default();
            bot.say(topic, &format!("⚙ {what} waits for you on the board"), &json!({}))?;
        }
        _ => {}
    }
    Ok(())
}
