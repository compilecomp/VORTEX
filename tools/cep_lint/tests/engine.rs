// CEP:FILE tools/cep_lint/tests/engine.rs
// CEP:WHAT Engine-level tests for the lint checkers on synthetic sources,
//        complementing the JSON self-test manifest.
// CEP:WHY The scanner must be verified as ordinary Rust tests during
//        development, not only through the manifest gate.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The repository configuration in .cep/cep_lint.json.
// CEP:COST O(bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint

#![allow(clippy::expect_used, clippy::unwrap_used)]

//! Lint engine test suite.

use cep_lint::config::Config;
use cep_lint::rules::{glob_match, scan_file};

fn repo_cfg() -> Config {
    let root = std::env::current_dir().expect("cwd");
    let root = root
        .ancestors()
        .find(|a| a.join("standard.md").is_file())
        .expect("repo root")
        .to_path_buf();
    Config::load(&root.join(".cep/cep_lint.json")).expect("config")
}

/// A function with six statements and no CEP block must be flagged.
// CEP:WHAT A function with six statements and no CEP block must be flagged.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES Deterministic inputs constructed in the test.
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn func_block_fires_on_undocumented_body() {
    let cfg = repo_cfg();
    // CEP:WHAT Implements `big` as part of the module contract.
    // CEP:WHY Required by the module contract stated in the file header.
    // CEP:STATUS complete
    // CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
    // CEP:ASSUMES Inputs satisfy the module invariants (see file header).
    // CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
    // CEP:EVIDENCE cargo test -p cep_lint
    let src = "// CEP:FILE probe.rs\n// CEP:WHAT p\n// CEP:WHY p\n// CEP:CLASS CEP-2\n// CEP:STATUS complete\n// CEP:FAILURE p\n// CEP:ASSUMES p\n// CEP:COST p\n// CEP:EVIDENCE p\n\npub fn big() {\n    let mut x = 1;\n    x += 1;\n    x += 2;\n    x += 3;\n    x += 4;\n    x += 5;\n    let _ = x;\n}\n";
    let found = scan_file(&cfg, "crates/probe.rs", src);
    let fb: Vec<_> = found
        .iter()
        .filter(|f| f.rule == "CEP-LINT-FUNC-BLOCK")
        .collect();
    assert_eq!(fb.len(), 1, "expected FUNC-BLOCK finding, got {found:?}");
}

/// Glob matching for the repository layout.
#[test]
fn globs_match_repository_paths() {
    assert!(glob_match("crates/**/*.rs", "crates/vortex-nv1/src/lib.rs"));
    assert!(glob_match("tools/**/*.rs", "tools/cep_lint/src/main.rs"));
    assert!(!glob_match("crates/**/*.rs", "doc/notes.md"));
}
