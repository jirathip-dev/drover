# ADR: providers — one module per provider behind one trait; core never names one (#806)

Status: proposed (architect lane, 2026-11-11). Read `usage.md` first, then this, `shapes.md`, `boundaries.md`.

## Question

How does herdsman keep every provider (harness) in one module behind a common trait — login, accounts, usage/limits,
models, plus launch, prompt and screen parsing — so that core names none of claude, anthropic, codex, openai, gemini,
hermes, and adding one is a new file plus one registry line (q1190)?

## Where it stands (staging 3d90dba1)

crates/core/src/harness/ (now providers/) already held a 40-method `Harness` trait with Generic defaults and five files. The leaks:

| Leak | Where |
|---|---|
| Five registries to touch per provider: `NAMES`, `ACP_ONLY`, `get()` match, `CONFIG_ENV`, `PROVIDERS` | `harness/mod.rs` |
| Anthropic proxy constants re-exported and used by name | `proxy.rs` (beta header merge), `spawn.rs` (`BASE_URL_ENV`, `AUTH_TOKEN_ENV`), `config.rs` (`PROXY_UPSTREAM`) |
| `codex login` hard-coded (codex is not in `NAMES`, so `all()` misses it) | `setup.rs:26` |
| Claude's banner as a core constant; `(Part, Part)` = Claude's 5 h/7 d | `shed.rs:279`, `usage.rs`, `claude_meter.rs` |
| Account env as a provider-string table; `efforts()` matches `"dsh"` | `access.rs`, `harness/mod.rs` |
| Hermes by name | `lib.rs` (`Ctx.hermes`, `FLEET_HERMES_PROFILES`), `cli.rs:2089`, `doctor.rs:229-238`, `spawn.rs:89-93`, `timer.rs:397`, `events.rs:305`, `serve.rs:3255` |
| AI co-author list | `hook.rs:782` |
| ~33 comment lines naming a provider | 15 files; the #316 grep skips `//` lines |
| CI check covers only claude/anthropic, `crates/*/src`, comments exempt | `.github/workflows/ci.yml:48` |

## Options

1. **Rename and extend in place (chosen).** `harness/` → `providers/`, `Harness` → `Provider`; keep the one trait and
   its method names (tests call them), add the missing facets as methods and small shapes (`Accounts`, `Proxy`,
   `Usage`/`Limits`, `Surface`, `Home`), and replace the five tables with one `registry!` line.
2. **A static `Spec` struct per provider plus a behaviour-only trait.** Tidier data, but every `h.login()` call site
   and test changes to `h.spec().login`; more churn for no behaviour.
3. **Split into facet traits** (`Login`, `Accounts`, `Usage`, `Launch`, `Screen`) each fetched from a provider.
   Cleanest on paper; ~100 call sites rewritten and the Generic fallbacks repeated per facet. Not worth it in a
   behaviour-unchanged refactor.
4. **Keep `harness/` and only widen the grep.** Leaves the five tables, the proxy and the Hermes chores in core;
   fails q1190's "one file + one registry line".

## Decision

- **Folder and names.** `crates/core/src/providers/{mod,claude,hermes,dsh,codex,gemini}.rs`, `claude/meter.rs` as
  claude's submodule. The config key stays `harness = "…"` (user-facing; no fleet.toml change): a role's `harness`
  names a provider. Submodules are private; core uses only `providers::` free functions.
- **Registry.** `registry! { claude, hermes, dsh, codex, gemini, #[cfg(test)] fake }` declares each `mod` and the
  `ALL: &[Entry]` list (`Entry { name, provider }`, name from the ident). Order is today's (`NAMES` then `ACP_ONLY`)
  because first-match readers depend on it; new providers append. `NAMES`, `ACP_ONLY`, `PROVIDERS`, `CONFIG_ENV` and
  the `get()` match are deleted; their answers derive from `ALL` and each provider's `surface()`.
- **Surface** says how a provider is reached: `Terminal` (claude, hermes, dsh: a `harness` choice), `Acp` (codex),
  `Listed` (gemini: only its ACP agent's vendor, for #732's impl≠rev rule; `get("gemini")` still returns Generic,
  so `of_agent` and the generic-harness test are unchanged).
