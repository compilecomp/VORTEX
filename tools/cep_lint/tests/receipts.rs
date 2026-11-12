// CEP:FILE tools/cep_lint/tests/receipts.rs
// CEP:WHAT Receipt gate tests (chapters 65 and 67): signature verification,
//        diff attribution, and quota enforcement over temporary fixtures.
// CEP:WHY The severity 0 receipt rules must be verified as ordinary Rust
//        tests with a dedicated test keypair, not only through review.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The test keypair is distinct from the CI runner key.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint

#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Receipt gate test suite (chapters 65 and 67) over temporary fixtures.

use cep_lint::receipts::{
    verify_hunt_receipt, verify_hunter_receipt, ReceiptError, CI_PUBLIC_KEY_PATH,
};
use serde_json::{json, Value};

/// Fixed test seed (documented, test-only, low entropy by design).
const TEST_SEED: [u8; 32] = [7u8; 32];

/// Hex-encodes bytes.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Writes the test public key into a temporary repository root.
fn write_test_key(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".cep/keys")).unwrap();
    let sk = ed25519_dalek::SigningKey::from_bytes(&TEST_SEED);
    std::fs::write(
        root.join(CI_PUBLIC_KEY_PATH),
        hex(&sk.verifying_key().to_bytes()),
    )
    .unwrap();
}

/// Signs a receipt's canonical bytes and inserts the hex signature field.
// CEP:WHAT sign signs the canonical JSON and stores the hex signature.
// CEP:WHY The signer must mirror the verifier's canonicalization exactly.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
fn sign(value: &mut Value) {
    let sk = ed25519_dalek::SigningKey::from_bytes(&TEST_SEED);
    let mut v = value.clone();
    if let Value::Object(map) = &mut v {
        map.remove("signature");
    }
    use ed25519_dalek::Signer;
    let sig = sk.sign(&serde_json::to_vec(&v).unwrap());
    value["signature"] = json!(hex(&sig.to_bytes()));
}

/// A hot-file fixture with one function spanning lines 5 through 8.
const HOT_FILE: (&str, &str) = (
    "crates/vortex-nv1/src/lib.rs",
    "// CEP:FILE: crates/vortex-nv1/src/lib.rs\n// CEP:CLASS: CEP-0\n// hot header continues\n\nfn decode_opcode(x: u64) -> u64 {\n    let y = x + 1;\n    let z = y * 2;\n    z\n}\n",
);

/// A cold-file fixture that must be ignored by attribution.
const COLD_FILE: (&str, &str) = (
    "crates/vortex-cli/src/main.rs",
    "// CEP:FILE: crates/vortex-cli/src/main.rs\n// CEP:CLASS: CEP-2\n\nfn main() {\n    println!(\"x\");\n}\n",
);

fn contents() -> Vec<(String, String)> {
    vec![
        (HOT_FILE.0.to_string(), HOT_FILE.1.to_string()),
        (COLD_FILE.0.to_string(), COLD_FILE.1.to_string()),
    ]
}

/// A unified diff modifying line 6 of the hot file (inside decode_opcode).
fn hot_diff() -> String {
    format!(
        "--- a/{file}\n+++ b/{file}\n@@ -6,1 +6,1 @@\n-    let y = x + 1;\n+    let y = x + 2;\n",
        file = HOT_FILE.0
    )
}

/// A Hunt Receipt covering the modified function with sufficient quotas.
fn hunt_receipt(cover: bool, fuzz: u64, inputs: u64, divergences: u64) -> Value {
    let targets = if cover {
        json!([{
            "file": HOT_FILE.0,
            "function": "decode_opcode",
            "fuzz_executions": fuzz,
            "diff_inputs": inputs,
            "divergences": divergences
        }])
    } else {
        json!([])
    };
    json!({
        "receipt_id": "test-hunt-1",
        "commit_hash": "",
        "ci_runner_id": "test-runner",
        "targets": targets,
        "timestamp": "2026-11-13T00:00:00Z"
    })
}

