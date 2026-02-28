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
    /// Telemetry configuration (opt-in, disabled by default).
    #[serde(default)]
    pub telemetry: TelemetryConfig,
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
pub struct TelemetryConfig {
    /// Enables telemetry emission when downstream callers opt in to publishing.
    #[serde(default)]
    pub enabled: bool,
}

fn default_spec_dirs() -> Vec<String> {
    vec![".".to_string()]
}

fn default_excludes() -> Vec<String> {
    vec![
        "**/node_modules/**".to_string(),
        "**/dist/**".to_string(),
        "**/build/**".to_string(),
        "**/.git/**".to_string(),
        "**/generated/**".to_string(),
    ]
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

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            spec_dirs: default_spec_dirs(),
            exclude: default_excludes(),
            test_patterns: default_test_patterns(),
            escape_hatches: EscapeHatchConfig::default(),
            jest_mock_mode: JestMockMode::Warn,
            stale_baseline: StaleBaselinePolicy::Warn,
            release_channel: ReleaseChannel::Stable,
            telemetry: TelemetryConfig::default(),
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
        assert!(!config.telemetry.enabled);
        assert!(config.exclude.iter().any(|g| g == "**/node_modules/**"));
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
        assert!(parsed.telemetry.enabled);
    }

    #[test]
    fn config_accepts_legacy_stale_baseline_policy_alias() {
        let parsed: SpecConfig =
            yaml_serde::from_str("stale_baseline_policy: fail\n").expect("parse config");
        assert_eq!(parsed.stale_baseline, StaleBaselinePolicy::Fail);
    }

    #[test]
    fn config_serialization_includes_new_surfaces_deterministically() {
        let rendered = serde_json::to_string(&SpecConfig::default()).expect("serialize config");

        assert!(rendered.contains("\"stale_baseline\":\"warn\""));
        assert!(rendered.contains("\"release_channel\":\"stable\""));
        assert!(rendered.contains("\"telemetry\":{\"enabled\":false}"));
    }
}
