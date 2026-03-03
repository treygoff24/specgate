use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use globset::GlobSet;
use miette::Diagnostic;
use thiserror::Error;

use crate::graph::discovery;
use crate::parser;
use crate::resolver::{ModuleResolver, ResolvedImport};
use crate::spec::{Boundaries, SpecConfig, SpecFile};

use super::{
    compile_optional_globset_strict, matches_test_file, normalized_string_set,
    sort_violations_stable, GlobCompileError, RuleContext, RuleViolation, RuleWithResolver,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyViolationKind {
    ForbiddenDependency,
    DependencyNotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyViolation {
    pub module_id: String,
    pub file: PathBuf,
    pub specifier: String,
    pub package_name: String,
    pub kind: DependencyViolationKind,
    pub is_test_file: bool,
}

#[derive(Debug, Error, Diagnostic)]
pub enum DependencyRuleError {
    #[error(transparent)]
    Discovery {
        #[from]
        source: discovery::DiscoveryError,
    },
    #[error(transparent)]
    Parse {
        #[from]
        source: parser::ParserError,
    },
    #[error("invalid test glob pattern '{pattern}': {source}")]
    InvalidTestGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

pub type Result<T> = std::result::Result<T, DependencyRuleError>;

pub const DEPENDENCY_FORBIDDEN_RULE_ID: &str = "dependency.forbidden";
pub const DEPENDENCY_NOT_ALLOWED_RULE_ID: &str = "dependency.not_allowed";

/// Dependency policy rule bridge that integrates with [`RuleContext`] and a mutable resolver.
#[derive(Debug, Default, Clone, Copy)]
pub struct DependencyRule;

impl RuleWithResolver for DependencyRule {
    type Error = DependencyRuleError;

    fn evaluate_with_resolver(
        &self,
        ctx: &RuleContext<'_>,
        resolver: &mut ModuleResolver,
    ) -> Result<Vec<RuleViolation>> {
        let typed = evaluate_dependency_rules(ctx.project_root, resolver, ctx.specs, ctx.config)?;
        Ok(typed_violations_to_rule_violations(typed))
    }
}

#[derive(Debug, Clone, Default)]
struct DependencyPolicy {
    forbidden_dependencies: BTreeSet<String>,
    allowed_dependencies: BTreeSet<String>,
    enforce_in_tests: bool,
}

/// Evaluate third-party dependency boundary policies for all files in the repository.
///
/// Semantics:
/// - `forbidden_dependencies` is always enforced first (deny precedence).
/// - `allowed_dependencies` acts as a default-deny list when non-empty.
/// - `allowed_dependencies` is skipped for test files unless `enforce_in_tests` is true.
/// - `forbidden_dependencies` still applies in tests regardless of `enforce_in_tests`.
pub fn evaluate_dependency_rules(
    project_root: &Path,
    resolver: &mut ModuleResolver,
    specs: &[SpecFile],
    config: &SpecConfig,
) -> Result<Vec<DependencyViolation>> {
    let policies = module_policies(specs);
    let test_matcher = build_test_globset(&config.test_patterns)?;

    let discovered = discovery::discover_source_files(project_root, &config.exclude)?;

    let mut violations = Vec::new();

    for file in discovered.files {
        let Some(module_id) = resolver.module_for_file(&file).map(ToString::to_string) else {
            continue;
        };

        let Some(policy) = policies.get(&module_id) else {
            continue;
        };

        if policy.forbidden_dependencies.is_empty() && policy.allowed_dependencies.is_empty() {
            continue;
        }

        let analysis = parser::parse_file(&file)?;
        let is_test = matches_test_file(project_root, &file, test_matcher.as_ref());

        for specifier in analysis.dependency_specifiers() {
            let ResolvedImport::ThirdParty { package_name } = resolver.resolve(&file, &specifier)
            else {
                continue;
            };

            if policy.forbidden_dependencies.contains(&package_name) {
                violations.push(DependencyViolation {
                    module_id: module_id.clone(),
                    file: file.clone(),
                    specifier,
                    package_name,
                    kind: DependencyViolationKind::ForbiddenDependency,
                    is_test_file: is_test,
                });
                continue;
            }

            if policy.allowed_dependencies.is_empty() {
                continue;
            }

            if is_test && !policy.enforce_in_tests {
                continue;
            }

            if !policy.allowed_dependencies.contains(&package_name) {
                violations.push(DependencyViolation {
                    module_id: module_id.clone(),
                    file: file.clone(),
                    specifier,
                    package_name,
                    kind: DependencyViolationKind::DependencyNotAllowed,
                    is_test_file: is_test,
                });
            }
        }
    }

    violations.sort_by(|a, b| {
        a.module_id
            .cmp(&b.module_id)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.package_name.cmp(&b.package_name))
            .then_with(|| a.specifier.cmp(&b.specifier))
            .then_with(|| a.is_test_file.cmp(&b.is_test_file))
    });

    Ok(violations)
}

fn typed_violations_to_rule_violations(typed: Vec<DependencyViolation>) -> Vec<RuleViolation> {
    let mut violations = typed
        .into_iter()
        .map(|violation| {
            let (rule, message) = match violation.kind {
                DependencyViolationKind::ForbiddenDependency => (
                    DEPENDENCY_FORBIDDEN_RULE_ID,
                    format!(
                        "module '{}' forbids dependency '{}' (import '{}')",
                        violation.module_id, violation.package_name, violation.specifier
                    ),
                ),
                DependencyViolationKind::DependencyNotAllowed => (
                    DEPENDENCY_NOT_ALLOWED_RULE_ID,
                    format!(
                        "module '{}' does not allow dependency '{}' (import '{}')",
                        violation.module_id, violation.package_name, violation.specifier
                    ),
                ),
            };

            RuleViolation {
                rule: rule.to_string(),
                message,
                from_file: violation.file,
                to_file: None,
                from_module: Some(violation.module_id),
                to_module: None,
                line: None,
                column: None,
            }
        })
        .collect::<Vec<_>>();

    sort_violations_stable(&mut violations);
    violations
}

fn module_policies(specs: &[SpecFile]) -> BTreeMap<String, DependencyPolicy> {
    let mut policies = BTreeMap::new();

    for spec in specs {
        let boundaries = spec.boundaries.clone().unwrap_or_else(Boundaries::default);

        policies.insert(
            spec.module.clone(),
            DependencyPolicy {
                forbidden_dependencies: normalized_string_set(&boundaries.forbidden_dependencies),
                allowed_dependencies: normalized_string_set(&boundaries.allowed_dependencies),
                enforce_in_tests: boundaries.enforce_in_tests,
            },
        );
    }

    policies
}

fn build_test_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    compile_optional_globset_strict(patterns).map_err(|error| match error {
        GlobCompileError::InvalidPattern { pattern, source } => {
            DependencyRuleError::InvalidTestGlob { pattern, source }
        }
        GlobCompileError::Build { source } => DependencyRuleError::InvalidTestGlob {
            pattern: "<globset>".to_string(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::deterministic::normalize_repo_relative;
    use crate::rules::{write_test_file, RuleContext, RuleWithResolver};
    use crate::spec::{Boundaries, SpecConfig};

    use super::*;

    fn spec_with_boundaries(module: &str, path: &str, boundaries: Boundaries) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(path.to_string()),
                ..boundaries
            }),
            constraints: Vec::new(),
            spec_path: None,
        }
    }

    fn write_npm_package(root: &std::path::Path, package_name: &str) {
        let package_dir = package_name
            .split('/')
            .fold(root.join("node_modules"), |acc, part| acc.join(part));

        fs::create_dir_all(&package_dir).expect("mkdir package");
        fs::write(
            package_dir.join("package.json"),
            format!("{{\"name\":\"{package_name}\",\"main\":\"index.js\"}}"),
        )
        .expect("write package json");
        fs::write(package_dir.join("index.js"), "module.exports = {};\n")
            .expect("write package entry");
    }

    #[test]
    fn forbidden_dependencies_have_precedence_over_allowlist() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");
        fs::write(
            temp.path().join("src/app/main.ts"),
            "import lodash from 'lodash';\n",
        )
        .expect("write main");
        write_npm_package(temp.path(), "lodash");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                allowed_dependencies: vec!["lodash".to_string()],
                forbidden_dependencies: vec!["lodash".to_string()],
                ..Boundaries::default()
            },
        )];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].kind,
            DependencyViolationKind::ForbiddenDependency
        );
    }

    #[test]
    fn allowlist_is_default_deny_for_third_party_dependencies() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");
        fs::write(
            temp.path().join("src/app/main.ts"),
            "import lodash from 'lodash'; import { format } from 'date-fns'; import './local';\n",
        )
        .expect("write main");
        fs::write(
            temp.path().join("src/app/local.ts"),
            "export const x = 1;\n",
        )
        .expect("write local");
        write_npm_package(temp.path(), "lodash");
        write_npm_package(temp.path(), "date-fns");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                allowed_dependencies: vec!["lodash".to_string()],
                ..Boundaries::default()
            },
        )];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].package_name, "date-fns");
        assert_eq!(
            violations[0].kind,
            DependencyViolationKind::DependencyNotAllowed
        );
    }

    #[test]
    fn resolver_package_classification_handles_subpaths_and_builtins() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");
        fs::write(
            temp.path().join("src/app/main.ts"),
            r#"
import '@scope/pkg/sub/path';
import 'lodash/fp';
import 'node:fs/promises';
"#,
        )
        .expect("write main");
        write_npm_package(temp.path(), "@scope/pkg");
        fs::create_dir_all(temp.path().join("node_modules/@scope/pkg/sub"))
            .expect("mkdir scoped subpath");
        fs::write(
            temp.path().join("node_modules/@scope/pkg/sub/path.js"),
            "module.exports = {};\n",
        )
        .expect("write scoped subpath");
        write_npm_package(temp.path(), "lodash");
        fs::create_dir_all(temp.path().join("node_modules/lodash")).expect("mkdir lodash");
        fs::write(
            temp.path().join("node_modules/lodash/fp.js"),
            "module.exports = {};\n",
        )
        .expect("write lodash subpath");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                allowed_dependencies: vec!["@scope/pkg".to_string(), "lodash".to_string()],
                forbidden_dependencies: vec!["fs".to_string()],
                ..Boundaries::default()
            },
        )];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].package_name, "fs");
        assert_eq!(violations[0].specifier, "node:fs/promises");
        assert_eq!(
            violations[0].kind,
            DependencyViolationKind::ForbiddenDependency
        );
    }

    #[test]
    fn forbidden_dependencies_apply_in_tests_even_when_enforce_in_tests_is_false() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");
        fs::write(
            temp.path().join("src/app/feature.test.ts"),
            "import 'left-pad';\n",
        )
        .expect("write test file");
        write_npm_package(temp.path(), "left-pad");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                forbidden_dependencies: vec!["left-pad".to_string()],
                enforce_in_tests: false,
                ..Boundaries::default()
            },
        )];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        assert_eq!(violations.len(), 1);
        assert!(violations[0].is_test_file);
        assert_eq!(
            violations[0].kind,
            DependencyViolationKind::ForbiddenDependency
        );
    }

    #[test]
    fn allowlist_checks_are_skipped_for_tests_when_enforce_in_tests_is_false() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/app")).expect("mkdir app");
        fs::write(
            temp.path().join("src/app/feature.test.ts"),
            "import 'left-pad';\n",
        )
        .expect("write test file");
        write_npm_package(temp.path(), "left-pad");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                allowed_dependencies: vec!["lodash".to_string()],
                enforce_in_tests: false,
                ..Boundaries::default()
            },
        )];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        assert!(violations.is_empty());
    }

    #[test]
    fn violations_are_sorted_deterministically() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(temp.path(), "src/b/second.ts", "import 'zod';\n");
        write_test_file(temp.path(), "src/a/first.ts", "import 'axios';\n");
        write_npm_package(temp.path(), "axios");
        write_npm_package(temp.path(), "zod");

        let specs = vec![
            spec_with_boundaries(
                "alpha",
                "src/a/**/*",
                Boundaries {
                    allowed_dependencies: vec!["lodash".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "beta",
                "src/b/**/*",
                Boundaries {
                    allowed_dependencies: vec!["lodash".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();

        let violations =
            evaluate_dependency_rules(temp.path(), &mut resolver, &specs, &config).expect("rules");

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");
        let ordered = violations
            .iter()
            .map(|violation| {
                let rel = normalize_repo_relative(&canonical_root, &violation.file);
                format!(
                    "{}|{}|{}|{}|{:?}",
                    violation.module_id,
                    rel,
                    violation.specifier,
                    violation.package_name,
                    violation.kind
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            ordered,
            vec![
                "alpha|src/a/first.ts|axios|axios|DependencyNotAllowed",
                "beta|src/b/second.ts|zod|zod|DependencyNotAllowed",
            ]
        );
    }

    #[test]
    fn dependency_rule_bridge_evaluates_with_mutable_resolver() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(temp.path(), "src/app/main.ts", "import 'axios';\n");
        write_npm_package(temp.path(), "axios");

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                allowed_dependencies: vec!["lodash".to_string()],
                ..Boundaries::default()
            },
        )];
        let config = SpecConfig::default();
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let graph = crate::graph::DependencyGraph::build(temp.path(), &mut resolver, &config)
            .expect("graph");

        let rule = DependencyRule;
        let ctx = RuleContext {
            project_root: temp.path(),
            config: &config,
            specs: &specs,
            graph: &graph,
        };

        let violations = rule
            .evaluate_with_resolver(&ctx, &mut resolver)
            .expect("dependency rule bridge");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, DEPENDENCY_NOT_ALLOWED_RULE_ID);
        assert_eq!(violations[0].from_module.as_deref(), Some("app"));
    }
}
