// Regression test for the silent-empty-results bug.
//
// dispatch.rs used to derive the index path as `<root>/.crabcc/index.db`
// while the CLI resolved it through the centralised
// `$CRABCC_HOME/repos/<key>/` layout. Because `Store::open` creates
// whatever path it is handed, the MCP server did not error on the
// mismatch — it created a fresh empty database and answered every lookup
// with `[]`. No error, no warning, just an apparently empty codebase for
// any agent on the other end.
//
// A unit test over `index_layout` alone cannot catch this: the rule was
// never wrong, dispatch simply did not consult it. The assertion has to
// run through the real dispatch entry point against a repo indexed the
// way the CLI indexes one.

use std::path::Path;

use crabcc_core::index_layout;
use serde_json::{json, Value};
use tempfile::TempDir;

/// Index `root` exactly where `index_layout` says the artifacts live —
/// i.e. what `crabcc index` does — then return the db path.
fn index_repo(root: &Path) -> std::path::PathBuf {
    let db = index_layout::index_db(root).expect("resolve index db");
    std::fs::create_dir_all(db.parent().unwrap()).expect("create data dir");
    let store = crabcc_core::store::Store::open(&db).expect("open store");
    let stats = crabcc_core::index::build_index(root, &store).expect("build index");
    assert!(
        stats.symbols > 0,
        "fixture produced no symbols; the test would pass vacuously"
    );
    db
}

fn call_sym(root: &Path, name: &str) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "sym", "arguments": {"name": name}}
    });
    let resp = crabcc_mcp::handle_with(&req, root, false);
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("unexpected MCP response shape: {resp}"));
    serde_json::from_str(text).expect("tool payload is JSON")
}

/// A repo with no in-repo `.crabcc/` uses the centralised layout — the
/// default for essentially every repo. This is the case that silently
/// returned nothing.
#[test]
fn mcp_finds_symbols_in_a_centrally_indexed_repo() {
    let home = TempDir::new().unwrap();
    // Scope the layout to this test's own $CRABCC_HOME. Single-test
    // binary, set before any resolution happens.
    std::env::set_var("CRABCC_HOME", home.path());

    let repo = TempDir::new().unwrap();
    std::fs::write(
        repo.path().join("a.ts"),
        "export function hello(name: string) { return name; }\n\
         export function greet() { return hello(\"world\"); }\n",
    )
    .unwrap();

    let db = index_repo(repo.path());
    assert!(
        db.starts_with(home.path()),
        "expected the centralised layout for this fixture, got {}",
        db.display()
    );
    // The exact path the buggy code used. Its absence is what made the
    // failure silent rather than loud.
    assert!(
        !repo.path().join(".crabcc").join("index.db").exists(),
        "fixture should not have an in-repo index"
    );

    let syms = call_sym(repo.path(), "hello");
    assert_eq!(
        syms.as_array().map(|a| a.len()),
        Some(1),
        "MCP sym returned {syms} — it resolved a different database than the indexer wrote"
    );
    assert_eq!(syms[0]["name"], "hello");
}

/// The legacy in-repo layout must keep working: when `<root>/.crabcc/`
/// already exists it wins, and dispatch has to follow it there too.
#[test]
fn mcp_finds_symbols_in_an_in_repo_indexed_repo() {
    let repo = TempDir::new().unwrap();
    std::fs::create_dir_all(repo.path().join(".crabcc")).unwrap();
    std::fs::write(
        repo.path().join("a.ts"),
        "export function solo(x: number) { return x; }\n",
    )
    .unwrap();

    let db = index_repo(repo.path());
    assert_eq!(db, repo.path().join(".crabcc").join("index.db"));

    let syms = call_sym(repo.path(), "solo");
    assert_eq!(syms.as_array().map(|a| a.len()), Some(1), "got {syms}");
    assert_eq!(syms[0]["name"], "solo");
}
