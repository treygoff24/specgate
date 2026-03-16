use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::deterministic::normalize_repo_relative;
use crate::graph::DependencyGraph;
use crate::spec::SpecFile;

/// Canonical rule id for the unique-export enforcement constraint.
pub const UNIQUE_EXPORT_RULE_ID: &str = "boundary.unique_export";

/// Parsed configuration for a `boundary.unique_export` constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueExportConfig {
    /// Specific export names to guard. Empty means guard all exports.
    pub exports: Vec<String>,
}

/// A violation emitted by the unique-export rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueExportViolation {
    pub module: String,
    pub export_name: String,
    pub files: Vec<PathBuf>,
    pub message: String,
    pub fix_hint: String,
}

/// Config issues found during rule setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueExportConfigIssue {
    pub module: String,
    pub message: String,
}

/// Full report from the unique-export rule evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueExportReport {
    pub violations: Vec<UniqueExportViolation>,
    pub config_issues: Vec<UniqueExportConfigIssue>,
}

impl UniqueExportReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty() && self.config_issues.is_empty()
    }
}

/// Parse error for unique-export constraint params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueExportConfigParseError {
    pub message: String,
}

impl std::fmt::Display for UniqueExportConfigParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for UniqueExportConfigParseError {}

/// Parse a `boundary.unique_export` constraint's params.
///
/// Accepted formats:
/// - `{}` — guard all exports within the module for uniqueness.
/// - `{ "exports": ["nameA", "nameB"] }` — guard only listed export names.
pub fn parse_unique_export_config(
    params: &Value,
) -> Result<UniqueExportConfig, UniqueExportConfigParseError> {
    let Value::Object(map) = params else {
        return Err(UniqueExportConfigParseError {
            message: "params for rule 'boundary.unique_export' must be an object".to_string(),
        });
    };

    let exports = if let Some(exports_value) = map.get("exports") {
        let Value::Array(raw_exports) = exports_value else {
            return Err(UniqueExportConfigParseError {
                message:
                    "params.exports for rule 'boundary.unique_export' must be an array of strings"
                        .to_string(),
            });
        };

        let mut exports = Vec::with_capacity(raw_exports.len());
        for entry in raw_exports {
            let Value::String(name) = entry else {
                return Err(UniqueExportConfigParseError {
                    message:
                        "params.exports for rule 'boundary.unique_export' must contain only strings"
                            .to_string(),
                });
            };

            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(UniqueExportConfigParseError {
                    message:
                        "params.exports for rule 'boundary.unique_export' cannot contain empty names"
                            .to_string(),
                });
            }

            exports.push(trimmed.to_string());
        }

        exports
    } else {
        Vec::new()
    };

    Ok(UniqueExportConfig { exports })
}

