# Providers: how the callers read (#806)

Read this first. These are the call sites after the change; the ADR explains why, `shapes.md` has the types.

## Core asks by name, never names one

```rust
// spawn.rs: a lane's provider is whatever its role's `harness` says.
let p = providers::get(&plan.harness);            // unknown name: the Generic fallbacks, as today
cmds.push(p.start_cmd(ctx, &plan, pane));
if let Credential::Account(a) = p.credential() {  // was Credential::Account("claude") + CONFIG_ENV lookup
    env.insert(a.config_env.into(), cfg.accounts.folder(name).display().to_string());
}

// spawn.rs, remote ACP lane (#637): the proxy env comes from the proxied provider, not two `pub const`s.
if let Some(px) = providers::proxied() {
    p.env.insert(px.base_url_env.into(), crate::proxy::url(cfg));
    secrets += &format!("export {}='{key}'\n", px.auth_token_env);
}
```

## Lists come from the registry

```rust
// setup.rs: every provider's login, gh between the lane ones and the ACP-only ones (order unchanged).
let logins = providers::lanes().filter_map(|p| Some((p.name, p.login()?)))
    .chain([("gh", "gh auth login")])
    .chain(providers::acp_only().filter_map(|p| Some((p.name, p.login()?))));

// cli.rs / config.rs / serve: the `harness` choices.
#[arg(long, value_parser = PossibleValuesParser::new(providers::lane_names()))]
bail!("chat.harness: \"{h}\" must be one of {:?}", providers::lane_names());

// hook.rs (#198): AI co-author names are the providers' brands plus the vendor-less rest.
let ai = providers::brands().chain(["copilot"]);

// config.rs (#732): impl and rev on different vendors.
let (a, b) = (providers::vendor_of_agent(impl_id), providers::vendor_of_agent(rev_id));
```

## Usage and limits through one shape

```rust
// usage.rs: `herdsman statusline` (#707).
let Some(u) = providers::accounts().and_then(|p| p.usage()) else { return Ok(()) };
let l: Limits = u.reading(&status_json);          // was (Part, Part)
row.five_hour = l.session;                        // usage.jsonl keys unchanged
row.seven_day = l.week;

// shed.rs: an ACP lane's exit cause.
let pat = cfg.accounts.cap_pattern.as_deref()
    .or_else(|| providers::accounts().and_then(|p| p.usage()).map(|u| u.cap_banner()))
    .unwrap_or_default();
```

## Provider-owned chores core used to do by name

```rust
// doctor.rs: each provider adds its own rows (Hermes: profile exists, hooks allowed).
for p in providers::lanes() { rows.extend(p.doctor(ctx, cfg, r)); }

// spawn::profiles_check: each provider checks its own profiles' hooks.
for p in providers::lanes() { good &= p.hooks_allowed(ctx, cfg, r, out)?; }

// timer.rs: config files to fingerprint.
for (role, def) in roles { v.extend(providers::get(&def.harness).profile_files(ctx, def.profile_or(role))); }

// events.rs: env a notifier must not inherit.
.filter(|k| providers::all().any(|p| p.scrubs_env(k)))

// serve.rs: the model a role gets when unset.
model_default: p.model_default().unwrap_or(&r.harness).into(),

// lib.rs: Ctx::from_env fills each provider's home dir from its own env override.
homes: providers::all().filter_map(|p| Some((p.name, p.home()?.resolve()))).collect(),
```

## Adding a provider

```rust
// crates/core/src/providers/acme.rs — the new file
pub struct Acme;
impl Provider for Acme {
    fn surface(&self) -> Surface { Surface::Terminal }
    fn login(&self) -> Option<&'static str> { Some("acme login") }
    // … only what differs from the defaults
}
pub static PROVIDER: Acme = Acme;

// crates/core/src/providers/mod.rs — the one line
registry! { claude, hermes, dsh, codex, gemini, acme, #[cfg(test)] fake }
```
