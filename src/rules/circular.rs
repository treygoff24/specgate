use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::deterministic::normalize_repo_relative;
use crate::graph::{CycleComponent, DependencyGraph};
use crate::spec::{Severity, SpecFile};

pub const NO_CIRCULAR_DEPS_RULE_ID: &str = "no-circular-deps";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CircularScopeParam {
    Internal,
    External,
    Both,
}

impl CircularScopeParam {
    fn parse(params: &serde_json::Value) -> Result<Self, InvalidCircularScope> {
        let Some(scope) = params.get("scope").and_then(serde_json::Value::as_str) else {
            return Ok(Self::Both);
        };

        match scope.trim().to_ascii_lowercase().as_str() {
            "internal" => Ok(Self::Internal),
            "external" => Ok(Self::External),
            "both" => Ok(Self::Both),
            _ => Err(InvalidCircularScope),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InvalidCircularScope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircularDependencyViolation {
    /// Module that declared the `no-circular-deps` constraint.
    pub module: String,
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub scope: CircularScopeParam,
    /// Deterministically sorted file members of the cyclic component.
    pub component_files: Vec<PathBuf>,
    /// Deterministically sorted module members of the cyclic component.
    pub component_modules: Vec<String>,
}

#[derive(Debug, Clone)]
struct PrecomputedCycleSets {
    internal: Vec<CycleComponent>,
    external: Vec<CycleComponent>,
    both: Vec<CycleComponent>,
}

/// Evaluate all `no-circular-deps` constraints across loaded specs.
///
/// Each unique configured constraint emits one violation per matching SCC cycle component.
/// Severity + optional custom message are sourced directly from the constraint.
pub fn evaluate_no_circular_deps(
    specs: &[SpecFile],
    graph: &DependencyGraph,
) -> Vec<CircularDependencyViolation> {
    // High-priority perf behavior:
    // compute SCC cycles once, then reuse pre-filtered sets per scope for all constraints.
    let cycles = precompute_cycle_sets(graph);

    // De-duplicate equivalent constraints to avoid repeated identical violations.
    let mut unique_constraints = BTreeMap::new();

    for spec in specs {
        for constraint in &spec.constraints {
            if constraint.rule != NO_CIRCULAR_DEPS_RULE_ID {
                continue;
            }

            let Ok(scope) = CircularScopeParam::parse(&constraint.params) else {
                continue;
            };
            unique_constraints
                .entry((
                    spec.module.clone(),
                    scope,
                    severity_rank(constraint.severity),
                    constraint.message.clone(),
                ))
                .or_insert(constraint.severity);
        }
    }

    let mut violations = Vec::new();

    for ((module, scope, _severity_rank, custom_message), severity) in unique_constraints {
        let scoped_cycles = match scope {
            CircularScopeParam::Internal => &cycles.internal,
            CircularScopeParam::External => &cycles.external,
            CircularScopeParam::Both => &cycles.both,
        };

        for component in scoped_cycles {
            violations.push(CircularDependencyViolation {
                module: module.clone(),
                rule: NO_CIRCULAR_DEPS_RULE_ID.to_string(),
                severity,
                message: custom_message
                    .clone()
                    .unwrap_or_else(|| default_message(scope, component, graph.project_root())),
                scope,
                component_files: component.files.clone(),
                component_modules: component.modules.clone(),
            });
        }
    }

    violations.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.scope.as_str().cmp(b.scope.as_str()))
            .then_with(|| a.component_files.cmp(&b.component_files))
            .then_with(|| a.component_modules.cmp(&b.component_modules))
            .then_with(|| severity_rank(a.severity).cmp(&severity_rank(b.severity)))
            .then_with(|| a.message.cmp(&b.message))
    });

    violations.dedup_by(|left, right| {
        left.module == right.module
            && left.rule == right.rule
            && left.severity == right.severity
            && left.message == right.message
            && left.scope == right.scope
            && left.component_files == right.component_files
            && left.component_modules == right.component_modules
    });

    violations
}

