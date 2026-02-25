use std::path::PathBuf;

use crate::graph::{CycleComponent, CycleScope, DependencyGraph};
use crate::spec::{Severity, SpecFile};

pub const NO_CIRCULAR_DEPS_RULE_ID: &str = "no-circular-deps";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircularScopeParam {
    Internal,
    External,
    Both,
}

impl CircularScopeParam {
    fn from_params(params: &serde_json::Value) -> Self {
        let Some(scope) = params.get("scope").and_then(serde_json::Value::as_str) else {
            return Self::Both;
        };

        match scope.trim().to_ascii_lowercase().as_str() {
            "internal" => Self::Internal,
            "external" => Self::External,
            "both" => Self::Both,
            _ => Self::Both,
        }
    }

    fn as_cycle_scope(self) -> CycleScope {
        match self {
            Self::Internal => CycleScope::Internal,
            Self::External => CycleScope::External,
            Self::Both => CycleScope::Both,
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

/// Evaluate all `no-circular-deps` constraints across loaded specs.
///
/// Each configured constraint emits one violation per matching SCC cycle component.
/// Severity + optional custom message are sourced directly from the constraint.
pub fn evaluate_no_circular_deps(
    specs: &[SpecFile],
    graph: &DependencyGraph,
) -> Vec<CircularDependencyViolation> {
    let mut violations = Vec::new();

    for spec in specs {
        for constraint in &spec.constraints {
            if constraint.rule != NO_CIRCULAR_DEPS_RULE_ID {
                continue;
            }

            let scope = CircularScopeParam::from_params(&constraint.params);
            let cycles = graph.find_cycles(scope.as_cycle_scope());

            for component in cycles {
                violations.push(CircularDependencyViolation {
                    module: spec.module.clone(),
                    rule: NO_CIRCULAR_DEPS_RULE_ID.to_string(),
                    severity: constraint.severity,
                    message: constraint
                        .message
                        .clone()
                        .unwrap_or_else(|| default_message(scope, &component)),
                    scope,
                    component_files: component.files,
                    component_modules: component.modules,
                });
            }
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

    violations
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn default_message(scope: CircularScopeParam, component: &CycleComponent) -> String {
    let files = component
        .files
        .iter()
        .map(|file| file.to_string_lossy().to_string())
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
}