/// A HUNTER Receipt for the modified function with the given unproven count.
fn hunter_receipt(cover: bool, unproven: u64) -> Value {
    let analyzed = if cover {
        json!(["decode_opcode"])
    } else {
        json!([])
    };
    json!({
        "engine": "HUNTER",
        "functions_analyzed": analyzed,
        "airlock_discharges_passed": 1,
        "hidden_cost_proofs": 0,
        "symbolic_paths_explored": 10,
        "unproven_paths": unproven
    })
}

// CEP:WHAT setup writes the test key and both signed receipts into the temp root.
// CEP:WHY Each test constructs an isolated repository layout.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
fn setup(root: &std::path::Path, hunt: Value, hunter: Value) {
    write_test_key(root);
    let mut h = hunt;
    sign(&mut h);
    let mut u = hunter;
    sign(&mut u);
    std::fs::create_dir_all(root.join(".cep/hunt")).unwrap();
    std::fs::create_dir_all(root.join(".cep/hunter")).unwrap();
    std::fs::write(
        root.join(".cep/hunt/receipt.json"),
        serde_json::to_string_pretty(&h).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(".cep/hunter/receipt.json"),
        serde_json::to_string_pretty(&u).unwrap(),
    )
    .unwrap();
}

// CEP:WHAT verified_receipts_pass_all_gates asserts a sufficient, signed pair passes both gates.
// CEP:WHY The happy path of rules CEP-LINT-HUNT-RECEIPT and CEP-LINT-HUNTER-RECEIPT.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn verified_receipts_pass_all_gates() {
    let dir = std::env::temp_dir().join("cep-receipt-test-ok");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(
        &dir,
        hunt_receipt(true, 150_000, 12_000, 0),
        hunter_receipt(true, 0),
    );
    let diff = hot_diff();
    assert!(verify_hunt_receipt(&dir, &diff, &contents()).is_ok());
    assert!(verify_hunter_receipt(&dir, &diff, &contents()).is_ok());
}

// CEP:WHAT tampered_receipt_fails_signature asserts an edited receipt fails signature verification.
// CEP:WHY 65.4.2: hand-edited execution counts must not verify.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn tampered_receipt_fails_signature() {
    let dir = std::env::temp_dir().join("cep-receipt-test-tamper");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = hunt_receipt(true, 150_000, 12_000, 0);
    sign(&mut h);
    h["targets"][0]["fuzz_executions"] = json!(999_999);
    std::fs::create_dir_all(dir.join(".cep/keys")).unwrap();
    std::fs::create_dir_all(dir.join(".cep/hunt")).unwrap();
    write_test_key(&dir);
    std::fs::write(
        dir.join(".cep/hunt/receipt.json"),
        serde_json::to_string(&h).unwrap(),
    )
    .unwrap();
    let err = verify_hunt_receipt(&dir, &hot_diff(), &contents()).unwrap_err();
    assert!(matches!(err[0].0, ReceiptError::Signature), "got {err:?}");
}

// CEP:WHAT insufficient_quota_fails asserts sub-quota fuzz executions are rejected.
// CEP:WHY 65.3.1: 100,000 executions are the minimum hunt.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn insufficient_quota_fails() {
    let dir = std::env::temp_dir().join("cep-receipt-test-quota");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(
        &dir,
        hunt_receipt(true, 500, 10_000, 0),
        hunter_receipt(true, 0),
    );
    let err = verify_hunt_receipt(&dir, &hot_diff(), &contents()).unwrap_err();
    assert!(err
        .iter()
        .any(|(e, _)| matches!(e, ReceiptError::Policy(p) if p == "fuzz_executions")));
}

