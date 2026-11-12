// CEP:FILE tools/cep_lint/src/receipts.rs
// CEP:WHAT The chapter 65 and 67 receipt gates: cryptographic verification of
//        the Hunt Receipt (65.4) and the HUNTER Receipt (67.3), diff-to-function
//        attribution, and quota enforcement (65.5, 67.5).
// CEP:WHY The Bloodhound Protocol is Severity 0: a commit that touches CEP-0,
//        HPC-0, or RCS-0 functions without a signed receipt is blocked. This
//        module is the mechanical enforcement of that contract.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Every verification failure becomes a typed `ReceiptError` or a
//        `Finding` at severity 0; the module never accepts an unverified or
//        tampered receipt silently.
// CEP:ASSUMES The receipt files live at `.cep/hunt/receipt.json` and
//        `.cep/hunter/receipt.json` inside the repository; the trusted public
//        key is hex-encoded at `.cep/keys/ci_runner.pub`; the signature is
//        hex-encoded ed25519 over the canonical JSON of the receipt with the
//        `signature` field removed (serde_json BTreeMap key order).
// CEP:COST O(diff bytes) for attribution and O(1) signature verification.
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs covers every failure path.

use serde_json::Value;

/// Path of the Hunt Receipt inside the repository artifacts (65.4).
pub const HUNT_RECEIPT_PATH: &str = ".cep/hunt/receipt.json";
/// Path of the HUNTER Receipt inside the repository artifacts (67.3).
pub const HUNTER_RECEIPT_PATH: &str = ".cep/hunter/receipt.json";
/// Path of the trusted CI runner public key (65.4.2).
pub const CI_PUBLIC_KEY_PATH: &str = ".cep/keys/ci_runner.pub";

/// Minimum fuzz executions per target (65.3.1 quota).
pub const MIN_FUZZ_EXECUTIONS: u64 = 100_000;
/// Minimum differential inputs per target (65.3.2 quota).
pub const MIN_DIFF_INPUTS: u64 = 10_000;

/// Receipt verification failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptError {
    /// The receipt file is missing or unreadable.
    Missing,
    /// The receipt is not valid JSON or lacks a required field.
    Malformed(String),
    /// The signature does not verify against the trusted public key.
    Signature,
    /// The receipt's commit hash does not match the repository HEAD.
    CommitHash(String),
    /// A quota or cross-reference check failed; the string explains which.
    Policy(String),
}

/// One modified function attributed from the diff.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModifiedFunction {
    /// Repository-relative file path.
    pub file: String,
    /// Function name from the enclosing `fn` definition.
    pub function: String,
}

// CEP:WHAT hex_decode converts an ASCII hex string to bytes.
// CEP:WHY Keys and signatures arrive hex-encoded in the receipt artifacts.
// CEP:STATUS complete
// CEP:FAILURE Returns None on odd length or non-hex characters.
// CEP:ASSUMES Whitespace between hex digits is ignored.
// CEP:COST O(input bytes).
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs
/// Hex-decodes an ASCII hex string; empty input decodes to empty output.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !t.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(t.len() / 2);
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

/// Canonical bytes of a receipt: the JSON re-serialized with the `signature`
/// field removed. Both signer and verifier derive the same bytes because
/// serde_json emits object keys in lexicographic order.
fn canonical_bytes(value: &Value) -> Vec<u8> {
    let mut v = value.clone();
    if let Value::Object(map) = &mut v {
        map.remove("signature");
    }
    serde_json::to_vec(&v).unwrap_or_default()
}

