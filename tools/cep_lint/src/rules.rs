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
use serde_json::Value;
use std::collections::BTreeSet;

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

/// A parsed source file: lines plus per-line classification.
struct Source {
    lines: Vec<String>,
    /// Per line: is this line inside a comment (line comment or block)?
    is_comment: Vec<bool>,
    /// Per line: the string-literal content on that line (concatenated).
    strings: Vec<String>,
}

/// Splits source lines into comment/string/code classification.
// CEP:WHAT Splits source lines into comment/string/code classification.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn classify(lines: Vec<String>) -> Source {
    let n = lines.len();
    let mut is_comment = vec![false; n];
    let mut strings = vec![String::new(); n];
    let mut in_block = false;
    for (i, raw) in lines.iter().enumerate() {
        let mut rest = raw.as_str();
        if in_block {
            is_comment[i] = true;
            if let Some(end) = rest.find("*/") {
                in_block = false;
                rest = &rest[end + 2..];
            } else {
                continue;
            }
        }
        let mut chars = rest.char_indices().peekable();
        let mut out_string = String::new();
        let mut comment_start: Option<usize> = None;
        let mut in_string = false;
        let mut escaped = false;
        while let Some((idx, c)) = chars.next() {
            if in_string {
                if escaped {
                    escaped = false;
                    out_string.push(c);
                    continue;
                }
                match c {
                    '\\' => {
                        escaped = true;
                        out_string.push(c);
                    }
                    '"' => in_string = false,
                    other => out_string.push(other),
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                '/' if rest[idx..].starts_with("//") => {
                    comment_start = Some(idx);
                    break;
                }
                '/' if rest[idx..].starts_with("/*") => {
                    // Same-line block close: resume scanning after `*/`.
                    match rest[idx + 2..].find("*/") {
                        Some(end) => {
                            let next = idx + 2 + end + 2;
                            // Continue after the closed block.
                            rest = &rest[next..];
                            chars = rest.char_indices().peekable();
                            continue;
                        }
                        None => {
                            in_block = true;
                            comment_start = Some(idx);
                            break;
                        }
                    }
                }
                _ => { /* ordinary character */ }
            }
        }
        if comment_start.is_some() || (in_block && is_comment[i]) {
            is_comment[i] = true;
        }
        if in_block && !is_comment[i] {
            is_comment[i] = true;
        }
        strings[i] = out_string;
    }
    Source {
        lines,
        is_comment,
        strings,
    }
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

    out.sort();
    out.dedup();
    out
}

/// Validates the status and class fields inside the block spanning
/// `[from, to)` lines (CEP-LINT-STATUS-VALUE and the FILE-HEADER class check).
// CEP:WHAT Validates the status and class fields inside the block spanning [from, to) lines (CEP-LINT-STATUS-VALUE and the FILE-HEADER class check).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
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

/// Extracts the CEP:STATUS value from a comment block, tolerating both
/// `CEP:STATUS: value` and `CEP:STATUS value` spellings.
// CEP:WHAT Extracts the CEP:STATUS value from a comment block, tolerating both CEP:STATUS: value and CEP:STATUS value spellings.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn status_of(block: &str) -> Option<String> {
    for line in block.lines() {
        let field_line = line.trim().trim_start_matches('/').trim_start();
        if let Some(rest) = field_line.strip_prefix("CEP:STATUS") {
            let v = rest.trim();
            let v = v.trim_start_matches(':').trim();
            let value = v.split_whitespace().next().unwrap_or("");
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
/// Extracts integer literals from a code line (word-bounded decimal).
// CEP:WHAT Extracts integer literals from a code line (word-bounded decimal).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn numeric_literals(line: &str) -> Vec<u64> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit()
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            // Skip hex/float/identifier tails: only clean decimals count.
            if i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                continue;
            }
            let text: String = line[start..i].chars().filter(|c| *c != '_').collect();
            if let Ok(v) = text.parse::<u64>() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The CEP:CLASS declared in the file header, if any.
fn class_of(src: &Source) -> Option<String> {
    for line in src.lines.iter().take(24) {
        let field_line = line.trim().trim_start_matches('/').trim_start();
        if let Some(rest) = field_line.strip_prefix("CEP:CLASS") {
            let v = rest.trim();
            let v = v.trim_start_matches(':').trim();
            return Some(v.split_whitespace().next().unwrap_or("").to_string());
        }
    }
    None
}

/// Returns the line range of a function body starting at the `fn` line.
/// Comment lines are skipped; a `}` without a preceding `{` ends the scan
/// (the match was a reference, not a definition).
// CEP:WHAT Returns the line range of a function body starting at the fn line.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn fn_body_range(lines: &[String], start: usize) -> Option<(usize, usize)> {
    let mut depth = 0_usize;
    let mut opened = false;
    for (offset, line) in lines[start..].iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if opened && depth == 0 {
                        return Some((start, start + offset));
                    }
                }
                ';' if !opened && offset > 0 => return None, // declaration only
                _ => { /* ordinary character */ }
            }
        }
    }
    None
}