fn precompute_cycle_sets(graph: &DependencyGraph) -> PrecomputedCycleSets {
    let both = graph
        .strongly_connected_components()
        .into_iter()
        .filter(CycleComponent::is_cycle)
        .collect::<Vec<_>>();

    let internal = both
        .iter()
        .filter(|component| component.is_internal())
        .cloned()
        .collect::<Vec<_>>();

    let external = both
        .iter()
        .filter(|component| component.is_external())
        .cloned()
        .collect::<Vec<_>>();

    PrecomputedCycleSets {
        internal,
        external,
        both,
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn default_message(
    scope: CircularScopeParam,
    component: &CycleComponent,
    project_root: &Path,
) -> String {
    let files = component
        .files
        .iter()
        .map(|file| normalize_repo_relative(project_root, file))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "Detected circular dependency component (scope: {}) across files: {}",
        scope.as_str(),
        files
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::deterministic::normalize_path;
    use crate::resolver::ModuleResolver;
    use crate::spec::{Boundaries, Constraint, SpecConfig};

    use super::*;

    fn module_spec(module: &str, path: &str) -> SpecFile {
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

    fn add_constraint(spec: &mut SpecFile, scope: &str, severity: Severity, message: Option<&str>) {
        spec.constraints.push(Constraint {
            rule: NO_CIRCULAR_DEPS_RULE_ID.to_string(),
            params: serde_json::json!({ "scope": scope }),
            severity,
            message: message.map(ToString::to_string),
        });
    }

    fn setup_cycle_fixture() -> (TempDir, DependencyGraph, Vec<SpecFile>) {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/alpha")).expect("mkdir alpha");
        fs::create_dir_all(temp.path().join("src/beta")).expect("mkdir beta");
        fs::create_dir_all(temp.path().join("src/gamma")).expect("mkdir gamma");

        // Internal cycle in alpha.
        fs::write(
            temp.path().join("src/alpha/a.ts"),
            "import { b } from './b'; export const a = b;\n",
        )
        .expect("write alpha a");
        fs::write(
            temp.path().join("src/alpha/b.ts"),
            "import { a } from './a'; export const b = a;\n",
        )
        .expect("write alpha b");

        // External cycle between beta and gamma.
        fs::write(
            temp.path().join("src/beta/x.ts"),
            "import { y } from '../gamma/y'; export const x = y;\n",
        )
        .expect("write beta x");
        fs::write(
            temp.path().join("src/gamma/y.ts"),
            "import { x } from '../beta/x'; export const y = x;\n",
        )
        .expect("write gamma y");

        let specs = vec![
            module_spec("gamma", "src/gamma/**/*"),
            module_spec("alpha", "src/alpha/**/*"),
            module_spec("beta", "src/beta/**/*"),
        ];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let config = SpecConfig::default();
        let graph =
            DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph");

        (temp, graph, specs)
    }

    #[test]
    fn internal_scope_reports_only_internal_components() {
        let (temp, graph, mut specs) = setup_cycle_fixture();
        add_constraint(&mut specs[0], "internal", Severity::Error, None);

        let violations = evaluate_no_circular_deps(&specs, &graph);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].scope, CircularScopeParam::Internal);
        assert_eq!(violations[0].severity, Severity::Error);
        assert_eq!(violations[0].component_modules, vec!["alpha".to_string()]);

        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let files = violations[0]
            .component_files
            .iter()
            .map(|file| normalize_path(file.strip_prefix(&root).expect("under root")))
            .collect::<Vec<_>>();

        assert_eq!(files, vec!["src/alpha/a.ts", "src/alpha/b.ts"]);

        let root_display = root.to_string_lossy();
        assert!(violations[0].message.contains("src/alpha/a.ts"));
        assert!(violations[0].message.contains("src/alpha/b.ts"));
        assert!(
            !violations[0].message.contains(root_display.as_ref()),
            "default circular message should not include absolute repo paths"
        );
    }

    #[test]
    fn external_scope_reports_only_external_components() {
        let (_temp, graph, mut specs) = setup_cycle_fixture();
        add_constraint(
            &mut specs[1],
            "external",
            Severity::Warning,
            Some("custom message"),
        );

        let violations = evaluate_no_circular_deps(&specs, &graph);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].scope, CircularScopeParam::External);
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].message, "custom message");
        assert_eq!(
            violations[0].component_modules,
            vec!["beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn both_scope_is_deterministic() {
        let (temp, graph, mut specs) = setup_cycle_fixture();

        // Add constraints in non-lexical module order; output should still be stable by module.
        add_constraint(&mut specs[0], "both", Severity::Error, None); // gamma
        add_constraint(&mut specs[2], "both", Severity::Error, None); // beta

        let violations = evaluate_no_circular_deps(&specs, &graph);
        assert_eq!(violations.len(), 4);

        let modules = violations
            .iter()
            .map(|violation| violation.module.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            modules,
            vec![
                "beta".to_string(),
                "beta".to_string(),
                "gamma".to_string(),
                "gamma".to_string(),
            ]
        );

        let root = fs::canonicalize(temp.path()).expect("canonical root");
        let components = violations
            .iter()
            .map(|violation| {
                violation
                    .component_files
                    .iter()
                    .map(|file| normalize_path(file.strip_prefix(&root).expect("under root")))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            components,
            vec![
                vec!["src/alpha/a.ts", "src/alpha/b.ts"],
                vec!["src/beta/x.ts", "src/gamma/y.ts"],
                vec!["src/alpha/a.ts", "src/alpha/b.ts"],
                vec!["src/beta/x.ts", "src/gamma/y.ts"],
            ]
        );
    }

    #[test]
    fn repeated_equivalent_constraints_are_deduped() {
        let (_temp, graph, mut specs) = setup_cycle_fixture();

        add_constraint(&mut specs[0], "both", Severity::Error, None);
        add_constraint(&mut specs[0], "both", Severity::Error, None);

        let violations = evaluate_no_circular_deps(&specs, &graph);
        assert_eq!(violations.len(), 2);
        assert!(
            violations
                .iter()
                .all(|violation| violation.module == "gamma")
        );
    }

    #[test]
    fn invalid_scope_does_not_widen_to_both() {
        let (_temp, graph, mut specs) = setup_cycle_fixture();

        add_constraint(&mut specs[0], "unexpected", Severity::Error, None);

        let violations = evaluate_no_circular_deps(&specs, &graph);
        assert!(violations.is_empty());
    }
}