/// Verifies the ed25519 signature of a receipt against the trusted public key.
// CEP:WHAT Verifies an ed25519 receipt signature against the CI public key.
// CEP:WHY 65.4.2: the receipt is proof only when the trusted key verifies it;
//        this is what stops hand-edited execution counts.
// CEP:STATUS complete
// CEP:FAILURE Returns `ReceiptError::Signature` on any verification failure and
//        `ReceiptError::Missing` when the key or signature material is absent.
// CEP:ASSUMES The public key file holds 64 hex characters (32 bytes); the
//        signature field holds 128 hex characters (64 bytes).
// CEP:COST O(1) verification.
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs signature cases.
pub fn verify_signature(value: &Value, root: &std::path::Path) -> Result<(), ReceiptError> {
    let key_text = std::fs::read_to_string(root.join(CI_PUBLIC_KEY_PATH))
        .map_err(|_| ReceiptError::Missing)?;
    let key_bytes =
        hex_decode(&key_text).ok_or(ReceiptError::Malformed("public key hex".into()))?;
    if key_bytes.len() != 32 {
        return Err(ReceiptError::Malformed("public key length".into()));
    }
    let sig_text = value
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or(ReceiptError::Missing)?;
    let sig_bytes = hex_decode(sig_text).ok_or(ReceiptError::Malformed("signature hex".into()))?;
    if sig_bytes.len() != 64 {
        return Err(ReceiptError::Malformed("signature length".into()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&key_bytes);
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&key)
        .map_err(|_| ReceiptError::Malformed("public key".into()))?;
    let sig = ed25519_dalek::Signature::from_slice(&sig_bytes)
        .map_err(|_| ReceiptError::Malformed("signature".into()))?;
    use ed25519_dalek::Verifier;
    vk.verify(&canonical_bytes(value), &sig)
        .map_err(|_| ReceiptError::Signature)
}

/// Extracts modified functions from a unified diff over CEP-0/HPC-0/RCS-0 files.
// CEP:WHAT Attributes unified-diff hunks to enclosing Rust functions.
// CEP:WHY 65.5 step 1 and 67.5 step 1: the receipt must cover exactly the
//        functions the commit modified.
// CEP:STATUS complete
// CEP:FAILURE Hunks outside any function are attributed to the file itself
//        with the function name `<module>`; malformed hunks are skipped.
// CEP:ASSUMES The diff is `git diff --unified=0` output over the target files;
//        file classes come from the current working-tree headers.
// CEP:COST O(diff bytes + file bytes) per modified file.
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs attribution cases.
pub fn modified_functions(
    diff_text: &str,
    file_contents: &[(String, String)],
) -> Vec<ModifiedFunction> {
    let hot: Vec<(String, String)> = file_contents
        .iter()
        .filter(|(_, c)| file_class_is_hot(c))
        .cloned()
        .collect();
    let mut out: Vec<ModifiedFunction> = Vec::new();
    let mut current_file: Option<String> = None;
    let mut pending: Vec<(usize, usize)> = Vec::new();
    for line in diff_text.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            flush_file(&mut out, &hot, &mut current_file, &mut pending);
            current_file = Some(rest.trim().to_string());
        } else if line.starts_with("@@") {
            if let Some(new_range) = parse_hunk_new_range(line) {
                pending.push(new_range);
            }
        }
    }
    flush_file(&mut out, &hot, &mut current_file, &mut pending);
    out.sort();
    out.dedup();
    out
}

/// Emits attributions for one file and resets the pending state.
fn flush_file(
    out: &mut Vec<ModifiedFunction>,
    hot: &[(String, String)],
    current_file: &mut Option<String>,
    pending: &mut Vec<(usize, usize)>,
) {
    if let Some(file) = current_file.take() {
        if !pending.is_empty() {
            if let Some((_, content)) = hot.iter().find(|(f, _)| *f == file) {
                for (start, count) in pending.drain(..) {
                    if count == 0 {
                        continue;
                    }
                    let fname = enclosing_function(content, start, start + count);
                    out.push(ModifiedFunction {
                        file: file.clone(),
                        function: fname,
                    });
                }
            }
        }
        pending.clear();
    }
}

/// Parses the new-file range from a unified diff hunk header.
fn parse_hunk_new_range(header: &str) -> Option<(usize, usize)> {
    let after = header.split("+").nth(1)?;
    let range = after.split(" @@").next()?;
    let (start, count) = match range.split_once(',') {
        Some((s, c)) => (s.parse::<usize>().ok()?, c.parse::<usize>().ok()?),
        None => (range.parse::<usize>().ok()?, 1),
    };
    Some((start, count))
}