- **Accounts.** `Credential::Account(Accounts { provider, config_env, token_env })`: the account env, folder var and
  token var come from the provider; `access.rs` loses `CONFIG_ENV` lookups. `{provider}` values stay `claude`/`codex`.
- **Proxy.** claude.rs exposes `Proxy { upstream, base_url_env, auth_token_env, headers }`; `proxy.rs` keeps the
  generic listener, keys and curl relay and calls `(px.headers)(kept)` for the beta merge. Exactly one proxied
  provider, as today (`providers::proxied()`); per-lane-provider proxies are out of scope.
- **Usage/limits.** `Usage { reading(&Value) -> Limits, cap_banner() }` with `Limits { session, week }` of
  `Window { used, resets }`. A provider fills the windows it has. On-disk `usage.jsonl` keys and `accounts` output do
  not change. `usage_limited` and `signed_out` stay screen readers on the trait.
- **Models.** `efforts()`, `takes_model()`, `model_default()` on the trait (dsh's `["none"]`, Hermes's `"profile"`).
- **Hermes chores** move into hermes.rs behind neutral hooks: `home()` (Ctx's dir and its env override; `Ctx.hermes`
  → `Ctx.homes`), `profile_files()` (timer), `hooks_allowed()` (`profiles sync --check`), `doctor()` rows,
  `scrubs_env()` (notifier). `setup.rs` lists every provider's `login()`; `hook.rs` takes `providers::brands()`
  plus `"copilot"`.
- **The check.** arch-check's `[[ban]]` gains `ignore_case: bool`; `.herdsman/arch.toml` gets one ban per name
  (`paths = ["crates/*/src/**"]`, `except = ["crates/core/src/providers/**"]`, `comments = true`). The #316 grep step
  is deleted (one way to check). Comment lines that name a provider are reworded or moved into the provider.
- **Proof of "one file + one line".** `providers/fake.rs` under `#[cfg(test)]` with unit tests that drive
  `Config` validation (`harness = "fake"` accepted), `setup::all` (its login listed), `access::account_env` (its
  `config_env`), a spawn `Plan`'s `start_cmd`/`env`, and `lane_names()`. `cfg(test)` keeps it out of the binary and of
  every integration test's lists, so no snapshot moves. An arch-check test fixture shows the ban failing on
  `Codex` in a core file and passing in `providers/`.

## Consequences

- Behaviour unchanged; tests change only by path/name (`herdsman::harness` → `herdsman::providers`, `NAMES` →
  `lane_names()`, `Ctx { hermes }` → `Ctx { homes }`, `usage_meter`/`status` → `usage`/`reading`). Setup's list
  keeps its order (lane logins, `gh`, then ACP-only logins).
- Core gains no layer edge: providers are inside core. Rough size: src net ≈ +120 (shapes, registry macro, ban flag)
  against ≈ −60 (tables, Hermes code in core, the grep step); the fake is test code.
- `herdsman doctor`'s Hermes rows and `profiles sync --check` read the same but come from the provider.
- First unit tests in `crates/core` (`fake.rs`); everything else stays in `crates/cli/tests`.
- Docs: `docs/providers.md` gains "Adding a provider" (the file, the registry line, which methods matter);
  `.herdsman/rules.md` (q1193, when it lands) lists the ban with its enforcer.

## Open questions for the owner

1. **Comments in the ban.** Decided `comments = true` (core never names one, even in `///`): ~33 lines reworded. The
   alternative is to leave comments to the q1191 strip. Recommend: keep `true`.
2. **`web/`.** `web/src/system.ts` hard-codes an Anthropic accounts card and `host.ts` filters the `claude` setup.
   Not in this issue's paths (scope item 4 says the providers folder, i.e. Rust). Recommend: a follow-up issue that
   sends the card's vendor label from the server and adds `web/src/**` to the ban.
3. **gemini.rs** exists only to carry `gemini-acp → google` for #732. Recommend: keep it (`Listed`); dropping the row
   would change config validation.
