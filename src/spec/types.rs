//! Spec language types and version contract.
//!
//! ## Version Contract (Wave 0 Lock)
//!
//! Specgate enforces strict version compatibility for spec files:
//!
//! - **Supported versions**: `2.2`, `2.3` (constants: `SUPPORTED_SPEC_VERSIONS`)
//! - **Policy**: Exact match required. Versions `2` or `2.0` are NOT accepted.
//! - **Migration**: Users must update spec files to `version: "2.2"` or `version: "2.3"` to use current features.
//!
//! ### Why strict matching?
//!
//! The spec language is evolving rapidly during foundation phases. Allowing loose
//! version matching (e.g., `2` matches `2.2`) would mask breaking changes and make
//! it harder to reason about spec compatibility. We enforce exact matching to:
//!
//! 1. Force explicit version updates when specs change
//! 2. Make version compatibility unambiguous
//! 3. Enable future support for multiple versions if needed
//!
//! ### Future versions
//!
//! When `2.3` is released:
//! - `2.2` specs will continue to work (backward compatible)
//! - `2.3` features will require `version: "2.3"`
//! - A compatibility matrix will be documented

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Array of supported spec language versions.
///
/// These are the accepted values for the `version` field in `.spec.yml` files.
/// See module documentation for version contract details.
pub const SUPPORTED_SPEC_VERSIONS: &[&str] = &["2.2", "2.3"];

/// Legacy alias for single supported spec version.
///
/// DEPRECATED: Use `SUPPORTED_SPEC_VERSIONS` for version checking.
/// This constant is maintained for backward compatibility.
pub const SUPPORTED_SPEC_VERSION: &str = "2.2";

/// Current spec language version used for scaffold generation.
///
/// This is the version used when generating new spec files via `specgate init`.
/// It may be newer than SUPPORTED_SPEC_VERSION during transitional periods.
pub const CURRENT_SPEC_VERSION: &str = "2.3";

/// Supported file extensions for contract files.
///
/// These extensions indicate files that can define or match boundary contracts.
pub const CONTRACT_FILE_EXTENSIONS: &[&str] = &[".json", ".yaml", ".yml", ".ts", ".zod", ".proto"];

/// A single `.spec.yml` file (one per module).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Boundaries {
    /// Glob pattern for files belonging to this module.
    #[serde(default)]
    pub path: Option<String>,
    /// Public API entrypoint files.
    #[serde(default)]
    pub public_api: Vec<String>,

    // importer-side
    /// Allowlist of modules this module may import from.
    ///
    /// - `None` (field omitted): no importer-side allowlist restriction.
    /// - `Some(vec![])`: deny all cross-module imports.
    /// - `Some([...])`: only listed provider modules allowed.
    #[serde(default)]
    pub allow_imports_from: Option<Vec<String>>,
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
    /// Boundary contracts defining cross-boundary data exchange.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contracts: Vec<BoundaryContract>,
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
#[serde(deny_unknown_fields)]
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

/// Direction of contract flow for boundary contracts.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContractDirection {
    /// Contract applies to inbound (incoming) data.
    Inbound,
    /// Contract applies to outbound (outgoing) data.
    Outbound,
    /// Contract applies bidirectionally.
    #[default]
    Bidirectional,
}

/// Envelope requirement for contract validation.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EnvelopeRequirement {
    /// Envelope is optional (default).
    #[default]
    Optional,
    /// Envelope is required.
    Required,
}

/// Contract matching pattern for boundary contracts.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContractMatch {
    /// Glob patterns for files containing this boundary crossing.
    #[serde(default)]
    pub files: Vec<String>,
    /// Symbol/function name for AST-level binding (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

