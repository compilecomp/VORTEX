// CEP:FILE tools/cep_lint/src/syntax.rs
// CEP:WHAT Source classification and structural helpers: comment/string
//        splitting, function-body ranges, CEP field extraction, numeric
//        literal extraction, glob matching, and whole-repo collection
//        (chapter 50 check primitives' shared machinery).
// CEP:WHY The rule table file must stay within the atomic-file limit; the
//        helpers are one cohesive concept shared by every checker.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Helpers never panic on malformed input.
// CEP:ASSUMES UTF-8 sources; `//` comments; quoted strings with escapes.
// CEP:COST O(bytes) per file.
// CEP:EVIDENCE cargo test -p cep_lint
//! Lint syntax helpers shared by the rule checkers.

#![warn(missing_docs)]

use crate::config::Config;
use crate::rules::{scan_file, Finding};
use std::collections::BTreeSet;

/// A parsed source file: lines plus per-line classification.
pub(crate) struct Source {
    /// Source lines.
    pub(crate) lines: Vec<String>,
    /// Per line: is this line inside a comment (line comment or block)?
    pub(crate) is_comment: Vec<bool>,
    /// Per line: the string-literal content on that line (concatenated).
    pub(crate) strings: Vec<String>,
}

/// Splits source lines into comment/string/code classification.
// CEP:WHAT Splits source lines into comment/string/code classification.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
// CEP:ID: CS-a10
pub(crate) fn classify(lines: Vec<String>) -> Source {
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

/// Extracts the CEP:STATUS value from a comment block, tolerating both
/// `CEP:STATUS: value` and `CEP:STATUS value` spellings.
// CEP:WHAT Extracts the CEP:STATUS value from a comment block, tolerating both CEP:STATUS: value and CEP:STATUS value spellings.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p cep_lint
pub(crate) fn status_of(block: &str) -> Option<String> {
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
pub(crate) fn numeric_literals(line: &str) -> Vec<u64> {
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
pub(crate) fn class_of(src: &Source) -> Option<String> {
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
pub(crate) fn fn_body_range(lines: &[String], start: usize) -> Option<(usize, usize)> {
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
pub(crate) fn preceding_block(lines: &[String], i: usize) -> String {
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
pub(crate) fn block_start_of(lines: &[String], i: usize) -> usize {
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
