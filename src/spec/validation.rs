use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use globset::Glob;

use crate::spec::types::{SUPPORTED_SPEC_VERSION, SpecFile};

const KNOWN_CONSTRAINT_RULES: &[&str] = &["no-circular-deps", "enforce-layer"];

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
                        "canonical import id '{}' already declared by module '{}'",
                        canonical_id, previous_module
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
    if spec.version.trim() != SUPPORTED_SPEC_VERSION {
        report.push_error(
            spec,
            format!(
                "unsupported spec version '{}'; expected '{}'",
                spec.version, SUPPORTED_SPEC_VERSION
            ),
        );
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

        let allow_set: BTreeSet<&str> = boundaries
            .allow_imports_from
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
                format!(
                    "module '{}' is in both allow_imports_from and never_imports",
                    overlap
                ),
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
    fn overlap_is_warning() {
        let mut spec = base_spec("orders");
        spec.boundaries = Some(Boundaries {
            allow_imports_from: vec!["payments".to_string()],
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
}
