use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::spec::types::Severity;

fn default_deep_import_max_depth() -> usize {
    0
}

/// Structured deep import policy entry.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DenyDeepImportEntry {
    pub pattern: String,
    #[serde(default = "default_deep_import_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub severity: Option<Severity>,
}

impl DenyDeepImportEntry {
    pub fn effective_severity(&self) -> Severity {
        self.severity.unwrap_or(Severity::Warning)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DenyDeepImportEntryCompat {
    Legacy(String),
    Structured(DenyDeepImportEntry),
}

fn deserialize_deny_deep_import_entries<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<DenyDeepImportEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let parsed = Option::<Vec<DenyDeepImportEntryCompat>>::deserialize(deserializer)?;
    Ok(parsed
        .unwrap_or_default()
        .into_iter()
        .map(|entry| match entry {
            DenyDeepImportEntryCompat::Legacy(pattern) => DenyDeepImportEntry {
                pattern,
                max_depth: default_deep_import_max_depth(),
                severity: None,
            },
            DenyDeepImportEntryCompat::Structured(entry) => entry,
        })
        .collect())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TestBoundaryMode {
    #[default]
    Off,
    ProductionOnly,
    Bidirectional,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct TestBoundaryConfigCompat {
    enabled: Option<bool>,
    mode: Option<TestBoundaryMode>,
    test_patterns: Vec<String>,
    deny_production_imports: Option<bool>,
}

/// Import hygiene rules for catching common agent mistakes.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
pub struct ImportHygieneConfig {
    /// Deny deep imports into third-party packages.
    ///
    /// Backward-compatible parsing accepts either legacy strings:
    /// `deny_deep_imports: ["express", "react"]`
    /// or structured entries:
    /// `deny_deep_imports: [{ pattern: "lodash/**", max_depth: 1 }]`
    #[serde(default, deserialize_with = "deserialize_deny_deep_import_entries")]
    pub deny_deep_imports: Vec<DenyDeepImportEntry>,

    /// Test-production boundary enforcement.
    #[serde(default)]
    pub test_boundary: TestBoundaryConfig,
}

/// Baseline-specific configuration.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
pub struct BaselineConfig {
    /// When true, baseline metadata gaps should fail auditing and `baseline add`
    /// requires `--owner` and `--reason`.
    #[serde(default)]
    pub require_metadata: bool,
}

/// Test-production boundary enforcement configuration.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
pub struct TestBoundaryConfig {
    /// Master enable switch for test boundary checks.
    #[serde(default)]
    pub enabled: bool,

    /// Which boundary directions are enforced when the rule is enabled.
    #[serde(default)]
    pub mode: TestBoundaryMode,

    /// Additional test file patterns beyond the global test_patterns.
    #[serde(default)]
    pub test_patterns: Vec<String>,
}

impl TestBoundaryConfig {
    pub fn effective_mode(&self) -> TestBoundaryMode {
        if self.enabled {
            self.mode
        } else {
            TestBoundaryMode::Off
        }
    }
}

impl Default for TestBoundaryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TestBoundaryMode::Off,
            test_patterns: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for TestBoundaryConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = TestBoundaryConfigCompat::deserialize(deserializer)?;
        let legacy_enabled = raw.deny_production_imports.unwrap_or(false);
        let enabled = raw.enabled.unwrap_or(
            legacy_enabled || raw.mode.is_some_and(|mode| mode != TestBoundaryMode::Off),
        );
        let mode = raw.mode.unwrap_or({
            if enabled || legacy_enabled {
                TestBoundaryMode::ProductionOnly
            } else {
                TestBoundaryMode::Off
            }
        });

        Ok(Self {
            enabled,
            mode,
            test_patterns: raw.test_patterns,
        })
    }
}

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
    /// Baseline configuration and governance.
    #[serde(default)]
    pub baseline: BaselineConfig,
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
    /// Filename of the TypeScript config file used for path resolution.
    #[serde(default = "default_tsconfig_filename")]
    pub tsconfig_filename: String,
    /// Envelope validation configuration for boundary contracts.
    #[serde(default)]
    pub envelope: EnvelopeConfig,
    /// Policy for unresolved import edges.
    #[serde(default)]
    pub unresolved_edge_policy: UnresolvedEdgePolicy,
    /// Import hygiene rules for catching common agent mistakes.
    #[serde(default)]
    pub import_hygiene: ImportHygieneConfig,
    /// When true, any ownership finding (overlapping, unclaimed, orphaned, or duplicate)
    /// causes a non-zero exit code.
    #[serde(default)]
    pub strict_ownership: bool,
    /// How strictly ownership findings gate runs when strict ownership is enabled.
    #[serde(default)]
    pub strict_ownership_level: StrictOwnershipLevel,
}

