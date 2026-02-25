use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Supported spec language version for this foundation phase.
pub const SUPPORTED_SPEC_VERSION: &str = "2.2";

/// A single `.spec.yml` file (one per module).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SpecFile {
    /// Schema version. Must be "2.2".
    pub version: String,
    /// Module identifier, e.g. "api/orders", "ui/checkout".
    pub module: String,
    /// Optional package/workspace identifier.
    #[serde(default)]
    pub package: Option<String>,
    /// Canonical import identifier (preferred single ID).
    #[serde(default)]
    pub import_id: Option<String>,
    /// Canonical import aliases.
    #[serde(default)]
    pub import_ids: Vec<String>,
    /// Human-readable description (not verified by core engine).
    #[serde(default)]
    pub description: Option<String>,
    /// Module boundary policy.
    #[serde(default)]
    pub boundaries: Option<Boundaries>,
    /// Architectural constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Path to spec file on disk, set post-load.
    #[serde(skip)]
    pub spec_path: Option<PathBuf>,
}

impl SpecFile {
    /// Canonical import identifiers for this module (deduped + sorted).
    pub fn canonical_import_ids(&self) -> Vec<String> {
        use std::collections::BTreeSet;

        let mut ids = BTreeSet::new();
        if let Some(primary) = &self.import_id {
            let trimmed = primary.trim();
            if !trimmed.is_empty() {
                ids.insert(trimmed.to_string());
            }
        }
        for alias in &self.import_ids {
            let trimmed = alias.trim();
            if !trimmed.is_empty() {
                ids.insert(trimmed.to_string());
            }
        }
        ids.into_iter().collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct Boundaries {
    /// Glob pattern for files belonging to this module.
    #[serde(default)]
    pub path: Option<String>,
    /// Public API entrypoint files.
    #[serde(default)]
    pub public_api: Vec<String>,

    // importer-side
    /// Allowlist of modules this module may import from (default deny if non-empty).
    #[serde(default)]
    pub allow_imports_from: Vec<String>,
    /// Hard-deny list of modules this module may never import from.
    #[serde(default)]
    pub never_imports: Vec<String>,
    /// Type-only import carve-out allowlist.
    #[serde(default)]
    pub allow_type_imports_from: Vec<String>,

    // provider-side
    /// Provider visibility gate. Defaults to public when omitted.
    #[serde(default)]
    pub visibility: Option<Visibility>,
    /// If non-empty, importer allowlist.
    #[serde(default)]
    pub allow_imported_by: Vec<String>,
    /// Hard-deny importers.
    #[serde(default)]
    pub deny_imported_by: Vec<String>,
    /// Internal visibility exceptions.
    #[serde(default)]
    pub friend_modules: Vec<String>,

    // canonical imports
    /// Enforce canonical imports for cross-module edges.
    #[serde(default)]
    pub enforce_canonical_imports: bool,

    // third-party dependencies
    #[serde(default)]
    pub allowed_dependencies: Vec<String>,
    #[serde(default)]
    pub forbidden_dependencies: Vec<String>,

    /// If true, boundary checks apply to test files.
    #[serde(default)]
    pub enforce_in_tests: bool,
}

impl Boundaries {
    /// Resolve configured visibility with default fallback.
    pub fn visibility_or_default(&self) -> Visibility {
        self.visibility.unwrap_or(Visibility::Public)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Internal,
    Private,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Constraint {
    /// Rule identifier.
    pub rule: String,
    /// Rule-specific parameters.
    #[serde(default = "default_params")]
    pub params: serde_json::Value,
    /// Severity level.
    #[serde(default)]
    pub severity: Severity,
    /// Optional human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

fn default_params() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_import_ids_merge_and_dedupe() {
        let spec = SpecFile {
            version: "2.2".to_string(),
            module: "orders".to_string(),
            package: None,
            import_id: Some("@app/orders".to_string()),
            import_ids: vec!["@app/orders".to_string(), "@app/orders-v2".to_string()],
            description: None,
            boundaries: None,
            constraints: Vec::new(),
            spec_path: None,
        };

        assert_eq!(
            spec.canonical_import_ids(),
            vec!["@app/orders".to_string(), "@app/orders-v2".to_string()]
        );
    }

    #[test]
    fn visibility_defaults_to_public() {
        let boundaries = Boundaries::default();
        assert_eq!(boundaries.visibility_or_default(), Visibility::Public);
    }
}
