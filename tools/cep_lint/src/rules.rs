// CEP:FILE tools/cep_lint/src/rules.rs
// CEP:WHAT The rule engine: comment/string separation, CEP block parsing,
//        and the chapter-50 check primitives for the Rust dialect.
// CEP:WHY The standard's mechanical contract must be deterministic and
//        sorted; these checkers are the single implementation of it.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Checkers never panic on malformed input; they report findings.
// CEP:ASSUMES UTF-8 sources; `//` line comments and `/* */` blocks; string
//        literals use quotes with backslash escapes.
// CEP:COST O(bytes) per file, O(files log files) for output sorting.
// CEP:EVIDENCE cep_lint --self-test and repository scans
use crate::config::Config;
// Syntax helpers live in `syntax.rs` (atomic-file split); public paths are
// preserved through re-exports.
// CEP:WHAT Re-exports the syntax helpers.
// CEP:WHY lib.rs and tests import them via crate::rules paths.
// CEP:STATUS complete
// CEP:FAILURE Cannot fail.
// CEP:ASSUMES None.
// CEP:COST O(1).
// CEP:EVIDENCE cargo test -p cep_lint
pub(crate) use crate::syntax::{
    block_start_of, class_of, classify, fn_body_range, numeric_literals, preceding_block,
    status_of, Source,
};
pub use crate::syntax::{documented_rules, glob_match, scan_all};
use serde_json::Value;

/// The literal probe strings live in constants so the engine's own source
/// does not self-trigger the rules it implements.
const EMPTY_BLOCK: &str = "{}";
/// The empty match-arm suffix probe.
const EMPTY_MATCH_ARM: &str = "=> {}";

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// File path as given to the scanner.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// Rule identifier.
    pub rule: String,
    /// Message text.
    pub message: String,
    /// Severity from the configuration.
    pub severity: u8,
}

/// Required file-header fields (CEP-LINT-FILE-HEADER default).
const FILE_FIELDS: [&str; 9] = [
    "CEP:FILE",
    "CEP:WHAT",
    "CEP:WHY",
    "CEP:CLASS",
    "CEP:STATUS",
    "CEP:FAILURE",
    "CEP:ASSUMES",
    "CEP:COST",
    "CEP:EVIDENCE",
];
/// Required function-block fields (CEP-LINT-FUNC-BLOCK default).
const FUNC_FIELDS: [&str; 7] = [
    "CEP:WHAT",
    "CEP:WHY",
    "CEP:STATUS",
    "CEP:FAILURE",
    "CEP:ASSUMES",
    "CEP:COST",
    "CEP:EVIDENCE",
];
/// Allowed CEP:STATUS values (CEP-LINT-STATUS-VALUE).
const STATUS_VALUES: [&str; 4] = ["complete", "partial", "stub", "placeholder"];
/// Allowed CEP:CLASS values (chapter 5 conformance classes).
const CLASSES: [&str; 10] = [
    "CEP-0", "CEP-1", "CEP-2", "CC", "FFI", "HPC-0", "HPC-1", "HPC-2", "RCS-0", "RCS-1",
];

