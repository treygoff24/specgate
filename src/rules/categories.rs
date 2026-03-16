use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

use crate::graph::DependencyGraph;
use crate::spec::SpecFile;

pub const ENFORCE_CATEGORY_RULE_ID: &str = "enforce-category";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforceCategoryConfig {
    /// Descriptive label for this category set (e.g. "domains").
    pub category: String,
    /// Member names that map to module-id first segments.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryViolation {
    pub from_module: String,
    pub to_module: String,
    pub from_category: String,
    pub to_category: String,
    pub from_file: PathBuf,
    pub to_file: PathBuf,
    pub specifier: String,
    pub message: String,
    pub fix_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryConfigIssue {
    pub module: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforceCategoryReport {
    pub violations: Vec<CategoryViolation>,
    pub config_issues: Vec<CategoryConfigIssue>,
}

impl EnforceCategoryReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty() && self.config_issues.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryConfigParseError {
    pub message: String,
}

impl std::fmt::Display for CategoryConfigParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CategoryConfigParseError {}

pub fn parse_enforce_category_config(
    params: &Value,
) -> Result<EnforceCategoryConfig, CategoryConfigParseError> {
    let Value::Object(map) = params else {
        return Err(CategoryConfigParseError {
            message:
                "params for rule 'enforce-category' must be an object with keys 'category' and 'members'"
                    .to_string(),
        });
    };

    let Some(category_value) = map.get("category") else {
        return Err(CategoryConfigParseError {
            message: "params for rule 'enforce-category' must include non-empty 'category' string"
                .to_string(),
        });
    };

    let Value::String(category) = category_value else {
        return Err(CategoryConfigParseError {
            message: "params.category for rule 'enforce-category' must be a non-empty string"
                .to_string(),
        });
    };

    let category = category.trim();
    if category.is_empty() {
        return Err(CategoryConfigParseError {
            message: "params.category for rule 'enforce-category' must be a non-empty string"
                .to_string(),
        });
    }

    let Some(members_value) = map.get("members") else {
        return Err(CategoryConfigParseError {
            message: "params for rule 'enforce-category' must include non-empty 'members' array"
                .to_string(),
        });
    };

    let Value::Array(raw_members) = members_value else {
        return Err(CategoryConfigParseError {
            message:
                "params.members for rule 'enforce-category' must be an array of non-empty strings"
                    .to_string(),
        });
    };

    if raw_members.is_empty() {
        return Err(CategoryConfigParseError {
            message: "params.members for rule 'enforce-category' must not be empty".to_string(),
        });
    }

    let mut members = Vec::with_capacity(raw_members.len());
    let mut seen = BTreeSet::new();

    for member in raw_members {
        let Value::String(member) = member else {
            return Err(CategoryConfigParseError {
                message: "params.members for rule 'enforce-category' must contain only strings"
                    .to_string(),
            });
        };

        let member = member.trim();
        if member.is_empty() {
            return Err(CategoryConfigParseError {
                message:
                    "params.members for rule 'enforce-category' cannot contain empty member names"
                        .to_string(),
            });
        }

        if !seen.insert(member.to_string()) {
            return Err(CategoryConfigParseError {
                message: format!(
                    "params.members for rule 'enforce-category' contains duplicate member '{member}'"
                ),
            });
        }

        members.push(member.to_string());
    }

    Ok(EnforceCategoryConfig {
        category: category.to_string(),
        members,
    })
}

/// Resolve a module id to its category.
///
/// Convention: category is the first non-empty `/`-delimited segment of the module id.
/// This is the same convention used by `layer_for_module`.
pub fn category_for_module(module_id: &str) -> Option<&str> {
    super::module_group_segment(module_id)
}

pub fn evaluate_enforce_category(
    specs: &[SpecFile],
    graph: &DependencyGraph,
) -> EnforceCategoryReport {
    let mut config_issues = Vec::new();

    let mut configured_constraints = specs
        .iter()
        .flat_map(|spec| {
            spec.constraints
                .iter()
                .filter(|constraint| constraint.rule == ENFORCE_CATEGORY_RULE_ID)
                .map(|constraint| (spec.module.clone(), constraint.params.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    // Deterministic ordering even when multiple constraints exist per module.
    configured_constraints.sort_by(|(left_module, left_params), (right_module, right_params)| {
        left_module
            .cmp(right_module)
            .then_with(|| left_params.to_string().cmp(&right_params.to_string()))
    });

    // De-dupe exact repeated declarations before parsing.
    configured_constraints.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let mut usable_configs = Vec::new();

    for (module, params) in configured_constraints {
        match parse_enforce_category_config(&params) {
            Ok(config) => usable_configs.push((module, config)),
            Err(error) => config_issues.push(CategoryConfigIssue {
                module,
                message: error.message,
            }),
        }
    }

    usable_configs.sort_by(|(left_module, left_config), (right_module, right_config)| {
        left_module.cmp(right_module).then_with(|| {
            left_config
                .category
                .cmp(&right_config.category)
                .then_with(|| left_config.members.cmp(&right_config.members))
        })
    });

    let Some((canonical_module, canonical_config)) = usable_configs.first().cloned() else {
        config_issues.sort_by(|a, b| {
            a.module
                .cmp(&b.module)
                .then_with(|| a.message.cmp(&b.message))
        });
        config_issues.dedup();
        return EnforceCategoryReport {
            violations: Vec::new(),
            config_issues,
        };
    };

    // Deterministic + explicit conflict policy:
    // choose the lexicographically first module declaration as canonical, and report all mismatches.
    for (module, config) in usable_configs.iter().skip(1) {
        if config != &canonical_config {
            config_issues.push(CategoryConfigIssue {
                module: module.clone(),
                message: format!(
                    "conflicting enforce-category config; using canonical category '{}' with members {:?} from module '{}' (deterministic: lexicographically first module id). This module declared category '{}' with members {:?}",
                    canonical_config.category, canonical_config.members, canonical_module,
                    config.category, config.members
                ),
            });
        }
    }

    let member_set: BTreeSet<&str> = canonical_config
        .members
        .iter()
        .map(String::as_str)
        .collect();

    let mut violations = Vec::new();

    for edge in graph.dependency_edges() {
        let Some(from_module) = graph.module_of_file(&edge.from).map(str::to_string) else {
            continue;
        };
        let Some(to_module) = graph.module_of_file(&edge.to).map(str::to_string) else {
            continue;
        };

        if from_module == to_module {
            continue;
        }

        let Some(from_category) = category_for_module(&from_module).map(str::to_string) else {
            continue;
        };
        let Some(to_category) = category_for_module(&to_module).map(str::to_string) else {
            continue;
        };

        // Only enforce within the governed member set
        if !member_set.contains(from_category.as_str())
            || !member_set.contains(to_category.as_str())
        {
            continue;
        }

        // Same category is allowed
        if from_category == to_category {
            continue;
        }

        let message = format!(
            "forbidden cross-category import: module '{}' (category '{}') imports '{}' (category '{}') via '{}'; category '{}' isolation policy forbids this",
            from_module,
            from_category,
            to_module,
            to_category,
            edge.specifier,
            canonical_config.category
        );

        let fix_hint = format!(
            "Modules within category '{}' members {:?} must not import across category boundaries. Move shared logic into a module outside these categories or restructure to avoid the cross-category dependency.",
            canonical_config.category, canonical_config.members
        );

        violations.push(CategoryViolation {
            from_module,
            to_module,
            from_category,
            to_category,
            from_file: edge.from,
            to_file: edge.to,
            specifier: edge.specifier,
            message,
            fix_hint,
        });
    }

    violations.sort_by(|a, b| {
        a.from_module
            .cmp(&b.from_module)
            .then_with(|| a.to_module.cmp(&b.to_module))
            .then_with(|| a.specifier.cmp(&b.specifier))
            .then_with(|| a.from_file.cmp(&b.from_file))
            .then_with(|| a.to_file.cmp(&b.to_file))
    });

    config_issues.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.message.cmp(&b.message))
    });
    config_issues.dedup();

    EnforceCategoryReport {
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
    use crate::spec::{Boundaries, Constraint, Severity, SpecConfig};

    use super::*;

    fn spec(module: &str, path: &str, with_rule: bool, params: Value) -> SpecFile {
        let constraints = if with_rule {
            vec![Constraint {
                rule: ENFORCE_CATEGORY_RULE_ID.to_string(),
                params,
                severity: Severity::Error,
                message: None,
            }]
        } else {
            Vec::new()
        };

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
            constraints,
            spec_path: None,
        }
    }

    fn build_graph(temp: &TempDir, specs: &[SpecFile]) -> DependencyGraph {
        let mut resolver = ModuleResolver::new(temp.path(), specs).expect("resolver");
        let config = SpecConfig::default();
        DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph")
    }

    #[test]
    fn parse_config_rejects_malformed_params() {
        let invalid = [
            json!([]),
            json!({}),
            json!({"category": "domains"}),
            json!({"members": ["auth"]}),
            json!({"category": "domains", "members": "auth"}),
            json!({"category": "domains", "members": []}),
            json!({"category": "domains", "members": ["auth", "auth"]}),
            json!({"category": "domains", "members": ["auth", 1]}),
            json!({"category": "domains", "members": [" "]}),
            json!({"category": "", "members": ["auth"]}),
            json!({"category": 42, "members": ["auth"]}),
        ];

        for params in invalid {
            assert!(
                parse_enforce_category_config(&params).is_err(),
                "expected error for: {params}"
            );
        }
    }

    #[test]
    fn parse_config_accepts_valid_params() {
        let params = json!({"category": "domains", "members": ["auth", "billing", "orders"]});
        let config = parse_enforce_category_config(&params).expect("valid config");
        assert_eq!(config.category, "domains");
        assert_eq!(config.members, vec!["auth", "billing", "orders"]);
    }

    #[test]
    fn same_category_imports_are_allowed() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/auth/login")).expect("mkdir");
        fs::create_dir_all(temp.path().join("src/auth/session")).expect("mkdir");

        fs::write(
            temp.path().join("src/auth/login/index.ts"),
            "import { session } from '../session/index'; export const login = session;\n",
        )
        .expect("write login");
        fs::write(
            temp.path().join("src/auth/session/index.ts"),
            "export const session = 1;\n",
        )
        .expect("write session");

        let params = json!({"category": "domains", "members": ["auth", "billing"]});
        let specs = vec![
            spec("auth/login", "src/auth/login/**/*", true, params.clone()),
            spec("auth/session", "src/auth/session/**/*", false, params),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_category(&specs, &graph);

        assert!(report.config_issues.is_empty());
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn cross_category_imports_are_flagged() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/auth/login")).expect("mkdir");
        fs::create_dir_all(temp.path().join("src/billing/invoices")).expect("mkdir");

        fs::write(
            temp.path().join("src/auth/login/index.ts"),
            "import { invoice } from '../../billing/invoices/index'; export const result = invoice;\n",
        )
        .expect("write login");
        fs::write(
            temp.path().join("src/billing/invoices/index.ts"),
            "export const invoice = 1;\n",
        )
        .expect("write invoices");

        let params = json!({"category": "domains", "members": ["auth", "billing"]});
        let specs = vec![
            spec("auth/login", "src/auth/login/**/*", true, params.clone()),
            spec(
                "billing/invoices",
                "src/billing/invoices/**/*",
                false,
                params,
            ),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_category(&specs, &graph);

        assert!(report.config_issues.is_empty());
        assert_eq!(report.violations.len(), 1);

        let violation = &report.violations[0];
        assert_eq!(violation.from_module, "auth/login");
        assert_eq!(violation.to_module, "billing/invoices");
        assert_eq!(violation.from_category, "auth");
        assert_eq!(violation.to_category, "billing");
        assert!(
            violation
                .message
                .contains("forbidden cross-category import")
        );
        assert!(
            violation
                .fix_hint
                .contains("must not import across category boundaries")
        );
    }

    #[test]
    fn imports_to_non_member_modules_are_allowed() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/auth/login")).expect("mkdir");
        fs::create_dir_all(temp.path().join("src/shared/utils")).expect("mkdir");

        fs::write(
            temp.path().join("src/auth/login/index.ts"),
            "import { util } from '../../shared/utils/index'; export const result = util;\n",
        )
        .expect("write login");
        fs::write(
            temp.path().join("src/shared/utils/index.ts"),
            "export const util = 1;\n",
        )
        .expect("write utils");

        let params = json!({"category": "domains", "members": ["auth", "billing"]});
        let specs = vec![
            spec("auth/login", "src/auth/login/**/*", true, params.clone()),
            spec("shared/utils", "src/shared/utils/**/*", false, params),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_category(&specs, &graph);

        assert!(report.config_issues.is_empty());
        assert!(
            report.violations.is_empty(),
            "imports to non-member modules should be allowed: {report:?}"
        );
    }

    #[test]
    fn malformed_constraint_params_are_reported_as_config_issues() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/core")).expect("mkdir core");
        fs::write(temp.path().join("src/core/a.ts"), "export const a = 1;\n").expect("write a");

        let specs = vec![spec(
            "core",
            "src/core/**/*",
            true,
            json!({"category": "domains", "members": "auth"}),
        )];
        let graph = build_graph(&temp, &specs);

        let report = evaluate_enforce_category(&specs, &graph);
        assert!(report.violations.is_empty());
        assert_eq!(report.config_issues.len(), 1);
        assert!(report.config_issues[0].message.contains("params.members"));
    }

    #[test]
    fn conflicting_configs_are_deterministic_and_explicit() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/auth")).expect("mkdir auth");
        fs::create_dir_all(temp.path().join("src/billing")).expect("mkdir billing");

        fs::write(temp.path().join("src/auth/a.ts"), "export const a = 1;\n").expect("write auth");
        fs::write(
            temp.path().join("src/billing/b.ts"),
            "export const b = 1;\n",
        )
        .expect("write billing");

        let canonical_params = json!({"category": "domains", "members": ["auth", "billing"]});
        let conflicting_params = json!({"category": "verticals", "members": ["billing", "orders"]});

        let specs_left = vec![
            spec(
                "billing/invoices",
                "src/billing/**/*",
                true,
                conflicting_params.clone(),
            ),
            spec(
                "auth/login",
                "src/auth/**/*",
                true,
                canonical_params.clone(),
            ),
        ];
        let specs_right = vec![
            spec(
                "auth/login",
                "src/auth/**/*",
                true,
                canonical_params.clone(),
            ),
            spec(
                "billing/invoices",
                "src/billing/**/*",
                true,
                conflicting_params,
            ),
        ];

        let graph_left = build_graph(&temp, &specs_left);
        let graph_right = build_graph(&temp, &specs_right);

        let report_left = evaluate_enforce_category(&specs_left, &graph_left);
        let report_right = evaluate_enforce_category(&specs_right, &graph_right);

        assert_eq!(report_left, report_right);
        assert_eq!(report_left.config_issues.len(), 1);

        let issue = &report_left.config_issues[0];
        assert_eq!(issue.module, "billing/invoices");
        assert!(
            issue
                .message
                .contains("conflicting enforce-category config")
        );
        assert!(
            issue
                .message
                .contains("deterministic: lexicographically first module id")
        );
    }

    #[test]
    fn category_for_module_uses_first_path_segment() {
        assert_eq!(category_for_module("auth/login"), Some("auth"));
        assert_eq!(category_for_module("billing"), Some("billing"));
        assert_eq!(category_for_module("/leading/slash"), Some("leading"));
        assert_eq!(category_for_module("///"), None);
    }
}