/// Reports whether a file's CEP header class is hot (CEP-0/HPC-0/RCS-0).
fn file_class_is_hot(content: &str) -> bool {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("// CEP:CLASS:") {
            let c = t.trim_start_matches("// CEP:CLASS:").trim();
            return c == "CEP-0" || c == "HPC-0" || c == "RCS-0";
        }
        if !t.is_empty() && !t.starts_with("//") {
            return false;
        }
    }
    false
}

// CEP:WHAT enclosing_function maps a line range to the Rust function containing it.
// CEP:WHY 65.5 step 1 attributes diff hunks to functions; the receipt must name them.
// CEP:STATUS complete
// CEP:FAILURE Returns `<module>` when the range falls outside every function extent.
// CEP:ASSUMES Function extents come from brace matching over the current file.
// CEP:COST O(file bytes) per call.
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs
/// Finds the enclosing function name for a 1-based line range.
fn enclosing_function(content: &str, start: usize, end: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut best: Option<(usize, String)> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if let Some(idx) = t.find("fn ") {
            let before_ok = idx == 0 || t[..idx].chars().last().is_some_and(|c| c.is_whitespace());
            if !before_ok {
                continue;
            }
            let rest = &t[idx + 3..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let line_no = i + 1;
            let extent_end = function_extent_end(&lines, i);
            let name_ok = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            let in_range = line_no <= end && extent_end >= start;
            let later = best.as_ref().map(|(b, _)| line_no > *b).unwrap_or(true);
            if in_range && name_ok && later {
                best = Some((line_no, name.to_string()));
            }
        }
    }
    best.map(|(_, n)| n)
        .unwrap_or_else(|| "<module>".to_string())
}

// CEP:WHAT function_extent_end returns the last line of the brace block at a line.
// CEP:WHY Function attribution needs extents, not just signature lines.
// CEP:STATUS complete
// CEP:FAILURE Returns the file length for unterminated blocks (defensive bound).
// CEP:ASSUMES None.
// CEP:COST O(block bytes).
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs
/// Returns the last line of the brace block starting at line `start` (0-based).
fn function_extent_end(lines: &[&str], start: usize) -> usize {
    let mut depth: i64 = 0;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(start) {
        for c in l.chars() {
            if c == '{' {
                depth += 1;
                opened = true;
            }
            if c == '}' {
                depth -= 1;
            }
        }
        if opened && depth <= 0 {
            return i + 1;
        }
        if i > start + 5000 {
            break;
        }
    }
    lines.len()
}