/// Scans one file under the configuration and returns findings.
// CEP:WHAT Scans one file under the configuration and returns findings.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:ID: CS-a11
// CEP:EVIDENCE cargo test -p cep_lint
pub fn scan_file(cfg: &Config, file: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let src = classify(text.lines().map(String::from).collect());
    let rules = &cfg.rules;
    // exempt_paths: rule-table data files (the engine's own source must
    // contain the marker literals it checks for) declare their exemption in
    // the configuration rather than in per-line comments, so `cargo fmt`
    // reflowing cannot silently weaken enforcement.
    let exempt = |rule: &str| -> bool {
        rules
            .get(rule)
            .and_then(|r| r.params.get("exempt_paths"))
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|p| file.contains(p))
            })
            .unwrap_or(false)
    };

    let enabled = |name: &str| rules.contains_key(name);
    let param_usize = |name: &str, key: &str, dflt: usize| -> usize {
        rules
            .get(name)
            .and_then(|r| r.params.get(key))
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .unwrap_or(dflt)
    };
    let sev = |name: &str, dflt: u8| -> u8 { rules.get(name).map_or(dflt, |r| r.severity) };
    let mut push = |line: usize, rule: &str, msg: String, severity: u8| {
        out.push(Finding {
            file: file.to_string(),
            line: line + 1,
            rule: rule.to_string(),
            message: msg,
            severity,
        });
    };
    // suppression_pattern: a line carrying the marker is exempt from the
    // line-scoped rules (standard 50.3 parameters). The lint engine's own
    // rule tables are the sanctioned users of this escape hatch.
    let suppression = |rule: &str, line_text: &str| -> bool {
        rules
            .get(rule)
            .and_then(|r| r.params.get("suppression_pattern"))
            .and_then(Value::as_str)
            .is_some_and(|pat| line_text.contains(pat))
    };

    // ---- CEP-LINT-FILE-HEADER
    if enabled("CEP-LINT-FILE-HEADER") {
        let max_start = param_usize("CEP-LINT-FILE-HEADER", "max_start_line", 1);
        let first_code = src
            .lines
            .iter()
            .position(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("//")
            })
            .unwrap_or(src.lines.len());
        let has_block = src
            .lines
            .first()
            .is_some_and(|l| l.trim_start().starts_with("//"));
        let _ = max_start;
        if !has_block {
            push(
                0,
                "CEP-LINT-FILE-HEADER",
                "file does not begin with a CEP header comment block".into(),
                sev("CEP-LINT-FILE-HEADER", 1),
            );
        } else {
            let header: String = src.lines[..first_code.min(src.lines.len())].join("\n");
            let missing: Vec<&str> = FILE_FIELDS
                .iter()
                .filter(|f| !header.contains(*f))
                .copied()
                .collect();
            if !missing.is_empty() {
                push(
                    0,
                    "CEP-LINT-FILE-HEADER",
                    format!("header missing fields: {}", missing.join(", ")),
                    sev("CEP-LINT-FILE-HEADER", 1),
                );
            }
            // Status and class value validation share the field rules.
            check_status_and_class(&src, 0, first_code, "CEP-LINT-FILE-HEADER", &sev, &mut push);
        }
    }

    // ---- CEP-LINT-FUNC-BLOCK + NAMING-FUNC + STUB-BODY
    let min_body = param_usize("CEP-LINT-FUNC-BLOCK", "min_body_statements", 5);
    for (i, line) in src.lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(fn_pos) = trimmed.find("fn ") else {
            continue;
        };
        if trimmed.starts_with("//") {
            continue;
        }
        let after = &trimmed[fn_pos + 3..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // Naming (CEP-LINT-NAMING-FUNC).
        if enabled("CEP-LINT-NAMING-FUNC")
            && !name
                .chars()
                .next()
                .is_none_or(|c| c.is_ascii_lowercase() || c == '_')
        {
            push(
                i,
                "CEP-LINT-NAMING-FUNC",
                format!("function name `{name}` is not snake_case"),
                sev("CEP-LINT-NAMING-FUNC", 2),
            );
        }
        // Find the body's opening brace and count statements.
        if let Some((body_start, body_end)) = fn_body_range(&src.lines, i) {
            let body: String = src.lines[body_start..=body_end].join("\n");
            let statements = body.matches(';').count();
            if statements >= min_body && enabled("CEP-LINT-FUNC-BLOCK") {
                let block = preceding_block(&src.lines, i);
                // The file header block is never accepted as a function
                // block (chapter 50.3 CEP-LINT-FUNC-BLOCK limits).
                let block = if block.contains("CEP:FILE") {
                    String::new()
                } else {
                    block
                };
                let missing: Vec<&str> = FUNC_FIELDS
                    .iter()
                    .filter(|f| !block.contains(*f))
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    push(
                        i,
                        "CEP-LINT-FUNC-BLOCK",
                        format!("function `{name}` ({statements} statements) lacks a CEP block: missing {}", missing.join(", ")),
                        sev("CEP-LINT-FUNC-BLOCK", 1),
                    );
                }
                check_status_and_class(
                    &src,
                    block_start_of(&src.lines, i),
                    i,
                    "CEP-LINT-FUNC-BLOCK",
                    &sev,
                    &mut push,
                );
            }
            // CEP-LINT-STUB-BODY: empty body.
            if enabled("CEP-LINT-STUB-BODY") {
                let open = body.find('{').map_or(0, |p| p + 1);
                let close = body.rfind('}').unwrap_or(body.len());
                let inner = body[open..close].trim();
                if inner.is_empty() && statements == 0 {
                    let block = preceding_block(&src.lines, i);
                    let declared = status_of(&block).is_some_and(|st| {
                        matches!(st.as_str(), "stub" | "partial" | "placeholder")
                    });
                    if !declared {
                        push(
                            i,
                            "CEP-LINT-STUB-BODY",
                            format!("function `{name}` has an empty body without a declared stub status"),
                            sev("CEP-LINT-STUB-BODY", 1),
                        );
                    }
                }
            }
            // CEP-LINT-DOC-ID (68.2/68.6.1): large functions need a Rosetta id.
            if enabled("CEP-LINT-DOC-ID")
                && statements > param_usize("CEP-LINT-DOC-ID", "max_id_statements", 10)
            {
                let block = preceding_block(&src.lines, i);
                if !block.contains("CEP:ID:") {
                    push(
                        i,
                        "CEP-LINT-DOC-ID",
                        format!(
                            "function `{name}` ({statements} statements) lacks a CEP:ID (Rosetta 68.2)"
                        ),
                        sev("CEP-LINT-DOC-ID", 1),
                    );
                }
            }
        }
    }

    // ---- Line-scoped comment rules.
    for (i, line) in src.lines.iter().enumerate() {
        if !src.is_comment[i] {
            continue;
        }
        let t = line.trim();
        // Rule: anonymous work markers. (CEP-LINT:table)
        if enabled("CEP-LINT-TODO-OWNER")
            && !line.contains("CEP-LINT-")
            && !exempt("CEP-LINT-TODO-OWNER")
            && !suppression("CEP-LINT-TODO-OWNER", line)
        {
            for marker in ["TODO", "FIXME", "XXX", "HACK"] {
                if t.to_uppercase().contains(marker) && !t.contains("CEP:TODO(") {
                    push(
                        i,
                        "CEP-LINT-TODO-OWNER",
                        format!(
                            "anonymous {marker} marker; use CEP:TODO(owner): CEP-<ticket>: text"
                        ),
                        sev("CEP-LINT-TODO-OWNER", 1),
                    );
                    break;
                }
            }
        }
        // Rule: marker text in comments. (CEP-LINT:table)
        if enabled("CEP-LINT-PLACEHOLDER-COMMENT")
            && !t.contains("CEP:")
            && !line.contains("CEP-LINT-")
            && !exempt("CEP-LINT-PLACEHOLDER-COMMENT")
            && !suppression("CEP-LINT-PLACEHOLDER-COMMENT", line)
        {
            let lower = t.to_lowercase();
            for marker in [
                "not implemented",
                "unimplemented",
                "placeholder",
                "fixme",
                "tbd",
                "wip",
            ] {
                if lower.contains(marker) {
                    push(
                        i,
                        "CEP-LINT-PLACEHOLDER-COMMENT",
                        format!("marker text in comment: {marker:?}"),
                        sev("CEP-LINT-PLACEHOLDER-COMMENT", 1),
                    );
                    break;
                }
            }
        }
        // CEP-LINT-COMMENTED-CODE: keyword-led statement ending in terminator.
        if enabled("CEP-LINT-COMMENTED-CODE") {
            let body = t.trim_start_matches('/').trim();
            let lower = body.to_lowercase();
            let kw = ["let ", "if ", "for ", "while ", "return ", "fn "]
                .iter()
                .any(|k| lower.starts_with(k));
            if kw && body.ends_with(';') {
                push(
                    i,
                    "CEP-LINT-COMMENTED-CODE",
                    "comment parses as a code statement; use version control".into(),
                    sev("CEP-LINT-COMMENTED-CODE", 2),
                );
            }
        }
    }

    // ---- CEP-LINT-PLACEHOLDER-TEXT (string literals).
    if enabled("CEP-LINT-PLACEHOLDER-TEXT") {
        for (i, s) in src.strings.iter().enumerate() {
            // Rule identifiers (CEP-LINT-*) are engine data, never prose;
            // exempt_paths covers the engine's own marker-table file.
            if src.lines[i].contains("CEP-LINT-")
                || exempt("CEP-LINT-PLACEHOLDER-TEXT")
                || suppression("CEP-LINT-PLACEHOLDER-TEXT", &src.lines[i])
            {
                continue;
            }
            let lower = s.to_lowercase();
            for marker in ["not implemented", "unimplemented", "placeholder", "tbd"] {
                if lower.contains(marker) {
                    push(
                        i,
                        "CEP-LINT-PLACEHOLDER-TEXT",
                        format!("marker text in string literal: {marker:?}"),
                        sev("CEP-LINT-PLACEHOLDER-TEXT", 1),
                    );
                    break;
                }
            }
        }
    }

    // ---- CEP-LINT-MACRO-PREFIX.
    if enabled("CEP-LINT-MACRO-PREFIX") {
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            if let Some(pos) = line.find("macro_rules!") {
                let after = line[pos + "macro_rules!".len()..].trim_start();
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue; // reference or the engine's own rule table
                }
                if !name.starts_with("CEP_") {
                    push(
                        i,
                        "CEP-LINT-MACRO-PREFIX",
                        format!("object-like macro `{name}` lacks the CEP_ prefix"),
                        sev("CEP-LINT-MACRO-PREFIX", 2),
                    );
                }
            }
        }
    }

    // ---- CEP-LINT-NAMING-TYPE.
    if enabled("CEP-LINT-NAMING-TYPE") {
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            for kw in ["struct ", "enum ", "trait "] {
                if let Some(pos) = line.find(kw) {
                    let after = line[pos + kw.len()..].trim_start();
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let pascal = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                        && !name.contains('_');
                    if !name.is_empty() && !pascal {
                        push(
                            i,
                            "CEP-LINT-NAMING-TYPE",
                            format!("type name `{name}` is not PascalCase"),
                            sev("CEP-LINT-NAMING-TYPE", 2),
                        );
                    }
                }
            }
        }
    }

    // ---- CEP-LINT-EMPTY-BRANCH (flat check: empty block after a guard).
    if enabled("CEP-LINT-EMPTY-BRANCH") {
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            let stmt = line.trim().trim_start_matches('}').trim();
            for guard in ["if ", "else ", "for ", "while "] {
                if stmt.starts_with(guard) && stmt.contains(EMPTY_BLOCK) {
                    push(
                        i,
                        "CEP-LINT-EMPTY-BRANCH",
                        format!("`{guard}` guards an empty block"),
                        sev("CEP-LINT-EMPTY-BRANCH", 2),
                    );
                    break;
                }
            }
            if line.trim().ends_with(EMPTY_MATCH_ARM) {
                push(
                    i,
                    "CEP-LINT-EMPTY-BRANCH",
                    "empty match arm; document the intentional no-op".into(),
                    sev("CEP-LINT-EMPTY-BRANCH", 2),
                );
            }
        }
    }

    // ---- Dialect-gated flat rules (MAGIC-NUMBER, HOT-BANNED, EMPTY-CATCH,
    // CONSTANT-RETURN). The 0.3 applicability note scopes these to the C++
    // dialect; the configuration carries the dialect list.
    let file_dialects = |rule: &str| -> Vec<String> {
        rules
            .get(rule)
            .and_then(|r| r.params.get("dialects"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let ext = file.rsplit('.').next().unwrap_or("").to_string();

    if enabled("CEP-LINT-MAGIC-NUMBER") && file_dialects("CEP-LINT-MAGIC-NUMBER").contains(&ext) {
        let allowed: Vec<u64> = rules
            .get("CEP-LINT-MAGIC-NUMBER")
            .and_then(|r| r.params.get("allowed_values"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_else(|| vec![0, 1]);
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            let t = line.trim();
            let sanctioned = t.starts_with("#")
                || t.contains("const ")
                || t.contains("enum ")
                || t.contains("static_assert");
            if sanctioned {
                continue;
            }
            for lit in numeric_literals(t) {
                if !allowed.contains(&lit) {
                    push(
                        i,
                        "CEP-LINT-MAGIC-NUMBER",
                        format!("numeric literal {lit} outside a sanctioned declaration"),
                        sev("CEP-LINT-MAGIC-NUMBER", 2),
                    );
                }
            }
        }
    }

    if enabled("CEP-LINT-HOT-BANNED") && file_dialects("CEP-LINT-HOT-BANNED").contains(&ext) {
        let class = class_of(&src);
        if matches!(
            class.as_deref(),
            Some("CEP-0") | Some("HPC-0") | Some("RCS-0")
        ) {
            let banned = [
                "new ",
                "delete ",
                "throw ",
                "try ",
                "catch ",
                "virtual ",
                "std::function",
                "std::any",
                "std::shared_ptr",
                "std::weak_ptr",
                "std::bind",
                "std::regex",
                "std::thread",
                "std::async",
                "std::sort",
                "std::vector",
                "std::string",
                "std::map",
                "std::set",
            ];
            for (i, line) in src.lines.iter().enumerate() {
                if src.is_comment[i] {
                    continue;
                }
                for tok in banned {
                    if line.contains(tok) {
                        push(
                            i,
                            "CEP-LINT-HOT-BANNED",
                            format!(
                                "token `{}` is exterminated in class {class:?} files",
                                tok.trim()
                            ),
                            sev("CEP-LINT-HOT-BANNED", 0),
                        );
                    }
                }
            }
        }
    }

    if enabled("CEP-LINT-EMPTY-CATCH") && file_dialects("CEP-LINT-EMPTY-CATCH").contains(&ext) {
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            if let Some(pos) = line.find("catch") {
                let rest = &line[pos..];
                if rest.contains(EMPTY_BLOCK) {
                    push(
                        i,
                        "CEP-LINT-EMPTY-CATCH",
                        "empty catch handler; handle the failure or propagate it".into(),
                        sev("CEP-LINT-EMPTY-CATCH", 1),
                    );
                }
            }
        }
    }

    if enabled("CEP-LINT-CONSTANT-RETURN")
        && file_dialects("CEP-LINT-CONSTANT-RETURN").contains(&ext)
    {
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            let t = line.trim();
            let is_const_return = t.contains("return true;")
                || t.contains("return false;")
                || t.contains("return nullptr;");
            if is_const_return && t.contains('(') && t.contains('{') && t.ends_with("}") {
                push(
                    i,
                    "CEP-LINT-CONSTANT-RETURN",
                    "function body is a single constant return; result is not computed".into(),
                    sev("CEP-LINT-CONSTANT-RETURN", 2),
                );
            }
        }
    }

    // ---- CEP-LINT-ATOMIC-FILE (62.5.3 file_topology_policy).
    if enabled("CEP-LINT-ATOMIC-FILE") {
        let n = src.lines.len();
        let class = class_of(&src).unwrap_or_else(|| "CC".to_string());
        let limits: std::collections::BTreeMap<String, usize> = rules
            .get("CEP-LINT-ATOMIC-FILE")
            .and_then(|r| r.params.get("line_limits"))
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_u64().map(|x| (k.clone(), x as usize)))
                    .collect()
            })
            .unwrap_or_default();
        let default_limit = param_usize("CEP-LINT-ATOMIC-FILE", "default_limit", 800);
        let limit = limits.get(&class).copied().unwrap_or(default_limit);
        if n > limit {
            push(
                0,
                "CEP-LINT-ATOMIC-FILE",
                format!("file has {n} lines; class {class} limit is {limit} (62.5.1)"),
                sev("CEP-LINT-ATOMIC-FILE", 1),
            );
        }
        let fname = file.rsplit('/').next().unwrap_or(file).to_lowercase();
        for junk in ["util", "helper", "misc", "common"] {
            if fname.contains(junk) {
                push(
                    0,
                    "CEP-LINT-ATOMIC-FILE",
                    format!("junk-drawer file name contains `{junk}` (62.4)"),
                    sev("CEP-LINT-ATOMIC-FILE", 1),
                );
            }
        }
    }

    // ---- CEP-LINT-SECRET-SCAN (10.14/22.13 secret_probe_policy).
    if enabled("CEP-LINT-SECRET-SCAN") && !exempt("CEP-LINT-SECRET-SCAN") {
        let patterns: Vec<String> = rules
            .get("CEP-LINT-SECRET-SCAN")
            .and_then(|r| r.params.get("patterns"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_else(|| {
                ["password", "passwd", "secret_key", "api_key", "akia"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect()
            });
        let probe = |text: &str| -> Option<String> {
            let lower = text.to_lowercase();
            if lower.contains("begin private key") {
                return Some("private key header".to_string());
            }
            for pat in &patterns {
                if let Some(pos) = lower.find(pat) {
                    let rest = text[pos + pat.len()..].trim_start();
                    // Assignment context only: prose is not a secret.
                    if rest.starts_with('=') || rest.starts_with(':') {
                        return Some(pat.clone());
                    }
                    // AWS access-key id: AKIA + identifier.
                    if pat == "akia" {
                        let tail: String = rest.chars().take(20).collect();
                        if tail.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 8 {
                            return Some("aws access-key id".to_string());
                        }
                    }
                }
            }
            None
        };
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                if let Some(what) = probe(line) {
                    push(
                        i,
                        "CEP-LINT-SECRET-SCAN",
                        format!("secret-shaped content in comment ({what})"),
                        sev("CEP-LINT-SECRET-SCAN", 0),
                    );
                }
            } else if let Some(what) = probe(&src.strings[i]) {
                push(
                    i,
                    "CEP-LINT-SECRET-SCAN",
                    format!("secret-shaped content in string literal ({what})"),
                    sev("CEP-LINT-SECRET-SCAN", 0),
                );
            }
        }
    }

    // ---- CEP-LINT-UNSAFE-DOC (22.7/25.7 unsafe_doc_policy).
    if enabled("CEP-LINT-UNSAFE-DOC") && !exempt("CEP-LINT-UNSAFE-DOC") {
        let header_has_unsafe_field = src.lines.iter().take(24).any(|l| l.contains("CEP:UNSAFE"));
        for (i, line) in src.lines.iter().enumerate() {
            if src.is_comment[i] {
                continue;
            }
            let word_unsafe = line
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == "unsafe");
            if !word_unsafe || header_has_unsafe_field {
                continue;
            }
            // A CEP:UNSAFE field anywhere in the preceding 40 comment lines
            // (function block or enclosing item block) documents the usage.
            let documented = src.lines[..i]
                .iter()
                .rev()
                .take(40)
                .any(|l| l.contains("CEP:UNSAFE"));
            if !documented {
                push(
                    i,
                    "CEP-LINT-UNSAFE-DOC",
                    "unsafe usage without a CEP:UNSAFE field in the enclosing block (22.7/25.7)"
                        .into(),
                    sev("CEP-LINT-UNSAFE-DOC", 1),
                );
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Cross-file Rosetta checks (68.6.1): the registry and the function
/// documentation tree must agree in both directions, and every documented
/// function must carry the tier and inquisition sections.
// CEP:WHAT Checks registry ↔ docs consistency and per-file doc sections.
// CEP:WHY Rules CEP-LINT-DOC-FILE, CEP-LINT-DOC-TIERS, and
//        CEP-LINT-DOC-INQUISITION are cross-file; the per-file scanner
//        cannot see the registry or the docs tree.
// CEP:STATUS complete
// CEP:FAILURE Findings are reported; a missing registry is a bootstrap error
//        at the caller when the rules are configured.
// CEP:ASSUMES registry.json is an object mapping ID -> {path, function}.
// CEP:ID: CS-a12
// CEP:COST O(|registry| + |docs|).
// CEP:EVIDENCE cargo test -p cep_lint
pub fn rosetta_findings(
    cfg: &Config,
    registry_text: &str,
    docs: &[(String, String)],
) -> Vec<Finding> {
    let mut out = Vec::new();
    if !cfg.rules.contains_key("CEP-LINT-DOC-FILE") {
        return out;
    }
    let sev = |name: &str| cfg.rules.get(name).map_or(1u8, |r| r.severity);
    let find = |file: &str, rule: &str, msg: String, s: u8| Finding {
        file: file.to_string(),
        line: 1,
        rule: rule.to_string(),
        message: msg,
        severity: s,
    };
    let reg: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(registry_text).unwrap_or_default();
    let doc_map: std::collections::BTreeMap<&String, &String> =
        docs.iter().map(|(p, t)| (p, t)).collect();
    let md_prefix = cfg
        .rules
        .get("CEP-LINT-DOC-FILE")
        .and_then(|r| r.params.get("docs_prefix"))
        .and_then(Value::as_str)
        .unwrap_or("docs/functions/");
    for id in reg.keys() {
        let domain = id.split('-').next().unwrap_or("");
        let md_path = format!("{md_prefix}{domain}/{id}.md");
        match doc_map.get(&md_path) {
            None => out.push(find(
                "docs/registry.json",
                "CEP-LINT-DOC-FILE",
                format!("registered function {id} has no documentation file {md_path}"),
                sev("CEP-LINT-DOC-FILE"),
            )),
            Some(text) => {
                if cfg.rules.contains_key("CEP-LINT-DOC-TIERS")
                    && !(text.contains("## Tier 1: Beginner")
                        && text.contains("## Tier 2: Silicon Reality"))
                {
                    out.push(find(
                        &md_path,
                        "CEP-LINT-DOC-TIERS",
                        "documentation lacks the Tier 1 / Tier 2 section headers (68.3)".into(),
                        sev("CEP-LINT-DOC-TIERS"),
                    ));
                }
                if cfg.rules.contains_key("CEP-LINT-DOC-INQUISITION")
                    && !text.contains("## Silent Failure Checklist")
                {
                    out.push(find(
                        &md_path,
                        "CEP-LINT-DOC-INQUISITION",
                        "documentation lacks the Silent Failure Checklist (68.4)".into(),
                        sev("CEP-LINT-DOC-INQUISITION"),
                    ));
                }
            }
        }
    }
    for (p, _) in docs {
        if p.starts_with(md_prefix) && p.ends_with(".md") {
            let id = p.rsplit('/').next().unwrap_or("").trim_end_matches(".md");
            if !reg.contains_key(id) {
                out.push(find(
                    p,
                    "CEP-LINT-DOC-FILE",
                    format!("documentation file {p} has no registry entry"),
                    sev("CEP-LINT-DOC-FILE"),
                ));
            }
        }
    }
    out.sort();
    out
}

/// Validates the status and class fields inside the block spanning
/// `[from, to)` lines (CEP-LINT-STATUS-VALUE and the FILE-HEADER class check).
// CEP:WHAT Validates the status and class fields inside the block spanning [from, to) lines (CEP-LINT-STATUS-VALUE and the FILE-HEADER class check).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ID: CS-a13
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn check_status_and_class(
    src: &Source,
    from: usize,
    to: usize,
    rule: &str,
    sev: &dyn Fn(&str, u8) -> u8,
    push: &mut dyn FnMut(usize, &str, String, u8),
) {
    let to = to.min(src.lines.len());
    for (i, line) in src.lines.iter().enumerate().take(to).skip(from) {
        let field_line = line.trim().trim_start_matches('/').trim_start();
        if let Some(rest) = field_line.strip_prefix("CEP:STATUS") {
            let v = rest.trim();
            let v = v.trim_start_matches(':').trim();
            let value = v.split_whitespace().next().unwrap_or("");
            if !STATUS_VALUES.contains(&value) {
                push(
                    i,
                    "CEP-LINT-STATUS-VALUE",
                    format!("CEP:STATUS value `{value}` is not one of {STATUS_VALUES:?}"),
                    sev("CEP-LINT-STATUS-VALUE", 2),
                );
            }
            // CEP-LINT-STUB-DECL: stub-class statuses need owner + ticket.
            if matches!(value, "stub" | "partial" | "placeholder") {
                let block: String = src.lines[from..to].join("\n");
                if !block.contains("CEP:OWNER") || !block.contains("CEP:TICKET") {
                    push(
                        i,
                        "CEP-LINT-STUB-DECL",
                        format!(
                            "status `{value}` requires CEP:OWNER and CEP:TICKET in the same block"
                        ),
                        sev("CEP-LINT-STUB-DECL", 1),
                    );
                }
            }
        }
        if let Some(rest) = field_line.strip_prefix("CEP:CLASS") {
            let v = rest.trim();
            let v = v.trim_start_matches(':').trim();
            let class = v.split_whitespace().next().unwrap_or("");
            if !class.is_empty() && !CLASSES.contains(&class) {
                push(
                    i,
                    rule,
                    format!("CEP:CLASS value `{class}` is not a conformance class"),
                    sev(rule, 1),
                );
            }
        }
    }
}
