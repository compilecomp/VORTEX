//! CEP&CC 0.4 mechanical lint engine (standard.md chapter 50).
// CEP:FILE tools/cep_lint/src/lib.rs
// CEP:WHAT cep_lint library root: configuration, rule engine, and re-exports
//        for tests and the binary.
// CEP:WHY The checkers must be testable as a library; the binary is a thin
//        CLI over this API.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE See per-module docs.
// CEP:ASSUMES None.
// CEP:COST Library crate.
// CEP:EVIDENCE cargo test -p cep_lint

/// Configuration schema and loader.
pub mod config;
/// Receipt gates (chapters 65 and 67).
pub mod receipts;
/// Rule engine and checkers.
pub mod rules;

pub use config::Config;
pub use rules::{glob_match, scan_file, Finding};