/// A boundary contract defining cross-boundary data exchange (spec version 2.3+).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct BoundaryContract {
    /// Unique identifier within the module.
    #[serde(default)]
    pub id: String,
    /// Path to contract file, relative to project root.
    #[serde(default)]
    pub contract: String,
    /// Code-site binding.
    pub r#match: ContractMatch,
    /// Data flow direction at this boundary.
    #[serde(default)]
    pub direction: ContractDirection,
    /// Envelope validation requirement.
    #[serde(default)]
    pub envelope: EnvelopeRequirement,
    /// Optional cross-module consumer references in format "module:contract_id".
    #[serde(default)]
    pub imports_contract: Vec<String>,
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

    // === Version Constants Tests ===

    #[test]
    fn supported_spec_versions_includes_2_2_and_2_3() {
        assert!(SUPPORTED_SPEC_VERSIONS.contains(&"2.2"));
        assert!(SUPPORTED_SPEC_VERSIONS.contains(&"2.3"));
        assert_eq!(SUPPORTED_SPEC_VERSIONS.len(), 2);
    }

    #[test]
    fn legacy_supported_spec_version_alias_is_2_2() {
        assert_eq!(SUPPORTED_SPEC_VERSION, "2.2");
        assert!(SUPPORTED_SPEC_VERSIONS.contains(&SUPPORTED_SPEC_VERSION));
    }

    #[test]
    fn current_spec_version_is_2_3() {
        assert_eq!(CURRENT_SPEC_VERSION, "2.3");
        assert!(SUPPORTED_SPEC_VERSIONS.contains(&CURRENT_SPEC_VERSION));
    }

    #[test]
    fn contract_file_extensions_contains_expected() {
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".json"));
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".yaml"));
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".yml"));
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".ts"));
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".zod"));
        assert!(CONTRACT_FILE_EXTENSIONS.contains(&".proto"));
        assert_eq!(CONTRACT_FILE_EXTENSIONS.len(), 6);
    }

    // === Serde Roundtrip Tests ===

    #[test]
    fn spec_file_serde_roundtrip() {
        let original = SpecFile {
            version: "2.3".to_string(),
            module: "api/orders".to_string(),
            package: Some("@app/orders".to_string()),
            import_id: Some("@app/orders".to_string()),
            import_ids: vec!["orders".to_string()],
            description: Some("Order management module".to_string()),
            boundaries: Some(Boundaries::default()),
            constraints: vec![Constraint {
                rule: "no-circular-deps".to_string(),
                params: default_params(),
                severity: Severity::Error,
                message: None,
            }],
            spec_path: None,
        };

        let json = serde_json::to_string(&original).expect("failed to serialize");
        let deserialized: SpecFile = serde_json::from_str(&json).expect("failed to deserialize");

        assert_eq!(original.version, deserialized.version);
        assert_eq!(original.module, deserialized.module);
        assert_eq!(original.package, deserialized.package);
        assert_eq!(original.import_id, deserialized.import_id);
    }

    #[test]
    fn boundaries_serde_roundtrip() {
        let original = Boundaries {
            path: Some("src/**/*.ts".to_string()),
            public_api: vec!["src/index.ts".to_string()],
            allow_imports_from: Some(vec!["api/core".to_string()]),
            never_imports: vec!["ui/old".to_string()],
            allow_type_imports_from: vec!["types/shared".to_string()],
            visibility: Some(Visibility::Internal),
            allow_imported_by: vec!["ui/checkout".to_string()],
            deny_imported_by: vec!["ui/legacy".to_string()],
            friend_modules: vec!["test/helpers".to_string()],
            enforce_canonical_imports: true,
            allowed_dependencies: vec!["lodash".to_string()],
            forbidden_dependencies: vec!["moment".to_string()],
            enforce_in_tests: true,
            contracts: Vec::new(),
        };

        let json = serde_json::to_string(&original).expect("failed to serialize");
        let deserialized: Boundaries = serde_json::from_str(&json).expect("failed to deserialize");

        assert_eq!(original.path, deserialized.path);
        assert_eq!(original.public_api, deserialized.public_api);
        assert_eq!(original.visibility, deserialized.visibility);
        assert_eq!(original.enforce_in_tests, deserialized.enforce_in_tests);
    }

    #[test]
    fn boundary_contract_serde_roundtrip() {
        let original = BoundaryContract {
            id: "api_contract".to_string(),
            contract: "contracts/api.json".to_string(),
            direction: ContractDirection::Outbound,
            r#match: ContractMatch {
                files: vec!["src/api/handlers.ts".to_string()],
                pattern: Some("handleRequest".to_string()),
            },
            envelope: EnvelopeRequirement::Required,
            imports_contract: vec!["other_module:their_contract".to_string()],
        };

        let json = serde_json::to_string(&original).expect("failed to serialize");
        let deserialized: BoundaryContract =
            serde_json::from_str(&json).expect("failed to deserialize");

        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.contract, deserialized.contract);
        assert_eq!(original.direction, deserialized.direction);
        assert_eq!(original.envelope, deserialized.envelope);
        assert_eq!(original.imports_contract, deserialized.imports_contract);
    }

    // === Default Tests ===

    #[test]
    fn boundaries_defaults() {
        let boundaries = Boundaries::default();
        assert!(boundaries.path.is_none());
        assert!(boundaries.public_api.is_empty());
        assert!(boundaries.allow_imports_from.is_none());
        assert!(boundaries.never_imports.is_empty());
        assert!(boundaries.allow_type_imports_from.is_empty());
        assert!(boundaries.visibility.is_none());
        assert!(boundaries.allow_imported_by.is_empty());
        assert!(boundaries.deny_imported_by.is_empty());
        assert!(boundaries.friend_modules.is_empty());
        assert!(!boundaries.enforce_canonical_imports);
        assert!(boundaries.allowed_dependencies.is_empty());
        assert!(boundaries.forbidden_dependencies.is_empty());
        assert!(!boundaries.enforce_in_tests);
        assert!(boundaries.contracts.is_empty());
    }

    #[test]
    fn boundary_contract_defaults() {
        let contract = BoundaryContract::default();
        assert!(contract.id.is_empty());
        assert!(contract.contract.is_empty());
        assert_eq!(contract.direction, ContractDirection::Bidirectional);
        assert_eq!(contract.envelope, EnvelopeRequirement::Optional);
        assert!(contract.imports_contract.is_empty());
        // r#match.files defaults to empty
        assert!(contract.r#match.files.is_empty());
    }

    #[test]
    fn contract_match_defaults() {
        let contract_match = ContractMatch::default();
        assert!(contract_match.files.is_empty());
        assert!(contract_match.pattern.is_none());
    }

    #[test]
    fn contract_direction_defaults_to_bidirectional() {
        let direction: ContractDirection = Default::default();
        assert_eq!(direction, ContractDirection::Bidirectional);
    }

    #[test]
    fn envelope_requirement_defaults_to_optional() {
        let req: EnvelopeRequirement = Default::default();
        assert_eq!(req, EnvelopeRequirement::Optional);
    }

    // === deny_unknown_fields Tests ===

    #[test]
    fn spec_file_deny_unknown_fields() {
        let json = r#"{
            "version": "2.2",
            "module": "test",
            "unknown_field": "value"
        }"#;

        let result: Result<SpecFile, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    #[test]
    fn boundaries_deny_unknown_fields() {
        let json = r#"{
            "unknown_field": "value"
        }"#;

        let result: Result<Boundaries, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    #[test]
    fn constraint_deny_unknown_fields() {
        let json = r#"{
            "rule": "test",
            "unknown_field": "value"
        }"#;

        let result: Result<Constraint, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    #[test]
    fn boundary_contract_deny_unknown_fields() {
        let json = r#"{
            "match": { "pattern": "test" },
            "unknown_field": "value"
        }"#;

        let result: Result<BoundaryContract, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    #[test]
    fn contract_match_deny_unknown_fields() {
        let json = r#"{
            "pattern": "test",
            "unknown_field": "value"
        }"#;

        let result: Result<ContractMatch, _> = serde_json::from_str(json);
        assert!(result.is_err(), "should reject unknown fields");
    }

    // === Contract Integration Tests ===

    #[test]
    fn boundaries_with_contracts_serialization() {
        let boundaries = Boundaries {
            contracts: vec![
                BoundaryContract {
                    id: "inbound".to_string(),
                    contract: "contracts/inbound.json".to_string(),
                    direction: ContractDirection::Inbound,
                    r#match: ContractMatch {
                        files: vec!["src/input/**/*.ts".to_string()],
                        pattern: Some("handleInput".to_string()),
                    },
                    envelope: EnvelopeRequirement::Optional,
                    imports_contract: vec![],
                },
                BoundaryContract {
                    id: "outbound".to_string(),
                    contract: "contracts/outbound.yaml".to_string(),
                    direction: ContractDirection::Outbound,
                    r#match: ContractMatch {
                        files: vec!["src/output/**/*.ts".to_string()],
                        pattern: None,
                    },
                    envelope: EnvelopeRequirement::Required,
                    imports_contract: vec!["consumer:their_contract".to_string()],
                },
            ],
            ..Default::default()
        };

        let json = serde_json::to_string(&boundaries).expect("failed to serialize");
        let deserialized: Boundaries = serde_json::from_str(&json).expect("failed to deserialize");

        assert_eq!(deserialized.contracts.len(), 2);
        assert_eq!(
            deserialized.contracts[0].direction,
            ContractDirection::Inbound
        );
        assert_eq!(
            deserialized.contracts[1].direction,
            ContractDirection::Outbound
        );
    }

    #[test]
    fn empty_contracts_skip_serializing() {
        let boundaries = Boundaries::default();
        let json = serde_json::to_string(&boundaries).expect("failed to serialize");
        // Empty contracts should not appear in JSON
        assert!(!json.contains("contracts"));
    }

    #[test]
    fn contract_direction_serde_variants() {
        assert_eq!(
            serde_json::to_string(&ContractDirection::Inbound).unwrap(),
            "\"inbound\""
        );
        assert_eq!(
            serde_json::to_string(&ContractDirection::Outbound).unwrap(),
            "\"outbound\""
        );
        assert_eq!(
            serde_json::to_string(&ContractDirection::Bidirectional).unwrap(),
            "\"bidirectional\""
        );

        let inbound: ContractDirection = serde_json::from_str("\"inbound\"").unwrap();
        assert_eq!(inbound, ContractDirection::Inbound);

        let outbound: ContractDirection = serde_json::from_str("\"outbound\"").unwrap();
        assert_eq!(outbound, ContractDirection::Outbound);

        let bidirectional: ContractDirection = serde_json::from_str("\"bidirectional\"").unwrap();
        assert_eq!(bidirectional, ContractDirection::Bidirectional);
    }

    #[test]
    fn envelope_requirement_serde_variants() {
        assert_eq!(
            serde_json::to_string(&EnvelopeRequirement::Optional).unwrap(),
            "\"optional\""
        );
        assert_eq!(
            serde_json::to_string(&EnvelopeRequirement::Required).unwrap(),
            "\"required\""
        );

        let optional: EnvelopeRequirement = serde_json::from_str("\"optional\"").unwrap();
        assert_eq!(optional, EnvelopeRequirement::Optional);

        let required: EnvelopeRequirement = serde_json::from_str("\"required\"").unwrap();
        assert_eq!(required, EnvelopeRequirement::Required);
    }
}
