use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use globset::{Glob, GlobBuilder};

use crate::rules::{
    BOUNDARY_CANONICAL_IMPORT_RULE_ID, BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS,
    BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID,
};
use crate::spec::types::{SUPPORTED_SPEC_VERSIONS, SpecFile};

const KNOWN_CONSTRAINT_RULES: &[&str] = &[
    "no-circular-deps",
    "enforce-layer",
    "boundary.never_imports",
    "boundary.allow_imports_from",
    "boundary.public_api",
    "boundary.deny_imported_by",
    "boundary.allow_imported_by",
    "boundary.visibility.internal",
    "boundary.visibility.private",
    BOUNDARY_CANONICAL_IMPORT_RULE_ID,
    BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS,
    // Contract validation rules
    "boundary.contract_missing",
    "boundary.contract_empty",
    "boundary.match_unresolved",
    "boundary.contract_ref_invalid",
    BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID,
    "boundary.envelope_missing",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub level: ValidationLevel,
    pub module: String,
    pub message: String,
    pub spec_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn errors(&self) -> Vec<&ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.level == ValidationLevel::Error)
            .collect()
    }

    pub fn warnings(&self) -> Vec<&ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.level == ValidationLevel::Warning)
            .collect()
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.level == ValidationLevel::Error)
    }

    fn push(
        &mut self,
        level: ValidationLevel,
        module: String,
        message: impl Into<String>,
        spec_path: Option<PathBuf>,
    ) {
        self.issues.push(ValidationIssue {
            level,
            module,
            message: message.into(),
            spec_path,
        });
    }

    pub fn push_error(&mut self, spec: &SpecFile, message: impl Into<String>) {
        self.push(
            ValidationLevel::Error,
            spec.module.clone(),
            message,
            spec.spec_path.clone(),
        );
    }

    pub fn push_warning(&mut self, spec: &SpecFile, message: impl Into<String>) {
        self.push(
            ValidationLevel::Warning,
            spec.module.clone(),
            message,
            spec.spec_path.clone(),
        );
    }
}

/// Validate loaded specs according to Phase 1 schema + consistency checks.
pub fn validate_specs(specs: &[SpecFile]) -> ValidationReport {
    let mut report = ValidationReport::default();

    let mut seen_modules: BTreeMap<String, Option<PathBuf>> = BTreeMap::new();
    let mut seen_canonical_ids: BTreeMap<String, String> = BTreeMap::new();

    for spec in specs {
        validate_single_spec(spec, &mut report);

        if let Some(previous_path) =
            seen_modules.insert(spec.module.clone(), spec.spec_path.clone())
        {
            report.push_error(
                spec,
                format!(
                    "duplicate module '{}' (previous declaration at {:?})",
                    spec.module, previous_path
                ),
            );
        }

        for canonical_id in spec.canonical_import_ids() {
            match seen_canonical_ids.get(&canonical_id) {
                Some(previous_module) if previous_module != &spec.module => report.push_error(
                    spec,
                    format!(
                        "canonical import id '{canonical_id}' already declared by module '{previous_module}'"
                    ),
                ),
                _ => {
                    seen_canonical_ids.insert(canonical_id, spec.module.clone());
                }
            }
        }
    }

    report
}

/// Validates imports_contract format: "module:contract_id" with exactly one colon
/// and non-empty module/contract segments.
fn validate_imports_contract_format(imports_contract: &str) -> Result<(), String> {
    let parts: Vec<&str> = imports_contract.split(':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "imports_contract '{imports_contract}' must contain exactly one colon separating module and contract id"
        ));
    }
    let module = parts[0];
    let contract_id = parts[1];
    if module.is_empty() {
        return Err(format!(
            "imports_contract '{imports_contract}' has empty module segment"
        ));
    }
    if contract_id.is_empty() {
        return Err(format!(
            "imports_contract '{imports_contract}' has empty contract id segment"
        ));
    }
    Ok(())
}