// CEP:WHAT divergences_block_the_merge asserts nonzero divergences are rejected.
// CEP:WHY 65.3.2: divergences must be exactly zero.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn divergences_block_the_merge() {
    let dir = std::env::temp_dir().join("cep-receipt-test-divergence");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(
        &dir,
        hunt_receipt(true, 150_000, 12_000, 2),
        hunter_receipt(true, 0),
    );
    let err = verify_hunt_receipt(&dir, &hot_diff(), &contents()).unwrap_err();
    assert!(err
        .iter()
        .any(|(e, _)| matches!(e, ReceiptError::Policy(p) if p == "divergences")));
}

// CEP:WHAT uncovered_function_blocks asserts modified hot functions missing from both receipts are rejected.
// CEP:WHY 65.5 step 4 and 67.5 step 4 cross-references.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn uncovered_function_blocks() {
    let dir = std::env::temp_dir().join("cep-receipt-test-uncovered");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(
        &dir,
        hunt_receipt(false, 150_000, 12_000, 0),
        hunter_receipt(false, 0),
    );
    let diff = hot_diff();
    let hunt_err = verify_hunt_receipt(&dir, &diff, &contents()).unwrap_err();
    assert!(hunt_err
        .iter()
        .any(|(e, _)| matches!(e, ReceiptError::Policy(p) if p == "coverage")));
    let hunter_err = verify_hunter_receipt(&dir, &diff, &contents()).unwrap_err();
    assert!(hunter_err
        .iter()
        .any(|(e, _)| matches!(e, ReceiptError::Policy(p) if p == "coverage")));
}

// CEP:WHAT unproven_paths_block asserts nonzero unproven paths are rejected.
// CEP:WHY 67.4: silence is a defect; unproven paths block the merge.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn unproven_paths_block() {
    let dir = std::env::temp_dir().join("cep-receipt-test-unproven");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(
        &dir,
        hunt_receipt(true, 150_000, 12_000, 0),
        hunter_receipt(true, 3),
    );
    let err = verify_hunter_receipt(&dir, &hot_diff(), &contents()).unwrap_err();
    assert!(err
        .iter()
        .any(|(e, _)| matches!(e, ReceiptError::Policy(p) if p == "unproven_paths")));
}

// CEP:WHAT missing_receipt_is_one_finding asserts an absent Hunt Receipt yields exactly one missing finding.
// CEP:WHY The Yell Protocol fires once for a missing receipt, not per function.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn missing_receipt_is_one_finding() {
    let dir = std::env::temp_dir().join("cep-receipt-test-missing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_test_key(&dir);
    let err = verify_hunt_receipt(&dir, &hot_diff(), &contents()).unwrap_err();
    assert_eq!(err.len(), 1);
    assert!(matches!(err[0].0, ReceiptError::Missing));
}

// CEP:WHAT cold_files_are_never_attributed asserts diffs over CEP-2 files need no receipts.
// CEP:WHY Only CEP-0, HPC-0, and RCS-0 files are in the protocol scope.
// CEP:STATUS complete
// CEP:FAILURE Failing assertions abort the test binary.
// CEP:ASSUMES The fixtures in this file; the test keypair is written to a temp root.
// CEP:COST O(receipt bytes) per case.
// CEP:EVIDENCE cargo test -p cep_lint
#[test]
fn cold_files_are_never_attributed() {
    let dir = std::env::temp_dir().join("cep-receipt-test-cold");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    setup(&dir, hunt_receipt(false, 0, 0, 0), hunter_receipt(false, 0));
    // A diff touching only the cold file requires no targets at all.
    let diff = format!(
        "--- a/{file}\n+++ b/{file}\n@@ -3,1 +3,1 @@\n-    println!(\"x\");\n+    println!(\"y\");\n",
        file = COLD_FILE.0
    );
    assert!(verify_hunt_receipt(&dir, &diff, &contents()).is_ok());
    assert!(verify_hunter_receipt(&dir, &diff, &contents()).is_ok());
}