/// Envelope validation settings for contract enforcement.
/// When a boundary contract declares `envelope: required`, specgate performs
/// a targeted AST check on matched files to verify they import the envelope
/// package and call the validation function with the correct contract ID.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct EnvelopeConfig {
    /// Master switch to enable/disable envelope checking project-wide.
    /// When false, all `envelope: required` contracts are treated as optional.
    #[serde(default = "default_envelope_enabled")]
    pub enabled: bool,
    /// Package name(s) to look for in imports.
    /// Default: ["specgate-envelope"]
    #[serde(default = "default_envelope_import_patterns")]
    pub import_patterns: Vec<String>,
    /// Call expression pattern to match.
    /// Supports dot notation: "boundary.validate" matches `boundary.validate(...)`.
    /// Default: "boundary.validate"
    #[serde(default = "default_envelope_function_pattern")]
    pub function_pattern: String,
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
pub enum UnresolvedEdgePolicy {
    #[default]
    Warn,
    Error,
    Ignore,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Beta,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StrictOwnershipLevel {
    #[default]
    Errors,
    Warnings,
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

fn default_envelope_enabled() -> bool {
    true
}

fn default_envelope_import_patterns() -> Vec<String> {
    vec!["specgate-envelope".to_string()]
}

fn default_envelope_function_pattern() -> String {
    "boundary.validate".to_string()
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

fn default_tsconfig_filename() -> String {
    "tsconfig.json".to_string()
}

impl SpecConfig {
    /// Validate config fields that cannot be checked at deserialization time.
    ///
    /// Returns an error message if any field is invalid.
    pub fn validate(&self) -> std::result::Result<(), String> {
        let name = &self.tsconfig_filename;
        if name.is_empty() {
            return Err("tsconfig_filename must not be empty".to_string());
        }
        if name.starts_with("..") {
            return Err(format!(
                "tsconfig_filename must not use path traversal: got '{name}'"
            ));
        }
        if name.contains('/') || name.contains('\\') {
            return Err(format!(
                "tsconfig_filename must be a plain filename (no path separators): got '{name}'"
            ));
        }
        Ok(())
    }
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        Self {
            enabled: default_envelope_enabled(),
            import_patterns: default_envelope_import_patterns(),
            function_pattern: default_envelope_function_pattern(),
        }
    }
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            spec_dirs: default_spec_dirs(),
            exclude: default_excludes(),
            test_patterns: default_test_patterns(),
            include_dirs: Vec::new(),
            escape_hatches: EscapeHatchConfig::default(),
            baseline: BaselineConfig::default(),
            jest_mock_mode: JestMockMode::Warn,
            stale_baseline: StaleBaselinePolicy::Warn,
            release_channel: ReleaseChannel::Stable,
            telemetry: false,
            enforce_type_only_imports: default_enforce_type_only_imports(),
            tsconfig_filename: default_tsconfig_filename(),
            envelope: EnvelopeConfig::default(),
            unresolved_edge_policy: UnresolvedEdgePolicy::Warn,
            import_hygiene: ImportHygieneConfig::default(),
            strict_ownership: false,
            strict_ownership_level: StrictOwnershipLevel::Errors,
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
        assert!(!config.baseline.require_metadata);
        assert_eq!(config.strict_ownership_level, StrictOwnershipLevel::Errors);
        assert!(config.exclude.iter().any(|g| g == "**/node_modules/**"));
        assert!(config.exclude.iter().any(|g| g == "**/target/**"));
        assert!(config.exclude.iter().any(|g| g == "**/coverage/**"));
        assert!(config.exclude.iter().any(|g| g == "**/vendor/**"));
    }

    #[test]
    fn envelope_config_defaults_are_stable() {
        let config = EnvelopeConfig::default();
        assert!(config.enabled);
        assert_eq!(config.import_patterns, vec!["specgate-envelope"]);
        assert_eq!(config.function_pattern, "boundary.validate");
    }

    #[test]
    fn config_parses_custom_envelope_config() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
        envelope:
          enabled: false
          import_patterns:
            - "@acme/specgate-envelope"
            - "specgate-envelope/validate"
          function_pattern: "boundary.validate"
        "#,
        )
        .expect("parse config");