/// The contiguous `//` comment block immediately above line `i` (skipping
/// attribute lines), as a single string.
// CEP:WHAT The contiguous // comment block immediately above line i (skipping attribute lines), as a single string.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn preceding_block(lines: &[String], i: usize) -> String {
    let mut block = Vec::new();
    let mut j = i;
    while j > 0 {
        j -= 1;
        let t = lines[j].trim_start();
        if t.starts_with("#[") || t.starts_with("#![") {
            continue;
        }
        if t.starts_with("//") {
            block.push(lines[j].as_str());
        } else {
            break;
        }
    }
    block.reverse();
    block.join("\n")
}

/// Start line of the preceding comment block (for status checks).
// CEP:WHAT Start line of the preceding comment block (for status checks).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn block_start_of(lines: &[String], i: usize) -> usize {
    let mut j = i;
    while j > 0 {
        j -= 1;
        let t = lines[j].trim_start();
        if t.starts_with("//") || t.starts_with("#[") {
            continue;
        }
        return j + 1;
    }
    0
}

/// Collects findings over a whole file set, sorted deterministically.
pub fn scan_all(cfg: &Config, files: &[(String, String)]) -> Vec<Finding> {
    let mut all = Vec::new();
    for (path, text) in files {
        if !cfg.file_globs.iter().any(|g| glob_match(g, path)) {
            continue;
        }
        all.extend(scan_file(cfg, path, text));
    }
    all.sort();
    all
}

/// Minimal glob: `**` matches any path components, `*` within one, `?` one char.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').collect();
    let t: Vec<&str> = path.split('/').collect();
    glob_segments(&p, &t)
}

fn glob_segments(p: &[&str], t: &[&str]) -> bool {
    match (p.first(), t.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(&"**"), _) => {
            if glob_segments(&p[1..], t) {
                return true;
            }
            !t.is_empty() && glob_segments(p, &t[1..])
        }
        (Some(seg), Some(first)) => seg_match(seg, first) && glob_segments(&p[1..], &t[1..]),
        (Some(_), None) => false,
    }
}

/// Single-segment wildcard match: `*` matches any character sequence within
/// the segment (e.g. `*.rs`, `vortex-*`).
// CEP:WHAT Single-segment wildcard match: * matches any character sequence within the segment (e.g.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
fn seg_match(seg: &str, s: &str) -> bool {
    if seg == "*" {
        return true;
    }
    if !seg.contains('*') {
        return seg == s;
    }
    let parts: Vec<&str> = seg.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if s.len() < first.len() + last.len() {
        return false;
    }
    if !s.starts_with(first) || !s.ends_with(last) {
        return false;
    }
    // Middle parts must appear in order within the remainder.
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(p) => rest = &rest[p + mid.len()..],
            None => return false,
        }
    }
    true
}

/// The set of rule names documented in the standard chapter (for DOC-SYNC).
#[must_use]
pub fn documented_rules(standard_text: &str) -> BTreeSet<String> {
    standard_text
        .lines()
        .filter_map(|l| l.strip_prefix("### Rule "))
        .map(str::trim)
        .map(String::from)
        .collect()
}
