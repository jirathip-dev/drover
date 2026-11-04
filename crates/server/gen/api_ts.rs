//! #265: web/src/api.ts is the API v1 types (crates/server/src/serve/api.rs) as ts-rs writes them; this test fails
//! when the committed file is stale. `BLESS=1 cargo test -p herdsman-server` rewrites it.
use super::*;
use ts_rs::TS;

#[test]
fn web_api_ts_matches_the_rust_types() {
    let cfg = ts_rs::Config::new().with_large_int("number");
    let decls = decls(&cfg).into_iter().chain([serde_json::Value::decl(&cfg)]);
    let mut ts =
        "// Generated from crates/server/src/serve/api.rs by crates/server/gen/api_ts.rs: do not edit.\n".to_string();
    for d in decls {
        ts += &format!("\nexport {d}\n");
    }
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/api.ts");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&file, &ts).unwrap();
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        old == ts,
        "web/src/api.ts is stale: run `BLESS=1 cargo test -p herdsman-server`\n{ts}"
    );
}
