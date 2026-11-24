use crate::runner::{argv, Runner};
use crate::Ctx;
use anyhow::{ensure, Context, Result};

/// The recipe's provider key `var` (`key_env`) and where it was found: the environment, else the 0600 file `<config>/secrets/<var>`. None: the provider's own CLI holds its login (boat's does).
pub fn key(ctx: &Ctx, var: &str) -> Result<Option<(String, String)>> {
    if var.is_empty() {
        return Ok(None);
    }
    if let Some(k) = crate::config::env(var).filter(|k| !k.is_empty()) {
        return Ok(Some((k, "env".into())));
    }
    let p = crate::secret::path(ctx, var);
    Ok(crate::secret::file(&p)?.map(|k| (k, p.display().to_string())))
}

/// Recipe `c`: `<config>/recipes/<c>.toml`, else the built-in `boat`.
pub fn load(ctx: &Ctx, c: &str) -> Result<toml::Table> {
    let p = user_recipe(ctx, c);
    let boat = || builtin(c).map(String::from);
    let text = std::fs::read_to_string(&p).ok().or_else(boat);
    let text = text.with_context(|| format!("cloud recipe {}", p.display()))?;
    Ok(toml::from_str(&text)?)
}

fn user_recipe(ctx: &Ctx, c: &str) -> std::path::PathBuf {
    ctx.config.with_file_name(format!("recipes/{c}.toml"))
}

fn builtin(c: &str) -> Option<&'static str> {
    (c == "boat").then_some(include_str!("../assets/recipes/boat.toml"))
}

/// Run recipe `rc`'s filled-in `line`: its provider key (when it has `key_env` and the key is set), then the runner `token` (as `RT`), exported from stdin.
pub fn run(ctx: &Ctx, r: &dyn Runner, rc: &toml::Table, what: &str, line: &str, token: Option<&str>) -> Result<String> {
    let var = rc.get("key_env").and_then(|v| v.as_str()).unwrap_or_default();
    let key = key(ctx, var)?.map(|k| k.0);
    let k = key.as_ref().map_or(String::new(), |_| {
        format!("IFS= read -r K; export {var}=\"$K\"; unset K; ")
    });
    let rt = token.map_or("", |_| "IFS= read -r RT; export RT; ");
    let input = [key.clone(), token.map(String::from)];
    let input: String = input.into_iter().flatten().map(|v| v + "\n").collect();
    let o = r.run_in("sh", &argv(&["-c", &format!("{k}{rt}{line}")]), &input)?;
    let mut err = o.stderr.trim().to_string();
    if let Some(k) = &key {
        err = err.replace(k, "•••");
    }
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        err = err.replace(t, "•••");
    }
    ensure!(o.code == 0, "{what} failed ({}): {err}", o.code);
    Ok(o.stdout)
}
