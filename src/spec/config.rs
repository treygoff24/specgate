use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Project-level configuration parsed from `specgate.config.yml`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct SpecConfig {
    /// Directories to search for `.spec.yml` files.
    #[serde(default = "default_spec_dirs")]
    pub spec_dirs: Vec<String>,
    /// Glob patterns excluded from analysis and discovery.
    #[serde(default = "default_excludes")]
    pub exclude: Vec<String>,
    /// Patterns treated as test files.
    #[serde(default = "default_test_patterns")]
    pub test_patterns: Vec<String>,
    /// Directory names re-included when they also match the default excluded directory list.
    #[serde(default)]
    pub include_dirs: Vec<String>,
    /// Escape hatch governance settings.
    #[serde(default)]
    pub escape_hatches: EscapeHatchConfig,
    /// `jest.mock` extraction mode.
    #[serde(default)]
    pub jest_mock_mode: JestMockMode,
    /// Policy for stale baseline entries discovered during check runs.
    ///
    /// Accepts `stale_baseline` (preferred) and `stale_baseline_policy` (compat alias).
    #[serde(default, alias = "stale_baseline_policy")]
    pub stale_baseline: StaleBaselinePolicy,
    /// Release channel controls stable/beta behavior gates.
    #[serde(default)]
    pub release_channel: ReleaseChannel,
    /// Telemetry opt-in flag (disabled by default).
    ///
    /// Backward-compatible with both:
    /// - `telemetry: true|false` (preferred)
    /// - `telemetry: { enabled: true|false }` (legacy)
    #[serde(default, deserialize_with = "deserialize_telemetry")]
    pub telemetry: bool,
    /// Whether type-only imports are enforced by dependency and boundary policy rules.
    #[serde(default = "default_enforce_type_only_imports")]
    pub enforce_type_only_imports: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default, PartialEq, Eq)]
pub struct EscapeHatchConfig {
    /// Maximum newly introduced ignores in a diff (`None` means unlimited).
    #[serde(default)]
    pub max_new_per_diff: Option<usize>,
    /// Whether all ignores must carry an expiry date.
    #[serde(default)]
    pub require_expiry: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum JestMockMode {
    #[default]
    Warn,
    Enforce,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StaleBaselinePolicy {
    #[default]
    Warn,
    Fail,
}

impl StaleBaselinePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TelemetryConfigCompat {
    Bool(bool),
    Object {
        #[serde(default)]
        enabled: bool,
    },
}

fn deserialize_telemetry<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let parsed = TelemetryConfigCompat::deserialize(deserializer)?;
    Ok(match parsed {
        TelemetryConfigCompat::Bool(enabled) => enabled,
        TelemetryConfigCompat::Object { enabled } => enabled,
    })
}

fn default_spec_dirs() -> Vec<String> {
    vec![".".to_string()]
}

pub const DEFAULT_EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    ".git",
    "generated",
    "target",
    "coverage",
    "vendor",
];

fn default_excludes() -> Vec<String> {
    DEFAULT_EXCLUDED_DIRS
        .iter()
        .map(|name| format!("**/{name}/**"))
        .collect()
}

pub fn normalize_dir_token(raw: &str) -> Option<String> {
    let normalized = raw.trim().trim_matches('/').trim();
    if normalized.is_empty() {
        return None;
    }

    let token = normalized.rsplit('/').next().unwrap_or(normalized);
    if token.is_empty() {
        return None;
    }

    Some(token.to_string())
}

pub fn include_dir_set(include_dirs: &[String]) -> BTreeSet<String> {
    include_dirs
        .iter()
        .filter_map(|entry| normalize_dir_token(entry))
        .collect()
}

fn default_test_patterns() -> Vec<String> {
    vec![
        "**/*.test.ts".to_string(),
        "**/*.test.tsx".to_string(),
        "**/*.spec.ts".to_string(),
        "**/*.spec.tsx".to_string(),
        "**/__tests__/**".to_string(),
        "**/__mocks__/**".to_string(),
    ]
}

fn default_enforce_type_only_imports() -> bool {
    false
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            spec_dirs: default_spec_dirs(),
            exclude: default_excludes(),
            test_patterns: default_test_patterns(),
            include_dirs: Vec::new(),
            escape_hatches: EscapeHatchConfig::default(),
            jest_mock_mode: JestMockMode::Warn,
            stale_baseline: StaleBaselinePolicy::Warn,
            release_channel: ReleaseChannel::Stable,
            telemetry: false,
            enforce_type_only_imports: default_enforce_type_only_imports(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_stable() {
        let config = SpecConfig::default();
        assert_eq!(config.spec_dirs, vec!["."]);
        assert_eq!(config.jest_mock_mode, JestMockMode::Warn);
        assert_eq!(config.stale_baseline, StaleBaselinePolicy::Warn);
        assert_eq!(config.release_channel, ReleaseChannel::Stable);
        assert!(!config.telemetry);
        assert!(!config.enforce_type_only_imports);
        assert!(config.include_dirs.is_empty());
        assert!(config.exclude.iter().any(|g| g == "**/node_modules/**"));
        assert!(config.exclude.iter().any(|g| g == "**/target/**"));
        assert!(config.exclude.iter().any(|g| g == "**/coverage/**"));
        assert!(config.exclude.iter().any(|g| g == "**/vendor/**"));
    }

    #[test]
    fn config_parses_stale_policy_and_telemetry_overrides() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
stale_baseline: fail
release_channel: beta
telemetry:
  enabled: true
"#,
        )
        .expect("parse config");

        assert_eq!(parsed.stale_baseline, StaleBaselinePolicy::Fail);
        assert_eq!(parsed.release_channel, ReleaseChannel::Beta);
        assert!(parsed.telemetry);
        assert!(!parsed.enforce_type_only_imports);
    }

    #[test]
    fn config_parses_type_only_enforcement_toggle() {
        let parsed: SpecConfig =
            yaml_serde::from_str("enforce_type_only_imports: true\n").expect("parse config");
        assert!(parsed.enforce_type_only_imports);
    }

    #[test]
    fn config_parses_include_dirs() {
        let parsed: SpecConfig =
            yaml_serde::from_str("include_dirs:\n  - vendor\n").expect("parse config");
        assert_eq!(parsed.include_dirs, vec!["vendor"]);
    }

    #[test]
    fn config_accepts_legacy_stale_baseline_policy_alias() {
        let parsed: SpecConfig =
            yaml_serde::from_str("stale_baseline_policy: fail\n").expect("parse config");
        assert_eq!(parsed.stale_baseline, StaleBaselinePolicy::Fail);
    }

    #[test]
    fn config_accepts_boolean_telemetry_short_form() {
        let parsed: SpecConfig = yaml_serde::from_str("telemetry: true\n").expect("parse config");
        assert!(parsed.telemetry);
    }

    #[test]
    fn config_serialization_includes_new_surfaces_deterministically() {
        let rendered = serde_json::to_string(&SpecConfig::default()).expect("serialize config");

        assert!(rendered.contains("\"stale_baseline\":\"warn\""));
        assert!(rendered.contains("\"release_channel\":\"stable\""));
        assert!(rendered.contains("\"telemetry\":false"));
        assert!(rendered.contains("\"enforce_type_only_imports\":false"));
        assert!(rendered.contains("\"include_dirs\":[]"));
    }
}