/// Validates contract id uniqueness within the module and all contract fields.
fn validate_contracts(spec: &SpecFile, report: &mut ValidationReport, _version: &str) {
    let Some(boundaries) = &spec.boundaries else {
        return;
    };

    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();

    for contract in &boundaries.contracts {
        // Validate contract id uniqueness
        if contract.id.is_empty() {
            report.push_error(
                spec,
                "boundary.contract_empty: contract id must be non-empty",
            );
        } else if seen_ids.contains(contract.id.as_str()) {
            report.push_error(
                spec,
                format!(
                    "boundary.contract_ref_invalid: duplicate contract id '{}'",
                    contract.id
                ),
            );
        } else {
            seen_ids.insert(&contract.id);
        }

        // Validate contract path is non-empty
        if contract.contract.trim().is_empty() {
            report.push_error(
                spec,
                format!(
                    "boundary.contract_missing: contract '{}' has empty contract path",
                    contract.id
                ),
            );
        } else {
            // Validate contract file extension
            let contract_path = contract.contract.trim();
            let has_valid_ext = crate::spec::types::CONTRACT_FILE_EXTENSIONS
                .iter()
                .any(|ext| contract_path.ends_with(ext));
            if !has_valid_ext {
                report.push_error(
                    spec,
                    format!(
                        "boundary.contract_ref_invalid: contract '{}' path '{}' has invalid extension, expected one of {:?}",
                        contract.id,
                        contract_path,
                        crate::spec::types::CONTRACT_FILE_EXTENSIONS
                    ),
                );
            }
        }

        // Validate match.files is non-empty
        if contract.r#match.files.is_empty() {
            report.push_error(
                spec,
                format!(
                    "boundary.match_unresolved: contract '{}' has empty match.files",
                    contract.id
                ),
            );
        } else {
            // Validate each glob pattern in match.files using GlobBuilder with literal_separator(true)
            for file_pattern in &contract.r#match.files {
                if file_pattern.is_empty() {
                    report.push_error(
                        spec,
                        format!(
                            "boundary.match_unresolved: contract '{}' has empty glob pattern in match.files",
                            contract.id
                        ),
                    );
                } else {
                    let glob_result = GlobBuilder::new(file_pattern)
                        .literal_separator(true)
                        .build();
                    if glob_result.is_err() {
                        report.push_error(
                            spec,
                            format!(
                                "boundary.match_unresolved: contract '{}' has invalid glob pattern '{}'",
                                contract.id, file_pattern
                            ),
                        );
                    }
                }
            }
        }

        // Validate imports_contract format
        for import_ref in &contract.imports_contract {
            if let Err(msg) = validate_imports_contract_format(import_ref) {
                report.push_error(spec, format!("boundary.contract_ref_invalid: {msg}"));
            }
        }
    }
}