        assert!(!parsed.envelope.enabled);
        assert_eq!(
            parsed.envelope.import_patterns,
            vec!["@acme/specgate-envelope", "specgate-envelope/validate"]
        );
        assert_eq!(parsed.envelope.function_pattern, "boundary.validate");
    }

    #[test]
    fn config_uses_envelope_defaults_when_missing() {
        let parsed: SpecConfig =
            yaml_serde::from_str("spec_dirs:\n  - specs\n").expect("parse config");
        assert_eq!(parsed.envelope, EnvelopeConfig::default());
        assert_eq!(parsed.spec_dirs, vec!["specs"]);
    }

    #[test]
    fn config_parses_envelope_enabled_false() {
        let parsed: SpecConfig =
            yaml_serde::from_str("envelope:\n  enabled: false\n").expect("parse config");
        assert!(!parsed.envelope.enabled);
        assert_eq!(
            parsed.envelope.import_patterns,
            default_envelope_import_patterns()
        );
        assert_eq!(
            parsed.envelope.function_pattern,
            default_envelope_function_pattern()
        );
    }

    #[test]
    fn config_parses_multiple_envelope_import_patterns() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
        envelope:
          import_patterns:
            - one
            - two
            - three
        "#,
        )
        .expect("parse config");

        assert_eq!(parsed.envelope.import_patterns, vec!["one", "two", "three"]);
    }

    #[test]
    fn envelope_config_round_trip() {
        let config = EnvelopeConfig {
            enabled: false,
            import_patterns: vec!["a".to_string(), "b".to_string()],
            function_pattern: "validator.call".to_string(),
        };
        let rendered = serde_json::to_string(&config).expect("serialize config");
        let restored: EnvelopeConfig = serde_json::from_str(&rendered).expect("round trip parse");
        assert_eq!(restored, config);
    }

    #[test]
    fn config_parses_stale_policy_and_telemetry_overrides() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
baseline:
  require_metadata: true
stale_baseline: fail
release_channel: beta
telemetry:
  enabled: true
"#,
        )
        .expect("parse config");

        assert!(parsed.baseline.require_metadata);
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
    fn import_hygiene_parses_legacy_string_entries() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  deny_deep_imports:
    - lodash
    - express
"#,
        )
        .expect("parse config");

        assert_eq!(parsed.import_hygiene.deny_deep_imports.len(), 2);
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].pattern, "lodash");
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].max_depth, 0);
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[0].effective_severity(),
            Severity::Warning
        );
    }

    #[test]
    fn import_hygiene_parses_structured_entries() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
      severity: error
