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
// CEP:ID: CS-a14
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

/// Rosetta cross-file checks: registry and docs must agree, and the
/// documentation files must carry the tier and inquisition sections.
// CEP:WHAT Exercises the Rosetta policy on a synthetic registry and tree.
// CEP:WHY The 68.6.1 rules are cross-file and need fixture-driven tests.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(fixtures).
// CEP:ID: CS-a15
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn rosetta_registry_and_docs_must_agree() {
    let cfg = repo_cfg();
    let registry = r#"{"CS-a01": {"path": "crates/x.rs", "function": "big"}}"#;
    let good = ("docs/functions/CS/CS-a01.md".to_string(),
        "# Big\n\n## Tier 1: Beginner\n\nAdds numbers.\n\n## Tier 2: Silicon Reality\n\nOne add uop.\n\n## Silent Failure Checklist\n\n- Fallbacks: none.\n- Truncation: none.\n- Wrapping: none.\n- Error codes: none.\n".to_string());
    let found = cep_lint::rules::rosetta_findings(&cfg, registry, std::slice::from_ref(&good));
    assert!(
        found.is_empty(),
        "complete fixture must be clean: {found:?}"
    );

    // Missing md file.
    let found = cep_lint::rules::rosetta_findings(&cfg, registry, &[]);
    assert!(found.iter().any(|f| f.rule == "CEP-LINT-DOC-FILE"));

    // Tier headers missing.
    let bad_tiers = (
        good.0.clone(),
        good.1.replace("## Tier 2: Silicon Reality\n\n", ""),
    );
    let found = cep_lint::rules::rosetta_findings(&cfg, registry, &[bad_tiers]);
    assert!(found.iter().any(|f| f.rule == "CEP-LINT-DOC-TIERS"));

    // Inquisition section missing.
    let bad_inq = (
        good.0.clone(),
        good.1.replace("## Silent Failure Checklist\n\n", ""),
    );
    let found = cep_lint::rules::rosetta_findings(&cfg, registry, &[bad_inq]);
    assert!(found.iter().any(|f| f.rule == "CEP-LINT-DOC-INQUISITION"));

    // md without a registry entry.
    let orphan = ("docs/functions/CS/CS-z99.md".to_string(), good.1.clone());
    let found = cep_lint::rules::rosetta_findings(&cfg, registry, &[good, orphan]);
    assert!(found
        .iter()
        .any(|f| f.message.contains("no registry entry")));
}

/// SECRET-SCAN flags assignment-shaped secrets but not prose.
// CEP:WHAT Exercises the secret probe on comment content.
// CEP:WHY Severity 0 rules must be pinned by tests.
// CEP:STATUS complete
// CEP:FAILURE A failing assertion aborts the test binary.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn secret_scan_needs_assignment_context() {
    let cfg = repo_cfg();
    let header = "// CEP:FILE scenario/s.rs\n// CEP:WHAT p\n// CEP:WHY p\n// CEP:CLASS CEP-2\n// CEP:STATUS complete\n// CEP:FAILURE p\n// CEP:ASSUMES p\n// CEP:COST p\n// CEP:EVIDENCE p\n";
    let leak = format!("{header}// password = \"hunter2hunter2\"\npub fn ok() {{}}\n");
    let hits = scan_file(&cfg, "scenario/s.rs", &leak)
        .iter()
        .filter(|f| f.rule == "CEP-LINT-SECRET-SCAN")
        .count();
    assert_eq!(hits, 1);
    let prose = format!("{header}// passwords are hashed before storage\npub fn ok() {{}}\n");
    let hits = scan_file(&cfg, "scenario/s.rs", &prose)
        .iter()
        .filter(|f| f.rule == "CEP-LINT-SECRET-SCAN")
        .count();
    assert_eq!(hits, 0);
}
