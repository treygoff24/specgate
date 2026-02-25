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
        assert!(config.exclude.iter().any(|g| g == "**/node_modules/**"));
    }
}