"#,
        )
        .expect("parse config");

        assert_eq!(parsed.import_hygiene.deny_deep_imports.len(), 1);
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[0].pattern,
            "lodash/**"
        );
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].max_depth, 1);
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[0].severity,
            Some(Severity::Error)
        );
    }

    #[test]
    fn import_hygiene_parses_mixed_legacy_and_structured_entries() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  deny_deep_imports:
    - lodash
    - pattern: express/**
      max_depth: 2
    - react
"#,
        )
        .expect("parse config");

        assert_eq!(parsed.import_hygiene.deny_deep_imports.len(), 3);
        // Legacy entry: lodash
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].pattern, "lodash");
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].max_depth, 0);
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].severity, None);
        // Structured entry: express/**
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[1].pattern,
            "express/**"
        );
        assert_eq!(parsed.import_hygiene.deny_deep_imports[1].max_depth, 2);
        // Legacy entry: react
        assert_eq!(parsed.import_hygiene.deny_deep_imports[2].pattern, "react");
        assert_eq!(parsed.import_hygiene.deny_deep_imports[2].max_depth, 0);
    }

    #[test]
    fn structured_entry_without_max_depth_uses_default_zero() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
"#,
        )
        .expect("parse config");

        assert_eq!(parsed.import_hygiene.deny_deep_imports.len(), 1);
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[0].pattern,
            "lodash/**"
        );
        // Default max_depth should be 0 (deny all deep imports)
        assert_eq!(parsed.import_hygiene.deny_deep_imports[0].max_depth, 0);
        // Default severity should be warning
        assert_eq!(
            parsed.import_hygiene.deny_deep_imports[0].effective_severity(),
            Severity::Warning
        );
    }

    #[test]
    fn test_boundary_compat_maps_legacy_deny_production_imports() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  test_boundary:
    deny_production_imports: true
"#,
        )
        .expect("parse config");

        assert!(parsed.import_hygiene.test_boundary.enabled);
        assert_eq!(
            parsed.import_hygiene.test_boundary.mode,
            TestBoundaryMode::ProductionOnly
        );
    }

    #[test]
    fn test_boundary_mode_defaults_to_off_when_disabled() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  test_boundary:
    mode: bidirectional
"#,
        )
        .expect("parse config");

        assert!(parsed.import_hygiene.test_boundary.enabled);
        assert_eq!(
            parsed.import_hygiene.test_boundary.effective_mode(),
            TestBoundaryMode::Bidirectional
        );
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
        assert!(rendered.contains("\"baseline\":{\"require_metadata\":false}"));
        assert!(rendered.contains("\"include_dirs\":[]"));
        assert!(rendered.contains("\"tsconfig_filename\":\"tsconfig.json\""));
        assert!(rendered.contains("\"strict_ownership_level\":\"errors\""));
    }

    #[test]
    fn config_defaults_tsconfig_filename_to_tsconfig_json() {
        let config = SpecConfig::default();
        assert_eq!(config.tsconfig_filename, "tsconfig.json");
    }

    #[test]
    fn config_parses_tsconfig_filename_override() {
        let parsed: SpecConfig =
            yaml_serde::from_str("tsconfig_filename: \"tsconfig.base.json\"\n")
                .expect("parse config");
        assert_eq!(parsed.tsconfig_filename, "tsconfig.base.json");
    }

    #[test]
    fn config_rejects_empty_tsconfig_filename() {
        let config = SpecConfig {
            tsconfig_filename: String::new(),
            ..SpecConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.contains("must not be empty"), "unexpected error: {err}");
    }

    #[test]
    fn config_rejects_tsconfig_filename_with_path_separator() {
        for bad in &["subdir/tsconfig.json", "sub\\tsconfig.json"] {
            let config = SpecConfig {
                tsconfig_filename: bad.to_string(),
                ..SpecConfig::default()
            };
            let err = config.validate().unwrap_err();
            assert!(
                err.contains("must be a plain filename"),
                "unexpected error for '{bad}': {err}"
            );
        }
    }

    #[test]
    fn test_unresolved_edge_policy_config_parse() {
        let warn: SpecConfig =
            yaml_serde::from_str("unresolved_edge_policy: warn\n").expect("parse warn");
        assert_eq!(warn.unresolved_edge_policy, UnresolvedEdgePolicy::Warn);

        let error: SpecConfig =
            yaml_serde::from_str("unresolved_edge_policy: error\n").expect("parse error");
        assert_eq!(error.unresolved_edge_policy, UnresolvedEdgePolicy::Error);

        let ignore: SpecConfig =
            yaml_serde::from_str("unresolved_edge_policy: ignore\n").expect("parse ignore");
        assert_eq!(ignore.unresolved_edge_policy, UnresolvedEdgePolicy::Ignore);
    }

    #[test]
    fn test_unresolved_edge_policy_defaults_to_warn() {
        let config = SpecConfig::default();
        assert_eq!(config.unresolved_edge_policy, UnresolvedEdgePolicy::Warn);
    }

    #[test]
    fn test_config_backward_compat_without_unresolved_policy() {
        // Old config without the field should still parse and default to Warn
        let parsed: SpecConfig =
            yaml_serde::from_str("spec_dirs:\n  - specs\n").expect("parse old config");
        assert_eq!(parsed.unresolved_edge_policy, UnresolvedEdgePolicy::Warn);
        assert_eq!(parsed.spec_dirs, vec!["specs"]);
    }

    #[test]
    fn test_unresolved_edge_policy_serializes_correctly() {
        let rendered = serde_json::to_string(&SpecConfig::default()).expect("serialize");
        assert!(rendered.contains("\"unresolved_edge_policy\":\"warn\""));
    }

    #[test]
    fn config_rejects_tsconfig_filename_with_path_traversal() {
        let config = SpecConfig {
            tsconfig_filename: "../tsconfig.json".to_string(),
            ..SpecConfig::default()
        };
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("must not use path traversal"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_strict_ownership_defaults_false() {
        let config = SpecConfig::default();
        assert!(!config.strict_ownership);
    }

    #[test]
    fn test_strict_ownership_parses_true() {
        let parsed: SpecConfig =
            yaml_serde::from_str("strict_ownership: true\n").expect("parse config");
        assert!(parsed.strict_ownership);
    }

    #[test]
    fn test_config_backward_compat_without_strict_ownership() {
        let parsed: SpecConfig =
            yaml_serde::from_str("spec_dirs:\n  - specs\n").expect("parse config");
        assert!(!parsed.strict_ownership);
        assert_eq!(parsed.strict_ownership_level, StrictOwnershipLevel::Errors);
    }

    #[test]
    fn test_strict_ownership_level_defaults_to_errors() {
        let config = SpecConfig::default();
        assert_eq!(config.strict_ownership_level, StrictOwnershipLevel::Errors);
    }

    #[test]
    fn test_strict_ownership_level_parses_warnings() {
        let parsed: SpecConfig =
            yaml_serde::from_str("strict_ownership_level: warnings\n").expect("parse config");
        assert_eq!(
            parsed.strict_ownership_level,
            StrictOwnershipLevel::Warnings
        );
    }

    #[test]
    fn test_strict_ownership_level_serializes_correctly() {
        let rendered = serde_json::to_string(&SpecConfig::default()).expect("serialize");
        assert!(rendered.contains("\"strict_ownership_level\":\"errors\""));
    }
}
