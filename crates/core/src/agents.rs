use crate::runner::Runner;
use anyhow::Result;
use serde_json::Value;

/// One agent of shed's list, as drover reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub vendor: String,
    pub installed: Option<Installed>,
    pub install: Option<String>,
    pub login: Option<String>,
    pub takes_model: bool,
    pub efforts: Vec<String>,
}

/// An installed agent's version and adapter command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub cmd: Vec<String>,
}

/// shed's agent list on the main host.
#[derive(Debug, Clone, Default)]
pub struct Catalog(pub Vec<Agent>);

fn agent(v: &Value) -> Agent {
    let p = v.get("provider").unwrap_or(&Value::Null);
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    let at = v.get("installed").filter(|i| !i.is_null()).unwrap_or(v);
    let installed = at
        .get("cmd")
        .and_then(|c| serde_json::from_value(c.clone()).ok())
        .map(|cmd| Installed {
            version: s(at, "version").unwrap_or_default(),
            cmd,
        });
    let set = |k: &str| {
        p.get(k)
            .is_some_and(|x| !x.is_null() && x.as_array().is_none_or(|a| !a.is_empty()))
    };
    Agent {
        id: s(v, "id").unwrap_or_default(),
        vendor: (s(p, "agent").filter(|a| !a.is_empty()))
            .or_else(|| crate::providers::of_agent(v.get("id")?.as_str()?).map(String::from))
            .or_else(|| s(v, "id"))
            .unwrap_or_default(),
        installed,
        install: s(p, "install"),
        login: s(p, "login"),
        takes_model: set("model_env") || set("model_args"),
        efforts: (p.get("efforts").and_then(Value::as_array).into_iter().flatten())
            .filter_map(|e| e.as_str().map(String::from))
            .collect(),
    }
}

impl Catalog {
    /// `shed agents --json` on runner `r`'s host.
    pub fn read(r: &dyn Runner) -> Result<Catalog> {
        let all: Value = serde_json::from_str(&crate::shed::cli(r, &["agents", "--json"])?.stdout)?;
        Ok(Catalog(all.as_array().into_iter().flatten().map(agent).collect()))
    }
    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.0.iter().find(|a| a.id == id)
    }
}

/// Agent `id` from shed's list, installed; else why not, with its fix.
pub fn installed(c: &Catalog, id: &str) -> Result<Agent> {
    let a = c
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("ACP agent {id} is not in shed's list; `shed agents add {id}`"))?;
    if a.installed.is_none() {
        let fix = a.install.clone().unwrap_or_else(|| format!("shed agents add {id}"));
        anyhow::bail!("ACP agent {id} is not installed; `{fix}`");
    }
    Ok(a.clone())
}

/// `BAD_VALUE` when a role's own model or effort is one agent `a` does not take.
pub fn fits(a: &Agent, model: Option<&str>, effort: Option<&str>) -> Result<()> {
    if let Some(m) = model.filter(|_| !a.takes_model) {
        return crate::gate::refused("BAD_VALUE", format!("agent {} takes no model, not {m:?}", a.id));
    }
    if let Some(e) = effort.filter(|e| !a.efforts.iter().any(|x| x == e)) {
        return crate::gate::refused(
            "BAD_VALUE",
            format!("agent {} takes no effort {e:?} ({:?})", a.id, a.efforts),
        );
    }
    Ok(())
}

/// Every agent a shed-acp fleet's role names, with the guard it runs under.
pub fn named(cfg: &crate::config::Config) -> std::collections::BTreeMap<String, crate::acp::Guard> {
    let mut all = std::collections::BTreeMap::new();
    for (n, f) in &cfg.fleet {
        let Some(s) = cfg.shed_for(n) else { continue };
        for r in f.role.values() {
            let id = r
                .agent
                .clone()
                .or_else(|| crate::providers::get(&r.harness).acp().map(Into::into));
            if let Some(id) = id.or_else(|| s.acp.clone()) {
                all.insert(id.clone(), crate::acp::guard(&s.policy, &id));
            }
        }
    }
    all
}