fn validate_single_spec(spec: &SpecFile, report: &mut ValidationReport) {
    let version = spec.version.trim();
    if !SUPPORTED_SPEC_VERSIONS.contains(&version) {
        report.push_error(
            spec,
            format!(
                "unsupported spec version '{}'; expected one of {:?}",
                spec.version, SUPPORTED_SPEC_VERSIONS
            ),
        );
    }

    // Check for contracts in 2.2 spec (version mismatch violation)
    if version == "2.2" {
        if let Some(boundaries) = &spec.boundaries {
            if !boundaries.contracts.is_empty() {
                report.push_error(
                    spec,
                    format!(
                        "{BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID}: contracts are not supported in spec version 2.2; upgrade to 2.3 to use boundary contracts"
                    ),
                );
            }
        }
    }

    // Validate contracts for 2.3+ specs
    validate_contracts(spec, report, version);

    if spec.module.trim().is_empty() {
        report.push_error(spec, "module must be non-empty");
    }

    for constraint in &spec.constraints {
        if !KNOWN_CONSTRAINT_RULES.contains(&constraint.rule.as_str()) {
            report.push_error(
                spec,
                format!(
                    "unknown constraint rule '{}'; expected one of {:?}",
                    constraint.rule, KNOWN_CONSTRAINT_RULES
                ),
            );
        }
    }

    if let Some(boundaries) = &spec.boundaries {
        if let Some(path_glob) = &boundaries.path {
            if Glob::new(path_glob).is_err() {
                report.push_error(
                    spec,
                    format!("invalid boundaries.path glob pattern: '{path_glob}'"),
                );
            }
        }

        for public_api_glob in &boundaries.public_api {
            if Glob::new(public_api_glob).is_err() {
                report.push_error(
                    spec,
                    format!("invalid boundaries.public_api glob pattern: '{public_api_glob}'"),
                );
            }
        }

        let allow_set: BTreeSet<&str> = boundaries
            .allow_imports_from
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(String::as_str)
            .collect();
        let deny_set: BTreeSet<&str> = boundaries
            .never_imports
            .iter()
            .map(String::as_str)
            .collect();

        for overlap in allow_set.intersection(&deny_set) {
            report.push_warning(
                spec,
                format!("module '{overlap}' is in both allow_imports_from and never_imports"),
            );
        }

        let allow_imported_by_set: BTreeSet<&str> = boundaries
            .allow_imported_by
            .iter()
            .map(String::as_str)
            .collect();
        let deny_imported_by_set: BTreeSet<&str> = boundaries
            .deny_imported_by
            .iter()
            .map(String::as_str)
            .collect();

        for overlap in allow_imported_by_set.intersection(&deny_imported_by_set) {
            report.push_warning(
                spec,
                format!("module '{overlap}' is in both allow_imported_by and deny_imported_by"),
            );
        }

        if boundaries.enforce_canonical_imports && spec.canonical_import_ids().is_empty() {
            report.push_warning(
                spec,
                "enforce_canonical_imports is true but module declares no import_id/import_ids",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS;
    use crate::spec::types::{Boundaries, Constraint, Severity};

    use super::*;

    fn base_spec(module: &str) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: None,
            constraints: Vec::new(),
            spec_path: None,
        }
    }

    #[test]
    fn duplicate_module_is_error() {
        let specs = vec![base_spec("orders"), base_spec("orders")];
        let report = validate_specs(&specs);
        assert!(report.has_errors());
        assert!(
            report
                .errors()
                .iter()
                .any(|issue| issue.message.contains("duplicate module"))
        );
    }

    #[test]
    fn invalid_rule_is_error() {
        let mut spec = base_spec("orders");
        spec.constraints.push(Constraint {
            rule: "unknown-rule".to_string(),
            params: serde_json::json!({}),
            severity: Severity::Error,
            message: None,
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors());
    }

    #[test]
    fn boundary_constraint_rules_are_supported() {
        let mut spec = base_spec("orders");
        spec.constraints = vec![
            Constraint {
                rule: "boundary.never_imports".to_string(),
                params: serde_json::json!({}),
                severity: Severity::Warning,
                message: None,
            },
            Constraint {
                rule: BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS.to_string(),
                params: serde_json::json!({}),
                severity: Severity::Error,
                message: None,
            },
        ];

        let report = validate_specs(&[spec]);
        assert_eq!(report.errors().len(), 0);
    }

    #[test]
    fn overlap_is_warning() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            allow_imports_from: Some(vec!["payments".to_string()]),
            never_imports: vec!["payments".to_string()],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert_eq!(report.errors().len(), 0);
        assert!(report.warnings().iter().any(|issue| {
            issue
                .message
                .contains("both allow_imports_from and never_imports")
        }));
    }

    #[test]
    fn provider_overlap_is_warning() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            allow_imported_by: vec!["api".to_string()],
            deny_imported_by: vec!["api".to_string()],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert_eq!(report.errors().len(), 0);
        assert!(report.warnings().iter().any(|issue| {
            issue
                .message
                .contains("both allow_imported_by and deny_imported_by")
        }));
    }

    #[test]
    fn invalid_public_api_glob_is_error() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            public_api: vec!["[".to_string()],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors());
        assert!(report.errors().iter().any(|issue| {
            issue
                .message
                .contains("invalid boundaries.public_api glob pattern")
        }));
    }

    #[test]
    fn duplicate_canonical_id_is_error() {
        let mut a = base_spec("orders");
        a.import_id = Some("@app/core".to_string());

        let mut b = base_spec("payments");
        b.import_ids = vec!["@app/core".to_string()];

        let report = validate_specs(&[a, b]);
        assert!(
            report
                .errors()
                .iter()
                .any(|issue| issue.message.contains("canonical import id"))
        );
    }

    // === Version Handling Tests ===

    #[test]
    fn spec_version_2_2_is_valid() {
        let spec = base_spec("orders");
        let report = validate_specs(&[spec]);
        assert!(!report.has_errors(), "spec version 2.2 should be valid");
    }

    #[test]
    fn spec_version_2_3_is_valid() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        let report = validate_specs(&[spec]);
        assert!(!report.has_errors(), "spec version 2.3 should be valid");
    }

    #[test]
    fn unsupported_spec_version_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.0".to_string();
        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "unsupported version should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("unsupported spec version") && issue.message.contains("2.0")
            }),
            "error should mention unsupported version 2.0"
        );
    }

    #[test]
    fn contracts_in_2_2_spec_emits_version_mismatch_error() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "contracts in 2.2 spec should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue
                    .message
                    .contains(BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID)
                    && issue.message.contains("upgrade to 2.3")
            }),
            "error should mention boundary.contract_version_mismatch and upgrade hint"
        );
    }

    #[test]
    fn contracts_in_2_3_spec_is_valid() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            !report
                .errors()
                .iter()
                .any(|issue| issue.message.contains("invalid extension")),
            "extension .json should be valid"
        );
    }

    #[test]
    fn empty_contracts_in_2_2_spec_is_valid() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            contracts: vec![],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            !report.has_errors(),
            "empty contracts in 2.2 spec should be valid"
        );
    }

    // === Contract Validation Tests ===

    #[test]
    fn duplicate_contract_id_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![
                crate::spec::types::BoundaryContract {
                    id: "same_id".to_string(),
                    contract: "contracts/first.json".to_string(),
                    direction: crate::spec::types::ContractDirection::Inbound,
                    r#match: crate::spec::types::ContractMatch {
                        files: vec!["src/first.ts".to_string()],
                        pattern: None,
                    },
                    envelope: crate::spec::types::EnvelopeRequirement::Optional,
                    imports_contract: vec![],
                },
                crate::spec::types::BoundaryContract {
                    id: "same_id".to_string(),
                    contract: "contracts/second.json".to_string(),
                    direction: crate::spec::types::ContractDirection::Outbound,
                    r#match: crate::spec::types::ContractMatch {
                        files: vec!["src/second.ts".to_string()],
                        pattern: None,
                    },
                    envelope: crate::spec::types::EnvelopeRequirement::Optional,
                    imports_contract: vec![],
                },
            ],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "duplicate contract id should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("duplicate contract id")
            }),
            "error should mention duplicate contract id"
        );
    }

    #[test]
    fn empty_contract_id_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "empty contract id should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_empty")
                    && issue.message.contains("contract id must be non-empty")
            }),
            "error should mention empty contract id"
        );
    }

    #[test]
    fn empty_contract_path_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "empty contract path should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_missing")
                    && issue.message.contains("empty contract path")
            }),
            "error should mention empty contract path"
        );
    }

    #[test]
    fn invalid_contract_extension_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.txt".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            report.has_errors(),
            "invalid contract extension should be error"
        );
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("invalid extension")
            }),
            "error should mention invalid extension"
        );
    }

    #[test]
    fn valid_contract_extensions_are_accepted() {
        let extensions = vec!["json", "yaml", "yml", "ts", "zod", "proto"];

        for ext in extensions {
            let mut spec = base_spec("orders");
            spec.version = "2.3".to_string();
            spec.boundaries = Some(Boundaries {
                contracts: vec![crate::spec::types::BoundaryContract {
                    id: "test_contract".to_string(),
                    contract: format!("contracts/test.{ext}"),
                    direction: crate::spec::types::ContractDirection::Bidirectional,
                    r#match: crate::spec::types::ContractMatch {
                        files: vec!["src/**/*.ts".to_string()],
                        pattern: None,
                    },
                    envelope: crate::spec::types::EnvelopeRequirement::Optional,
                    imports_contract: vec![],
                }],
                ..Boundaries::default()
            });

            let report = validate_specs(&[spec]);
            assert!(
                !report
                    .errors()
                    .iter()
                    .any(|issue| issue.message.contains("invalid extension")),
                "extension .{ext} should be valid"
            );
        }
    }

    #[test]
    fn empty_match_files_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec![],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "empty match.files should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.match_unresolved")
                    && issue.message.contains("empty match.files")
            }),
            "error should mention empty match.files"
        );
    }

    #[test]
    fn invalid_glob_pattern_in_match_files_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["[".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(report.has_errors(), "invalid glob pattern should be error");
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.match_unresolved")
                    && issue.message.contains("invalid glob pattern")
            }),
            "error should mention invalid glob pattern"
        );
    }

    #[test]
    fn valid_glob_pattern_in_match_files_is_accepted() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string(), "lib/*.js".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            !report
                .errors()
                .iter()
                .any(|issue| issue.message.contains("match_unresolved")),
            "valid glob patterns should not produce match_unresolved error"
        );
    }

    #[test]
    fn imports_contract_missing_colon_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec!["invalid_format".to_string()],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            report.has_errors(),
            "imports_contract without colon should be error"
        );
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("exactly one colon")
            }),
            "error should mention exactly one colon"
        );
    }

    #[test]
    fn imports_contract_multiple_colons_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec!["module:id:extra".to_string()],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            report.has_errors(),
            "imports_contract with multiple colons should be error"
        );
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("exactly one colon")
            }),
            "error should mention exactly one colon"
        );
    }

    #[test]
    fn imports_contract_empty_module_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![":contract_id".to_string()],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            report.has_errors(),
            "imports_contract with empty module should be error"
        );
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("empty module segment")
            }),
            "error should mention empty module segment"
        );
    }

    #[test]
    fn imports_contract_empty_contract_id_is_error() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec!["module:".to_string()],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            report.has_errors(),
            "imports_contract with empty contract id should be error"
        );
        assert!(
            report.errors().iter().any(|issue| {
                issue.message.contains("boundary.contract_ref_invalid")
                    && issue.message.contains("empty contract id segment")
            }),
            "error should mention empty contract id segment"
        );
    }

    #[test]
    fn valid_imports_contract_format_is_accepted() {
        let mut spec = base_spec("orders");
        spec.version = "2.3".to_string();
        spec.boundaries = Some(Boundaries {
            contracts: vec![crate::spec::types::BoundaryContract {
                id: "test_contract".to_string(),
                contract: "contracts/test.json".to_string(),
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    files: vec!["src/**/*.ts".to_string()],
                    pattern: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
                imports_contract: vec![
                    "api/handlers:create_user".to_string(),
                    "core/domain:billing_contract".to_string(),
                ],
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            !report
                .errors()
                .iter()
                .any(|issue| issue.message.contains("contract_ref_invalid")),
            "valid imports_contract format should not produce error"
        );
    }
}
