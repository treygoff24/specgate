use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use globset::Glob;

use crate::rules::{
    BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS, BOUNDARY_CANONICAL_IMPORT_RULE_ID,
    BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID,
};
use crate::spec::types::{SpecFile, SUPPORTED_SPEC_VERSIONS};

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
                        "{}: contracts are not supported in spec version 2.2; upgrade to 2.3 to use boundary contracts",
                        BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID
                    ),
                );
            }
        }
    }

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
        assert!(report
            .errors()
            .iter()
            .any(|issue| issue.message.contains("duplicate module")));
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
        assert!(report
            .errors()
            .iter()
            .any(|issue| issue.message.contains("canonical import id")));
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
                name: Some("Test Contract".to_string()),
                description: None,
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    pattern: "contracts/**/*.json".to_string(),
                    extensions: None,
                    prefix: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
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
                name: Some("Test Contract".to_string()),
                description: None,
                direction: crate::spec::types::ContractDirection::Bidirectional,
                r#match: crate::spec::types::ContractMatch {
                    pattern: "contracts/**/*.json".to_string(),
                    extensions: None,
                    prefix: None,
                },
                envelope: crate::spec::types::EnvelopeRequirement::Optional,
            }],
            ..Boundaries::default()
        });

        let report = validate_specs(&[spec]);
        assert!(
            !report.has_errors(),
            "contracts in 2.3 spec should be valid"
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
}