/// Evaluate the `boundary.unique_export` constraint across all specs.
///
/// For each module declaring this constraint, the rule scans all files
/// in the module boundary for duplicate export names. When `exports`
/// is non-empty in the config, only those names are checked; otherwise
/// all exports are checked.
pub fn evaluate_unique_export(
    project_root: &std::path::Path,
    specs: &[SpecFile],
    graph: &DependencyGraph,
) -> UniqueExportReport {
    let mut config_issues = Vec::new();

    // Collect constraints, keyed by (module, params string) for dedup.
    let mut configured_constraints: Vec<(String, Value)> = specs
        .iter()
        .flat_map(|spec| {
            spec.constraints
                .iter()
                .filter(|constraint| constraint.rule == UNIQUE_EXPORT_RULE_ID)
                .map(|constraint| (spec.module.clone(), constraint.params.clone()))
                .collect::<Vec<_>>()
        })
        .collect();

    // Deterministic ordering.
    configured_constraints.sort_by(|(left_module, left_params), (right_module, right_params)| {
        left_module
            .cmp(right_module)
            .then_with(|| left_params.to_string().cmp(&right_params.to_string()))
    });
    configured_constraints.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut usable_configs: Vec<(String, UniqueExportConfig)> = Vec::new();

    for (module, params) in configured_constraints {
        match parse_unique_export_config(&params) {
            Ok(config) => usable_configs.push((module, config)),
            Err(error) => config_issues.push(UniqueExportConfigIssue {
                module,
                message: error.message,
            }),
        }
    }

    let mut violations = Vec::new();

    for (module, config) in &usable_configs {
        let files_in_module = graph.files_in_module(module);

        // Build a map of export name -> list of files that export it.
        let mut export_locations: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

        for file_node in &files_in_module {
            for export in &file_node.analysis.exports {
                // Skip __default pseudo-export; it's always unique per file.
                if export.name == "__default" {
                    continue;
                }

                // If specific exports are declared, only check those.
                if !config.exports.is_empty() && !config.exports.contains(&export.name) {
                    continue;
                }

                export_locations
                    .entry(export.name.clone())
                    .or_default()
                    .push(file_node.path.clone());
            }

            // Also check re-exports that introduce named bindings.
            for re_export in &file_node.analysis.re_exports {
                if re_export.is_star {
                    // Star re-exports don't introduce specific named bindings
                    // at the static level we can check.
                    continue;
                }
                for name in &re_export.names {
                    if !config.exports.is_empty() && !config.exports.contains(name) {
                        continue;
                    }

                    export_locations
                        .entry(name.clone())
                        .or_default()
                        .push(file_node.path.clone());
                }
            }
        }

        // Find duplicates.
        for (export_name, mut locations) in export_locations {
            if locations.len() <= 1 {
                continue;
            }

            // Deduplicate and sort for determinism.
            locations.sort();
            locations.dedup();

            if locations.len() <= 1 {
                continue;
            }

            let file_list: Vec<String> = locations
                .iter()
                .map(|p| normalize_repo_relative(project_root, p))
                .collect();

            let file_count = file_list.len();
            let file_list_str = file_list.join(", ");
            let message = format!(
                "duplicate export '{export_name}' in module '{module}' found in {file_count} files: {file_list_str}"
            );

            let fix_hint = format!(
                "Remove or rename the duplicate export '{export_name}' so each export name is unique within module '{module}'."
            );

            violations.push(UniqueExportViolation {
                module: module.clone(),
                export_name,
                files: locations,
                message,
                fix_hint,
            });
        }
    }

    // Deterministic sort.
    violations.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.export_name.cmp(&b.export_name))
    });

    config_issues.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.message.cmp(&b.message))
    });
    config_issues.dedup();

    UniqueExportReport {
        violations,
        config_issues,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use crate::graph::DependencyGraph;
    use crate::resolver::ModuleResolver;
    use crate::spec::{Boundaries, Constraint, Severity, SpecConfig, SpecFile};

    use super::*;

    fn spec_with_constraint(module: &str, path: &str, params: Value) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(path.to_string()),
                ..Boundaries::default()
            }),
            constraints: vec![Constraint {
                rule: UNIQUE_EXPORT_RULE_ID.to_string(),
                params,
                severity: Severity::Error,
                message: None,
            }],
            spec_path: None,
        }
    }

    fn spec_without_constraint(module: &str, path: &str) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(path.to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        }
    }

    fn build_graph(temp: &TempDir, specs: &[SpecFile]) -> DependencyGraph {
        let mut resolver = ModuleResolver::new(temp.path(), specs).expect("resolver");
        let config = SpecConfig::default();
        DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph")
    }

    // === Config Parsing Tests ===

    #[test]
    fn parse_config_accepts_empty_params() {
        let config = parse_unique_export_config(&json!({})).expect("valid config");
        assert!(config.exports.is_empty());
    }

    #[test]
    fn parse_config_accepts_explicit_exports() {
        let config = parse_unique_export_config(&json!({
            "exports": ["getData", "setData"]
        }))
        .expect("valid config");
        assert_eq!(config.exports, vec!["getData", "setData"]);
    }

    #[test]
    fn parse_config_rejects_non_object() {
        assert!(parse_unique_export_config(&json!([])).is_err());
        assert!(parse_unique_export_config(&json!("string")).is_err());
        assert!(parse_unique_export_config(&json!(42)).is_err());
    }

    #[test]
    fn parse_config_rejects_non_array_exports() {
        assert!(parse_unique_export_config(&json!({"exports": "bad"})).is_err());
    }

    #[test]
    fn parse_config_rejects_non_string_in_exports() {
        assert!(parse_unique_export_config(&json!({"exports": [1]})).is_err());
    }

    #[test]
    fn parse_config_rejects_empty_string_in_exports() {
        assert!(parse_unique_export_config(&json!({"exports": [""]})).is_err());
        assert!(parse_unique_export_config(&json!({"exports": ["  "]})).is_err());
    }

    // === Evaluation Tests ===

    #[test]
    fn no_violations_when_exports_are_unique() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export const alpha = 1;\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export const beta = 2;\n",
        )
        .expect("write beta");

        let specs = vec![spec_with_constraint("tools", "src/tools/**/*", json!({}))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.config_issues.is_empty());
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn detects_duplicate_export_names_across_files() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export const sharedName = 1;\nexport const alpha = 2;\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export const sharedName = 3;\nexport const beta = 4;\n",
        )
        .expect("write beta");

        let specs = vec![spec_with_constraint("tools", "src/tools/**/*", json!({}))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.config_issues.is_empty());
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].export_name, "sharedName");
        assert_eq!(report.violations[0].module, "tools");
        assert_eq!(report.violations[0].files.len(), 2);
        assert!(report.violations[0].message.contains("duplicate export"));
    }

    #[test]
    fn scoped_exports_only_checks_listed_names() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export const sharedName = 1;\nexport const tracked = 10;\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export const sharedName = 3;\nexport const tracked = 20;\n",
        )
        .expect("write beta");

        // Only guard "tracked", not "sharedName"
        let specs = vec![spec_with_constraint(
            "tools",
            "src/tools/**/*",
            json!({"exports": ["tracked"]}),
        )];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.config_issues.is_empty());
        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].export_name, "tracked");
    }

    #[test]
    fn no_constraint_means_no_violations() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export const sharedName = 1;\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export const sharedName = 2;\n",
        )
        .expect("write beta");

        let specs = vec![spec_without_constraint("tools", "src/tools/**/*")];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.violations.is_empty());
        assert!(report.config_issues.is_empty());
    }

    #[test]
    fn malformed_params_are_reported_as_config_issues() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "export const a = 1;\n").expect("write a");

        let specs = vec![spec_with_constraint("core", "src/**/*", json!([]))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.violations.is_empty());
        assert_eq!(report.config_issues.len(), 1);
        assert!(
            report.config_issues[0]
                .message
                .contains("must be an object")
        );
    }

    #[test]
    fn default_exports_are_excluded_from_uniqueness_check() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export default function handler() {}\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export default class Widget {}\n",
        )
        .expect("write beta");

        let specs = vec![spec_with_constraint("tools", "src/tools/**/*", json!({}))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(
            report.violations.is_empty(),
            "default exports should be excluded: {report:?}"
        );
    }

    #[test]
    fn violations_are_deterministic() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/a.ts"),
            "export const foo = 1;\nexport const bar = 2;\n",
        )
        .expect("write a");
        fs::write(
            temp.path().join("src/tools/b.ts"),
            "export const foo = 3;\nexport const bar = 4;\n",
        )
        .expect("write b");

        let specs = vec![spec_with_constraint("tools", "src/tools/**/*", json!({}))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert_eq!(report.violations.len(), 2);
        // Should be sorted by export_name: bar before foo
        assert_eq!(report.violations[0].export_name, "bar");
        assert_eq!(report.violations[1].export_name, "foo");
    }

    #[test]
    fn duplicate_export_in_different_modules_does_not_trigger_violation() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/payments")).expect("mkdir payments");
        fs::create_dir_all(temp.path().join("src/billing")).expect("mkdir billing");
        fs::write(
            temp.path().join("src/payments/utils.ts"),
            "export function formatDate(d: Date) { return d.toISOString(); }\n",
        )
        .expect("write payments utils");
        fs::write(
            temp.path().join("src/billing/utils.ts"),
            "export function formatDate(d: Date) { return d.toLocaleDateString(); }\n",
        )
        .expect("write billing utils");

        // Both modules have the unique_export constraint, but each module's
        // formatDate export lives in a different module — no intra-module
        // duplication, so zero violations expected.
        let specs = vec![
            spec_with_constraint("payments", "src/payments/**/*", json!({})),
            spec_with_constraint("billing", "src/billing/**/*", json!({})),
        ];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert!(report.config_issues.is_empty());
        assert!(
            report.violations.is_empty(),
            "same export name in different modules must not produce violations: {report:?}"
        );
    }

    #[test]
    fn re_exports_are_included_in_uniqueness_check() {
        let temp = TempDir::new().expect("tempdir");

        fs::create_dir_all(temp.path().join("src/tools")).expect("mkdir");
        fs::write(
            temp.path().join("src/tools/alpha.ts"),
            "export const sharedName = 1;\n",
        )
        .expect("write alpha");
        fs::write(
            temp.path().join("src/tools/beta.ts"),
            "export { sharedName } from './alpha';\n",
        )
        .expect("write beta");

        let specs = vec![spec_with_constraint("tools", "src/tools/**/*", json!({}))];
        let graph = build_graph(&temp, &specs);
        let report = evaluate_unique_export(temp.path(), &specs, &graph);

        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].export_name, "sharedName");
    }
}