/// Verifies the Hunt Receipt (65.5): signature, coverage, and quotas.
// CEP:WHAT Verifies the Hunt Receipt against the modified functions and quotas.
// CEP:WHY Rule CEP-LINT-HUNT-RECEIPT (severity 0): no signed, sufficient hunt,
//        no merge.
// CEP:STATUS complete
// CEP:FAILURE Returns one or more findings at the configured severity; a
//        missing receipt is one finding, not a crash.
// CEP:ASSUMES `diff_text` is the unified diff of the commit under review;
//        `file_contents` holds current working-tree contents.
// CEP:COST O(diff bytes + receipt bytes).
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs hunt cases.
pub fn verify_hunt_receipt(
    root: &std::path::Path,
    diff_text: &str,
    file_contents: &[(String, String)],
) -> Result<(), Vec<(ReceiptError, String)>> {
    let text = std::fs::read_to_string(root.join(HUNT_RECEIPT_PATH))
        .map_err(|_| vec![(ReceiptError::Missing, HUNT_RECEIPT_PATH.to_string())])?;
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        vec![(
            ReceiptError::Malformed(e.to_string()),
            HUNT_RECEIPT_PATH.to_string(),
        )]
    })?;
    verify_signature(&value, root).map_err(|e| vec![(e, "signature".to_string())])?;
    let mut errors: Vec<(ReceiptError, String)> = Vec::new();
    if let Some(commit) = value.get("commit_hash").and_then(|v| v.as_str()) {
        let head = git_head(root);
        if let Some(head) = head {
            if !commit.is_empty() && !head.starts_with(commit) {
                errors.push((ReceiptError::CommitHash(commit.to_string()), head));
            }
        }
    }
    let modified = modified_functions(diff_text, file_contents);
    let empty: Vec<Value> = Vec::new();
    let targets = value
        .get("targets")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    for mf in &modified {
        let entry = targets.iter().find(|t| {
            t.get("file").and_then(|f| f.as_str()) == Some(mf.file.as_str())
                && t.get("function").and_then(|f| f.as_str()) == Some(mf.function.as_str())
        });
        if entry.is_none() {
            errors.push((
                ReceiptError::Policy("coverage".into()),
                format!("{}::{}", mf.file, mf.function),
            ));
            continue;
        }
        if let Some(entry) = entry {
            let fuzz = entry
                .get("fuzz_executions")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let inputs = entry
                .get("diff_inputs")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let divergences = entry
                .get("divergences")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            if fuzz < MIN_FUZZ_EXECUTIONS {
                errors.push((
                    ReceiptError::Policy("fuzz_executions".into()),
                    format!("{}: {fuzz} < {MIN_FUZZ_EXECUTIONS}", mf.function),
                ));
            }
            if inputs < MIN_DIFF_INPUTS {
                errors.push((
                    ReceiptError::Policy("diff_inputs".into()),
                    format!("{}: {inputs} < {MIN_DIFF_INPUTS}", mf.function),
                ));
            }
            if divergences != 0 {
                errors.push((
                    ReceiptError::Policy("divergences".into()),
                    format!("{}: {divergences} != 0", mf.function),
                ));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Verifies the HUNTER Receipt (67.5): signature, coverage, unproven paths.
// CEP:WHAT Verifies the HUNTER Receipt against the modified functions.
// CEP:WHY Rule CEP-LINT-HUNTER-RECEIPT (severity 0): unproven hot paths block
//        the merge (67.4 makes silence a defect).
// CEP:STATUS complete
// CEP:FAILURE Returns findings for missing receipts, uncovered functions, and
//        any nonzero `unproven_paths`.
// CEP:ASSUMES Same receipt layout as the Hunt Receipt.
// CEP:COST O(diff bytes + receipt bytes).
// CEP:EVIDENCE tools/cep_lint/tests/receipts.rs hunter cases.
pub fn verify_hunter_receipt(
    root: &std::path::Path,
    diff_text: &str,
    file_contents: &[(String, String)],
) -> Result<(), Vec<(ReceiptError, String)>> {
    let text = std::fs::read_to_string(root.join(HUNTER_RECEIPT_PATH))
        .map_err(|_| vec![(ReceiptError::Missing, HUNTER_RECEIPT_PATH.to_string())])?;
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        vec![(
            ReceiptError::Malformed(e.to_string()),
            HUNTER_RECEIPT_PATH.to_string(),
        )]
    })?;
    verify_signature(&value, root).map_err(|e| vec![(e, "signature".to_string())])?;
    let modified = modified_functions(diff_text, file_contents);
    let analyzed = value
        .get("functions_analyzed")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let unproven = value
        .get("unproven_paths")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX);
    let mut errors: Vec<(ReceiptError, String)> = Vec::new();
    for mf in &modified {
        let covered = analyzed.iter().any(|a| {
            a.as_str().is_some_and(|s| {
                s.ends_with(&mf.function) || s == format!("{}::{}", mf.file, mf.function)
            })
        });
        if !covered {
            errors.push((
                ReceiptError::Policy("coverage".into()),
                format!("{}::{}", mf.file, mf.function),
            ));
        }
    }
    if unproven != 0 {
        errors.push((
            ReceiptError::Policy("unproven_paths".into()),
            format!("{unproven} != 0"),
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Reads the current HEAD hash; `None` outside a git repository.
fn git_head(root: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", root.to_str()?, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}
