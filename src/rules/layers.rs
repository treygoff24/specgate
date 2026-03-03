use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;

use crate::graph::DependencyGraph;
use crate::spec::SpecFile;

pub const ENFORCE_LAYER_RULE_ID: &str = "enforce-layer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforceLayerConfig {
    pub layers: Vec<String>,
}

impl EnforceLayerConfig {
    fn index_by_layer(&self) -> BTreeMap<&str, usize> {
        self.layers
            .iter()
            .enumerate()
            .map(|(index, layer)| (layer.as_str(), index))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerViolation {
    pub from_module: String,
    pub to_module: String,
    pub from_layer: String,
    pub to_layer: String,
    pub from_file: PathBuf,
    pub to_file: PathBuf,
    pub specifier: String,
    pub message: String,
    pub fix_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerConfigIssue {
    pub module: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforceLayerReport {
    pub violations: Vec<LayerViolation>,
    pub config_issues: Vec<LayerConfigIssue>,
}

impl EnforceLayerReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty() && self.config_issues.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerConfigParseError {
    pub message: String,
}

impl std::fmt::Display for LayerConfigParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LayerConfigParseError {}

pub fn parse_enforce_layer_config(
    params: &Value,
) -> Result<EnforceLayerConfig, LayerConfigParseError> {
    let Value::Object(map) = params else {
        return Err(LayerConfigParseError {
            message: "params for rule 'enforce-layer' must be an object with key 'layers'"
                .to_string(),
        });
    };

    let Some(layers_value) = map.get("layers") else {
        return Err(LayerConfigParseError {
            message: "params for rule 'enforce-layer' must include non-empty 'layers' array"
                .to_string(),
        });
    };

    let Value::Array(raw_layers) = layers_value else {
        return Err(LayerConfigParseError {
            message: "params.layers for rule 'enforce-layer' must be an array of non-empty strings"
                .to_string(),
        });
    };

    if raw_layers.is_empty() {
        return Err(LayerConfigParseError {
            message: "params.layers for rule 'enforce-layer' must not be empty".to_string(),
        });
    }

    let mut layers = Vec::with_capacity(raw_layers.len());
    let mut seen = BTreeSet::new();

    for layer in raw_layers {
        let Value::String(layer) = layer else {
            return Err(LayerConfigParseError {
                message: "params.layers for rule 'enforce-layer' must contain only strings"
                    .to_string(),
            });
        };

        let layer = layer.trim();
        if layer.is_empty() {
            return Err(LayerConfigParseError {
                message: "params.layers for rule 'enforce-layer' cannot contain empty layer names"
                    .to_string(),
            });
        }

        if !seen.insert(layer.to_string()) {
            return Err(LayerConfigParseError {
                message: format!(
                    "params.layers for rule 'enforce-layer' contains duplicate layer '{layer}'"
                ),
            });
        }

        layers.push(layer.to_string());
    }

    Ok(EnforceLayerConfig { layers })
}

/// Resolve a module id to its architectural layer.
///
/// Convention: layer is the first non-empty `/`-delimited segment of the module id.
/// Examples:
/// - `ui/checkout` -> `ui`
/// - `core` -> `core`
pub fn layer_for_module(module_id: &str) -> Option<&str> {
    module_id
        .split('/')
        .find(|segment| !segment.trim().is_empty())
}

pub fn evaluate_enforce_layer(specs: &[SpecFile], graph: &DependencyGraph) -> EnforceLayerReport {
    let mut config_issues = Vec::new();

    let mut configured_constraints = specs
        .iter()
        .flat_map(|spec| {
            spec.constraints
                .iter()
                .filter(|constraint| constraint.rule == ENFORCE_LAYER_RULE_ID)
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
    configured_constraints.dedup();

    let mut usable_configs = Vec::new();

    for (module, params) in configured_constraints {
        match parse_enforce_layer_config(&params) {
            Ok(config) => usable_configs.push((module, config)),
            Err(error) => config_issues.push(LayerConfigIssue {
                module,
                message: error.message,
            }),
        }
    }

    usable_configs.sort_by(|(left_module, left_config), (right_module, right_config)| {
        left_module
            .cmp(right_module)
            .then_with(|| left_config.layers.cmp(&right_config.layers))
    });

    let Some((canonical_module, canonical_config)) = usable_configs.first().cloned() else {
        config_issues.sort_by(|a, b| {
            a.module
                .cmp(&b.module)
                .then_with(|| a.message.cmp(&b.message))
        });
        config_issues.dedup();
        return EnforceLayerReport {
            violations: Vec::new(),
            config_issues,
        };
    };

    // Deterministic + explicit conflict policy:
    // choose the lexicographically first module declaration as canonical, and report all mismatches.
    for (module, config) in usable_configs.iter().skip(1) {
        if config != &canonical_config {
            config_issues.push(LayerConfigIssue {
                module: module.clone(),
                message: format!(
                    "conflicting enforce-layer config; using canonical layers {:?} from module '{}' (deterministic: lexicographically first module id). This module declared layers {:?}",
                    canonical_config.layers, canonical_module, config.layers
                ),
            });
        }
    }

    let layer_index = canonical_config.index_by_layer();

    // Validate module -> layer mappings up-front (not only when an edge references the module).
    let mut all_modules = graph.modules().into_iter().collect::<BTreeSet<_>>();
    all_modules.extend(specs.iter().map(|spec| spec.module.clone()));

    let mut unknown_layer_issues = BTreeSet::new();
    for module in all_modules {
        let Some(layer) = layer_for_module(&module).map(str::to_string) else {
            continue;
        };

        if !layer_index.contains_key(layer.as_str()) {
            unknown_layer_issues.insert((module, layer));
        }
    }

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

        let Some(from_layer) = layer_for_module(&from_module).map(str::to_string) else {
            continue;
        };
        let Some(to_layer) = layer_for_module(&to_module).map(str::to_string) else {
            continue;
        };

        if from_layer == to_layer {
            continue;
        }

        let Some(from_index) = layer_index.get(from_layer.as_str()).copied() else {
            continue;
        };
        let Some(to_index) = layer_index.get(to_layer.as_str()).copied() else {
            continue;
        };

        if to_index >= from_index {
            continue;
        }

        let message = format!(
            "forbidden layer edge: module '{}' (layer '{}') imports '{}' (layer '{}') via '{}'",
            from_module, from_layer, to_module, to_layer, edge.specifier
        );

        let fix_hint = format!(
            "Re-route this dependency to follow layer order {:?}: modules may import same layer or a later layer. Consider moving shared logic into a lower layer or introducing an interface boundary.",
            canonical_config.layers
        );

        violations.push(LayerViolation {
            from_module,
            to_module,
            from_layer,
            to_layer,
            from_file: edge.from,
            to_file: edge.to,
            specifier: edge.specifier,
            message,
            fix_hint,
        });
    }

    for (module, layer) in unknown_layer_issues {
        config_issues.push(LayerConfigIssue {
            module: module.clone(),
            message: format!(
                "module '{}' maps to layer '{}', which is not listed in params.layers {:?}",
                module, layer, canonical_config.layers
            ),
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

    EnforceLayerReport {
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

    fn spec(module: &str, path: &str, with_rule: bool, layers: Value) -> SpecFile {
        let constraints = if with_rule {
            vec![Constraint {
                rule: ENFORCE_LAYER_RULE_ID.to_string(),
                params: layers,
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
            json!({"layers": "ui"}),
            json!({"layers": []}),
            json!({"layers": ["ui", "ui"]}),
            json!({"layers": ["ui", 1]}),
            json!({"layers": [" "]}),
        ];

        for params in invalid {
            assert!(parse_enforce_layer_config(&params).is_err());
        }
    }

    #[test]
    fn allows_same_layer_and_forward_edges() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/ui")).expect("mkdir ui");
        fs::create_dir_all(temp.path().join("src/domain")).expect("mkdir domain");

        fs::write(
            temp.path().join("src/ui/a.ts"),
            "import { b } from './b'; import { d } from '../domain/d'; export const a = b + d;\n",
        )
        .expect("write ui a");
        fs::write(temp.path().join("src/ui/b.ts"), "export const b = 1;\n").expect("write ui b");
        fs::write(temp.path().join("src/domain/d.ts"), "export const d = 2;\n")
            .expect("write domain d");

        let layers = json!({"layers": ["ui", "domain"]});
        let specs = vec![
            spec("ui/checkout", "src/ui/**/*", true, layers.clone()),
            spec("domain/orders", "src/domain/**/*", false, layers),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_layer(&specs, &graph);

        assert!(report.config_issues.is_empty());
        assert!(report.violations.is_empty(), "{report:?}");
    }

    #[test]
    fn flags_reverse_layer_edges_with_deterministic_message_and_hint() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/ui")).expect("mkdir ui");
        fs::create_dir_all(temp.path().join("src/domain")).expect("mkdir domain");

        fs::write(
            temp.path().join("src/domain/order.ts"),
            "import { button } from '../ui/button'; export const order = button;\n",
        )
        .expect("write domain");
        fs::write(
            temp.path().join("src/ui/button.ts"),
            "export const button = 1;\n",
        )
        .expect("write ui");

        let layers = json!({"layers": ["ui", "domain"]});
        let specs = vec![
            spec("ui/checkout", "src/ui/**/*", true, layers.clone()),
            spec("domain/orders", "src/domain/**/*", false, layers),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_layer(&specs, &graph);

        assert!(report.config_issues.is_empty());
        assert_eq!(report.violations.len(), 1);

        let violation = &report.violations[0];
        assert_eq!(violation.from_module, "domain/orders");
        assert_eq!(violation.to_module, "ui/checkout");
        assert_eq!(violation.from_layer, "domain");
        assert_eq!(violation.to_layer, "ui");
        assert!(violation.message.contains("forbidden layer edge"));
        assert!(violation.fix_hint.contains("same layer or a later layer"));
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
            json!({"layers": "core"}),
        )];
        let graph = build_graph(&temp, &specs);

        let report = evaluate_enforce_layer(&specs, &graph);
        assert!(report.violations.is_empty());
        assert_eq!(report.config_issues.len(), 1);
        assert!(report.config_issues[0].message.contains("params.layers"));
    }

    #[test]
    fn unknown_layer_mappings_are_reported_once_per_module() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/feature")).expect("mkdir feature");
        fs::create_dir_all(temp.path().join("src/domain")).expect("mkdir domain");

        fs::write(
            temp.path().join("src/feature/f.ts"),
            "import { d } from '../domain/d'; export const f = d;\n",
        )
        .expect("write f");
        fs::write(temp.path().join("src/domain/d.ts"), "export const d = 1;\n").expect("write d");

        let layers = json!({"layers": ["ui", "domain"]});
        let specs = vec![
            spec("feature/search", "src/feature/**/*", true, layers.clone()),
            spec("domain/orders", "src/domain/**/*", false, layers),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_layer(&specs, &graph);

        assert!(report.violations.is_empty());
        assert_eq!(report.config_issues.len(), 1);
        assert!(report.config_issues[0]
            .message
            .contains("which is not listed in params.layers"));
    }

    #[test]
    fn unknown_layer_is_reported_even_without_cross_module_edges() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/ui")).expect("mkdir ui");
        fs::create_dir_all(temp.path().join("src/feature")).expect("mkdir feature");

        fs::write(temp.path().join("src/ui/a.ts"), "export const a = 1;\n").expect("write ui");
        fs::write(
            temp.path().join("src/feature/f.ts"),
            "export const f = 2;\n",
        )
        .expect("write feature");

        let layers = json!({"layers": ["ui", "domain"]});
        let specs = vec![
            spec("ui/checkout", "src/ui/**/*", true, layers.clone()),
            spec("feature/search", "src/feature/**/*", false, layers),
        ];

        let graph = build_graph(&temp, &specs);
        let report = evaluate_enforce_layer(&specs, &graph);

        assert!(report.violations.is_empty());
        assert_eq!(report.config_issues.len(), 1);
        assert_eq!(report.config_issues[0].module, "feature/search");
        assert!(report.config_issues[0]
            .message
            .contains("which is not listed in params.layers"));
    }

    #[test]
    fn conflicting_configs_are_deterministic_and_explicit() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/core")).expect("mkdir core");
        fs::create_dir_all(temp.path().join("src/ui")).expect("mkdir ui");

        fs::write(
            temp.path().join("src/core/order.ts"),
            "export const order = 1;\n",
        )
        .expect("write core");
        fs::write(
            temp.path().join("src/ui/button.ts"),
            "export const button = 1;\n",
        )
        .expect("write ui");

        let canonical_layers = json!({"layers": ["core", "ui"]});
        let conflicting_layers = json!({"layers": ["ui", "domain"]});

        let specs_left = vec![
            spec(
                "ui/checkout",
                "src/ui/**/*",
                true,
                conflicting_layers.clone(),
            ),
            spec(
                "core/orders",
                "src/core/**/*",
                true,
                canonical_layers.clone(),
            ),
        ];
        let specs_right = vec![
            spec("core/orders", "src/core/**/*", true, canonical_layers),
            spec("ui/checkout", "src/ui/**/*", true, conflicting_layers),
        ];

        let graph_left = build_graph(&temp, &specs_left);
        let graph_right = build_graph(&temp, &specs_right);

        let report_left = evaluate_enforce_layer(&specs_left, &graph_left);
        let report_right = evaluate_enforce_layer(&specs_right, &graph_right);

        assert_eq!(report_left, report_right);
        assert_eq!(report_left.config_issues.len(), 1);

        let issue = &report_left.config_issues[0];
        assert_eq!(issue.module, "ui/checkout");
        assert!(issue
            .message
            .contains("using canonical layers [\"core\", \"ui\"] from module 'core/orders'"));
        assert!(issue
            .message
            .contains("deterministic: lexicographically first module id"));
    }

    #[test]
    fn layer_for_module_uses_first_path_segment() {
        assert_eq!(layer_for_module("ui/checkout"), Some("ui"));
        assert_eq!(layer_for_module("core"), Some("core"));
        assert_eq!(layer_for_module("/leading/slash"), Some("leading"));
        assert_eq!(layer_for_module("///"), None);
    }
}
