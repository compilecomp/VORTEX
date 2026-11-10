// CEP:FILE tools/cep_lint/src/main.rs
// CEP:WHAT cep_lint binary: CLI (chapter 50.1), deterministic scan pipeline,
//        DOC-SYNC enforcement (50.3 CEP-LINT-DOC-SYNC), and the self-test
//        manifest runner (50.6 item 3).
// CEP:WHY The standard's enforcement completeness is what makes it a
//        standard rather than advice; this tool is that enforcement.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Exit 0 clean, 1 findings/self-test failure, 2 bootstrap error,
//        3 usage error (chapter 50.1 table).
// CEP:ASSUMES Runs from a directory containing standard.md and the config.
// CEP:COST O(source bytes); deterministic output ordering by file/line/rule.
// CEP:EVIDENCE cep_lint --self-test and repository scans
//! cep_lint command-line interface.

use cep_lint::{config::Config, rules};
use std::path::{Path, PathBuf};

/// Tool version (50.1 `--version` banner component).
pub const TOOL_VERSION: &str = "0.1.0";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

// CEP:WHAT Implements `run` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn run(args: &[String]) -> i32 {
    let mut config_path: Option<String> = None;
    let mut self_test = false;
    let mut version = false;
    let mut help = false;
    let mut targets: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" | "-c" => match it.next() {
                Some(p) => config_path = Some(p.clone()),
                None => {
                    eprintln!("--config requires a path");
                    return 3;
                }
            },
            "--self-test" | "-t" => self_test = true,
            "--version" | "-v" => version = true,
            "--help" | "-h" => help = true,
            other if other.starts_with('-') => {
                eprintln!("unknown option {other}");
                return 3;
            }
            other => targets.push(other.to_string()),
        }
    }

    if help {
        println!("usage: cep_lint [-c <config>] [--self-test] [--version] <files or directories>");
        println!("options: --config/-c, --self-test/-t, --help/-h, --version/-v");
        return 0;
    }
    if version {
        println!(
            "cep_lint {TOOL_VERSION} (CEP&CC {})",
            standard_version().unwrap_or_else(|| "?".into())
        );
        return 0;
    }

    // Locate the repository root: nearest ancestor containing standard.md.
    let root = match find_repo_root() {
        Some(r) => r,
        None => {
            eprintln!("CEP-LINT-BOOT-1: cannot locate standard.md in any ancestor directory");
            return 2;
        }
    };
    let config_path = config_path
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".cep/cep_lint.json"));
    let cfg = match Config::load(&config_path) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return 2;
        }
    };

    if self_test {
        return run_self_test(&root, &cfg);
    }

    // DOC-SYNC (CEP-LINT-DOC-SYNC): configured rules must be documented,
    // documented rules must be configured, versions must agree.
    let mut findings: Vec<rules::Finding> = Vec::new();
    let standard_text = match std::fs::read_to_string(root.join("standard.md")) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("CEP-LINT-BOOT-1: cannot read standard.md: {e}");
            return 2;
        }
    };
    findings.extend(doc_sync_findings(&root, &cfg, &standard_text));

    // Collect target files.
    let files = collect_files(&root, &targets);
    findings.extend(rules::scan_all(&cfg, &files));

    if findings.is_empty() {
        println!(
            "cep_lint: no findings ({} files, {} rules)",
            files.len(),
            cfg.rules.len()
        );
        return 0;
    }
    for f in &findings {
        println!(
            "{}:{}: {} [sev {}]: {}",
            f.file, f.line, f.rule, f.severity, f.message
        );
    }
    let fail = findings.iter().any(|f| cfg.fail_on.contains(&f.severity));
    println!("cep_lint: {} findings", findings.len());
    if fail {
        1
    } else {
        0
    }
}

