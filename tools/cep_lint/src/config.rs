// CEP:FILE tools/cep_lint/src/config.rs
// CEP:WHAT Lint configuration schema and loader for .cep/cep_lint.json.
// CEP:WHY Chapter 50: rules, severities, file globs, and the standard version
//        are configuration data, not code.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Malformed configuration is a bootstrap failure (exit 2), never
//        a defaulting silent path.
// CEP:ASSUMES Single JSON document; unknown rule keys are DOC-SYNC failures.
// CEP:COST O(size of config) at startup.
// CEP:EVIDENCE cep_lint --self-test
use serde::Deserialize;

/// Top-level configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// The standard version this configuration enforces (DOC-SYNC).
    #[serde(rename = "standard.version")]
    pub standard_version: String,
    /// Glob patterns (prefix/path style) of first-party files to lint.
    pub file_globs: Vec<String>,
    /// Severities that fail the run (default 0 and 1).
    #[serde(default = "default_fail_on")]
    pub fail_on: Vec<u8>,
    /// Rule table.
    pub rules: std::collections::BTreeMap<String, Rule>,
}

fn default_fail_on() -> Vec<u8> {
    vec![0, 1]
}

/// One rule entry.
#[derive(Debug, Deserialize)]
pub struct Rule {
    /// Severity 0..=3 (chapter 50.2).
    pub severity: u8,
    /// Check primitive name (chapter 50.3 `Check:` lines).
    pub check: String,
    /// Rule parameters (checker-specific).
    #[serde(default)]
    pub params: serde_json::Value,
}

impl Config {
    /// Loads and validates a configuration document.
    ///
    /// # Errors
    /// Returns a message when the document is missing, unparseable, or
    /// schema-invalid (bootstrap error classes 1/2/4/5/7 of chapter 50.4).
    pub fn load(path: &std::path::Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "CEP-LINT-BOOT-1: cannot read config {}: {e}",
                path.display()
            )
        })?;
        let cfg: Config = serde_json::from_str(&text)
            .map_err(|e| format!("CEP-LINT-BOOT-2/5: invalid config schema: {e}"))?;
        for (name, rule) in &cfg.rules {
            if rule.severity > 3 {
                return Err(format!("CEP-LINT-BOOT-7: severity out of range for {name}"));
            }
        }
        Ok(cfg)
    }
}