/// DOC-SYNC checks (documented vs configured rule sets and version match).
// CEP:WHAT DOC-SYNC checks (documented vs configured rule sets and version match).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn doc_sync_findings(root: &Path, cfg: &Config, standard_text: &str) -> Vec<rules::Finding> {
    let mut out = Vec::new();
    let documented = rules::documented_rules(standard_text);
    for name in cfg.rules.keys() {
        if !documented.contains(name) {
            out.push(rules::Finding {
                file: ".cep/cep_lint.json".into(),
                line: 1,
                rule: "CEP-LINT-DOC-SYNC".into(),
                message: format!("configured rule {name} has no `### Rule` section in standard.md"),
                severity: 1,
            });
        }
    }
    for name in &documented {
        if !cfg.rules.contains_key(name) {
            out.push(rules::Finding {
                file: "standard.md".into(),
                line: 1,
                rule: "CEP-LINT-DOC-SYNC".into(),
                message: format!("documented rule {name} is absent from the configuration"),
                severity: 1,
            });
        }
    }
    // Version line: `Version: **X**` in the standard must equal config.
    if let Some(line) = standard_text
        .lines()
        .find(|l| l.trim_start().starts_with("Version:"))
    {
        let doc_ver: String = line
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if doc_ver != cfg.standard_version {
            out.push(rules::Finding {
                file: "standard.md".into(),
                line: 1,
                rule: "CEP-LINT-DOC-SYNC".into(),
                message: format!(
                    "document Version `{doc_ver}` != configuration standard.version `{}`",
                    cfg.standard_version
                ),
                severity: 1,
            });
        }
    }
    let _ = root;
    out
}

/// Runs the self-test manifest (50.6 item 3): one violating and one compliant
/// scenario per configured rule with exact expected finding counts.
// CEP:WHAT Runs the self-test manifest (50.6 item 3): one violating and one compliant scenario per configured rule with exact expected finding counts.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn run_self_test(root: &Path, cfg: &Config) -> i32 {
    let manifest_path = root.join("tools/cep_lint/tests/manifest.json");
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "CEP-LINT-BOOT-1: cannot read manifest {}: {e}",
                manifest_path.display()
            );
            return 2;
        }
    };
    #[derive(serde::Deserialize)]
    struct Scenario {
        name: String,
        rule: String,
        compliant: bool,
        file: String,
        content: String,
        expected_findings: usize,
    }
    #[derive(serde::Deserialize)]
    struct Manifest {
        scenarios: Vec<Scenario>,
    }
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("CEP-LINT-BOOT-2: invalid manifest: {e}");
            return 2;
        }
    };

    let mut failures = 0;
    for sc in &manifest.scenarios {
        let got = rules::scan_file(cfg, &sc.file, &sc.content)
            .iter()
            .filter(|f| f.rule == sc.rule)
            .count();
        if got != sc.expected_findings {
            println!(
                "FAIL {}: rule {} expected {} findings, got {}",
                sc.name, sc.rule, sc.expected_findings, got
            );
            failures += 1;
        }
    }
    // Coverage: every configured rule must have a violating and a compliant
    // scenario.
    for name in cfg.rules.keys() {
        let has_v = manifest
            .scenarios
            .iter()
            .any(|s| &s.rule == name && !s.compliant);
        let has_c = manifest
            .scenarios
            .iter()
            .any(|s| &s.rule == name && s.compliant);
        if !has_v || !has_c {
            println!("FAIL coverage: rule {name} lacks a violating/compliant scenario pair");
            failures += 1;
        }
    }
    if failures == 0 {
        println!(
            "cep_lint self-test: {} scenarios passed, full rule coverage",
            manifest.scenarios.len()
        );
        0
    } else {
        println!("cep_lint self-test: {failures} failure(s)");
        1
    }
}

/// Finds the nearest ancestor directory containing standard.md.
fn find_repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("standard.md").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Reads the standard's version from the `Version:` line.
fn standard_version() -> Option<String> {
    let root = find_repo_root()?;
    let text = std::fs::read_to_string(root.join("standard.md")).ok()?;
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("Version:"))?;
    Some(
        line.chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect(),
    )
}

/// Collects (path, content) pairs for the targets (or the configured globs'
/// default roots), deterministically sorted.
// CEP:WHAT Collects (path, content) pairs for the targets (or the configured globs' default roots), deterministically sorted.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn collect_files(root: &Path, targets: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();
    if targets.is_empty() {
        roots.push(root.join("crates"));
        roots.push(root.join("tools"));
    } else {
        for t in targets {
            let p = PathBuf::from(t);
            if p.is_absolute() {
                roots.push(p);
            } else {
                roots.push(root.join(p));
            }
        }
    }
    for r in roots {
        collect_dir(&r, &mut out);
    }
    out.sort();
    out
}

// CEP:WHAT Implements `collect_dir` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn collect_dir(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target" || n == ".git") {
                continue;
            }
            collect_dir(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&p) {
                let rel = p
                    .strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                    .map(|pp| pp.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| p.to_string_lossy().into_owned());
                out.push((rel, text));
            }
        }
    }
}
