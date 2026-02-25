use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::deterministic::normalize_repo_relative;
use crate::graph::EdgeKind;
use crate::rules::{Rule, RuleContext, RuleViolation, sort_violations_stable};
use crate::spec::{Boundaries, Visibility};

/// Boundary rule engine for importer/provider constraints.
#[derive(Debug, Default, Clone, Copy)]
pub struct BoundaryRule;

impl Rule for BoundaryRule {
    fn evaluate(&self, ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
        evaluate_boundary_rules(ctx)
    }
}

pub fn evaluate_boundary_rules(ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
    let spec_by_module = ctx
        .specs
        .iter()
        .map(|spec| (spec.module.as_str(), spec))
        .collect::<BTreeMap<_, _>>();

    let test_matcher = compile_optional_globset(&ctx.config.test_patterns);

    let mut violations = Vec::new();

    for node in ctx.graph.files() {
        let Some(importer_module) = node.module_id.as_deref() else {
            continue;
        };

        let Some(importer_spec) = spec_by_module.get(importer_module) else {
            continue;
        };

        let importer_boundaries = importer_spec
            .boundaries
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let importer_is_test = is_test_file(ctx.project_root, &node.path, test_matcher.as_ref());
        let ignored_import_specifiers = ignored_import_specifiers(node.analysis.imports.as_slice());

        for edge in ctx.graph.dependencies_from(&node.path) {
            let Some(provider_module) = ctx.graph.module_of_file(&edge.to) else {
                continue;
            };

            if provider_module == importer_module {
                continue;
            }

            let Some(provider_spec) = spec_by_module.get(provider_module) else {
                continue;
            };

            let provider_boundaries = provider_spec
                .boundaries
                .as_ref()
                .cloned()
                .unwrap_or_default();

            if import_is_ignored(&edge.specifier, edge.kind, &ignored_import_specifiers) {
                continue;
            }

            let position = find_import_position(node.analysis.imports.as_slice(), &edge.specifier);

            if !importer_is_test || importer_boundaries.enforce_in_tests {
                check_importer_side(
                    edge.kind,
                    importer_module,
                    &importer_boundaries,
                    provider_module,
                    &edge.to,
                    ctx.project_root,
                    &provider_boundaries,
                    position,
                    &node.path,
                    &mut violations,
                );
            }

            if !importer_is_test || provider_boundaries.enforce_in_tests {
                check_provider_side(
                    &edge.specifier,
                    importer_module,
                    provider_module,
                    &provider_boundaries,
                    edge.kind,
                    &node.path,
                    &edge.to,
                    position,
                    &mut violations,
                );
            }
        }
    }

    sort_violations_stable(&mut violations);
    violations
}

fn check_importer_side(
    edge_kind: EdgeKind,
    importer_module: &str,
    importer_boundaries: &Boundaries,
    provider_module: &str,
    provider_file: &Path,
    project_root: &Path,
    provider_boundaries: &Boundaries,
    position: Option<(u32, u32)>,
    from_file: &Path,
    violations: &mut Vec<RuleViolation>,
) {
    let never_set = as_set(&importer_boundaries.never_imports);
    if never_set.contains(provider_module) {
        violations.push(build_violation(
            "boundary.never_imports",
            format!("module '{importer_module}' may never import from '{provider_module}'"),
            from_file,
            Some(provider_file),
            Some(importer_module),
            Some(provider_module),
            position,
        ));
        return;
    }

    let allow_set = as_set(&importer_boundaries.allow_imports_from);
    if !allow_set.is_empty() && !allow_set.contains(provider_module) {
        let type_allow_set = as_set(&importer_boundaries.allow_type_imports_from);
        let type_only_carve_out =
            edge_kind == EdgeKind::TypeOnlyImport && type_allow_set.contains(provider_module);

        if !type_only_carve_out {
            violations.push(build_violation(
                "boundary.allow_imports_from",
                format!(
                    "module '{importer_module}' is not allowed to import from '{provider_module}'"
                ),
                from_file,
                Some(provider_file),
                Some(importer_module),
                Some(provider_module),
                position,
            ));
            return;
        }
    }

    if !provider_boundaries.public_api.is_empty() {
        let canonical_root =
            fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
        let canonical_target =
            fs::canonicalize(provider_file).unwrap_or_else(|_| provider_file.to_path_buf());
        let target_rel = normalize_repo_relative(&canonical_root, &canonical_target);
        let public_api_matcher = compile_optional_globset(&provider_boundaries.public_api);

        let is_public_api = public_api_matcher
            .as_ref()
            .is_some_and(|matcher| matcher.is_match(&target_rel));

        if !is_public_api {
            violations.push(build_violation(
                "boundary.public_api",
                format!(
                    "module '{importer_module}' imported non-public file '{}' from '{provider_module}'",
                    target_rel
                ),
                from_file,
                Some(provider_file),
                Some(importer_module),
                Some(provider_module),
                position,
            ));
        }
    }
}

fn check_provider_side(
    specifier: &str,
    importer_module: &str,
    provider_module: &str,
    provider_boundaries: &Boundaries,
    edge_kind: EdgeKind,
    from_file: &Path,
    to_file: &Path,
    position: Option<(u32, u32)>,
    violations: &mut Vec<RuleViolation>,
) {
    let deny_set = as_set(&provider_boundaries.deny_imported_by);
    if deny_set.contains(importer_module) {
        violations.push(build_violation(
            "boundary.deny_imported_by",
            format!("module '{provider_module}' denies imports from '{importer_module}'"),
            from_file,
            Some(to_file),
            Some(importer_module),
            Some(provider_module),
            position,
        ));
        return;
    }

    let allow_set = as_set(&provider_boundaries.allow_imported_by);
    if !allow_set.is_empty() && !allow_set.contains(importer_module) {
        violations.push(build_violation(
            "boundary.allow_imported_by",
            format!(
                "module '{provider_module}' only allows imports from {:?}",
                provider_boundaries.allow_imported_by
            ),
            from_file,
            Some(to_file),
            Some(importer_module),
            Some(provider_module),
            position,
        ));
        return;
    }

    match provider_boundaries.visibility_or_default() {
        Visibility::Public => {}
        Visibility::Internal => {
            let friends = as_set(&provider_boundaries.friend_modules);
            if !friends.contains(importer_module) {
                violations.push(build_violation(
                    "boundary.visibility.internal",
                    format!(
                        "module '{provider_module}' is internal and not visible to '{importer_module}'"
                    ),
                    from_file,
                    Some(to_file),
                    Some(importer_module),
                    Some(provider_module),
                    position,
                ));
                return;
            }
        }
        Visibility::Private => {
            violations.push(build_violation(
                "boundary.visibility.private",
                format!(
                    "module '{provider_module}' is private and cannot be imported by '{importer_module}'"
                ),
                from_file,
                Some(to_file),
                Some(importer_module),
                Some(provider_module),
                position,
            ));
            return;
        }
    }

    if provider_boundaries.enforce_canonical_imports
        && is_cross_module_relative(specifier, edge_kind)
    {
        violations.push(build_violation(
            "boundary.canonical_imports",
            format!(
                "module '{provider_module}' requires canonical imports; cross-module relative specifier '{specifier}' is not allowed"
            ),
            from_file,
            Some(to_file),
            Some(importer_module),
            Some(provider_module),
            position,
        ));
    }
}

fn is_cross_module_relative(specifier: &str, edge_kind: EdgeKind) -> bool {
    if matches!(edge_kind, EdgeKind::DynamicImport) {
        // dynamic imports can still be relative literals and should respect the policy.
    }

    specifier.starts_with("./") || specifier.starts_with("../")
}

fn import_is_ignored(
    specifier: &str,
    edge_kind: EdgeKind,
    ignored_specifiers: &BTreeSet<String>,
) -> bool {
    matches!(
        edge_kind,
        EdgeKind::RuntimeImport | EdgeKind::TypeOnlyImport
    ) && ignored_specifiers.contains(specifier)
}

fn ignored_import_specifiers(imports: &[crate::parser::ImportInfo]) -> BTreeSet<String> {
    imports
        .iter()
        .filter(|import| import.ignore_comment.is_some())
        .map(|import| import.specifier.clone())
        .collect()
}

fn find_import_position(
    imports: &[crate::parser::ImportInfo],
    specifier: &str,
) -> Option<(u32, u32)> {
    imports
        .iter()
        .find(|import| import.specifier == specifier)
        .map(|import| (import.line, import.column))
}

fn as_set(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

fn compile_optional_globset(patterns: &[String]) -> Option<GlobSet> {
    if patterns.is_empty() {
        return None;
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let Ok(glob) = Glob::new(pattern) else {
            continue;
        };
        builder.add(glob);
    }

    builder.build().ok()
}

fn is_test_file(project_root: &Path, file: &Path, test_matcher: Option<&GlobSet>) -> bool {
    let Some(test_matcher) = test_matcher else {
        return false;
    };

    let canonical_root =
        fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_file = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let relative = normalize_repo_relative(&canonical_root, &canonical_file);
    test_matcher.is_match(&relative)
}

fn build_violation(
    rule: &str,
    message: String,
    from_file: &Path,
    to_file: Option<&Path>,
    from_module: Option<&str>,
    to_module: Option<&str>,
    position: Option<(u32, u32)>,
) -> RuleViolation {
    let (line, column) = position.unwrap_or((0, 0));

    RuleViolation {
        rule: rule.to_string(),
        message,
        from_file: from_file.to_path_buf(),
        to_file: to_file.map(Path::to_path_buf),
        from_module: from_module.map(str::to_string),
        to_module: to_module.map(str::to_string),
        line: Some(line),
        column: Some(column),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::graph::DependencyGraph;
    use crate::resolver::ModuleResolver;
    use crate::spec::{Boundaries, SpecConfig, SpecFile, Visibility};

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

    fn write_file(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, contents).expect("write file");
    }

    fn run_engine(
        temp: &TempDir,
        mut config: SpecConfig,
        specs: Vec<SpecFile>,
    ) -> Vec<RuleViolation> {
        config.spec_dirs = vec![".".to_string()];

        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("graph");

        let ctx = RuleContext {
            project_root: temp.path(),
            config: &config,
            specs: &specs,
            graph: &graph,
        };

        evaluate_boundary_rules(&ctx)
    }

    #[test]
    fn enforces_importer_allow_and_never_with_type_only_carve_out() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';\nimport type { T } from '../types/index';\nimport { n } from '../never/index';\nexport const x = b + n;\n",
        );
        write_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");
        write_file(
            temp.path(),
            "src/types/index.ts",
            "export type T = string;\n",
        );
        write_file(temp.path(), "src/never/index.ts", "export const n = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: vec!["b".to_string()],
                    allow_type_imports_from: vec!["types".to_string()],
                    never_imports: vec!["never".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
            spec_with_boundaries("types", "src/types/**/*", Boundaries::default()),
            spec_with_boundaries("never", "src/never/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.never_imports");
    }

    #[test]
    fn enforces_public_api_on_provider_files() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { ok } from '../provider/public/index';\nimport { nope } from '../provider/internal/secret';\nexport const x = ok + nope;\n",
        );
        write_file(
            temp.path(),
            "src/provider/public/index.ts",
            "export const ok = 1;\n",
        );
        write_file(
            temp.path(),
            "src/provider/internal/secret.ts",
            "export const nope = 2;\n",
        );

        let specs = vec![
            spec_with_boundaries("app", "src/app/**/*", Boundaries::default()),
            spec_with_boundaries(
                "provider",
                "src/provider/**/*",
                Boundaries {
                    public_api: vec!["src/provider/public/**/*".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.public_api");
        assert!(violations[0].message.contains("non-public"));
    }

    #[test]
    fn enforces_provider_visibility_allow_and_deny_precedence() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/a/main.ts",
            "import { p } from '../provider/index';\nexport const x = p;\n",
        );
        write_file(
            temp.path(),
            "src/provider/index.ts",
            "export const p = 1;\n",
        );

        let specs = vec![
            spec_with_boundaries("a", "src/a/**/*", Boundaries::default()),
            spec_with_boundaries(
                "provider",
                "src/provider/**/*",
                Boundaries {
                    visibility: Some(Visibility::Internal),
                    allow_imported_by: vec!["a".to_string()],
                    deny_imported_by: vec!["a".to_string()],
                    friend_modules: vec!["a".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.deny_imported_by");
    }

    #[test]
    fn visibility_internal_respects_friend_modules_and_private_blocks_all() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/friend/main.ts",
            "import { i } from '../internal/index';\nimport { p } from '../private/index';\nexport const x = i + p;\n",
        );
        write_file(
            temp.path(),
            "src/stranger/main.ts",
            "import { i } from '../internal/index';\nexport const y = i;\n",
        );
        write_file(
            temp.path(),
            "src/internal/index.ts",
            "export const i = 1;\n",
        );
        write_file(temp.path(), "src/private/index.ts", "export const p = 2;\n");

        let specs = vec![
            spec_with_boundaries("friend", "src/friend/**/*", Boundaries::default()),
            spec_with_boundaries("stranger", "src/stranger/**/*", Boundaries::default()),
            spec_with_boundaries(
                "internal",
                "src/internal/**/*",
                Boundaries {
                    visibility: Some(Visibility::Internal),
                    friend_modules: vec!["friend".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "private",
                "src/private/**/*",
                Boundaries {
                    visibility: Some(Visibility::Private),
                    friend_modules: vec!["friend".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        let rules = violations
            .iter()
            .map(|v| v.rule.as_str())
            .collect::<Vec<_>>();

        assert!(rules.contains(&"boundary.visibility.internal"));
        assert!(rules.contains(&"boundary.visibility.private"));
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn canonical_imports_bans_cross_module_relative_when_enabled() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';\nimport { c } from '@acme/c/index';\nexport const x = b + c;\n",
        );
        write_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");
        write_file(temp.path(), "src/c/index.ts", "export const c = 2;\n");

        let specs = vec![
            spec_with_boundaries("a", "src/a/**/*", Boundaries::default()),
            spec_with_boundaries(
                "b",
                "src/b/**/*",
                Boundaries {
                    enforce_canonical_imports: true,
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("c", "src/c/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.canonical_imports");
    }

    #[test]
    fn test_files_are_exempt_unless_enforce_in_tests_is_true() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/a/example.test.ts",
            "import { b } from '../b/index';\nexport const x = b;\n",
        );
        write_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: vec![],
                    never_imports: vec!["b".to_string()],
                    enforce_in_tests: false,
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "b",
                "src/b/**/*",
                Boundaries {
                    enforce_in_tests: false,
                    ..Boundaries::default()
                },
            ),
        ];

        let no_enforce = run_engine(&temp, SpecConfig::default(), specs.clone());
        assert!(no_enforce.is_empty());

        let mut specs_enforced = specs;
        specs_enforced[0]
            .boundaries
            .as_mut()
            .expect("boundaries")
            .enforce_in_tests = true;

        let enforced = run_engine(&temp, SpecConfig::default(), specs_enforced);
        assert_eq!(enforced.len(), 1);
        assert_eq!(enforced[0].rule, "boundary.never_imports");
    }

    #[test]
    fn ignores_imports_marked_with_specgate_ignore() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/a/main.ts",
            "// @specgate-ignore: temporary\nimport { b } from '../b/index';\nexport const x = b;\n",
        );
        write_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    never_imports: vec!["b".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn violations_are_deterministically_sorted() {
        let temp = TempDir::new().expect("tempdir");

        write_file(
            temp.path(),
            "src/z/main.ts",
            "import { a } from '../a/index';\nimport { b } from '../b/index';\nexport const z = a + b;\n",
        );
        write_file(
            temp.path(),
            "src/y/main.ts",
            "import { a } from '../a/index';\nexport const y = a;\n",
        );
        write_file(temp.path(), "src/a/index.ts", "export const a = 1;\n");
        write_file(temp.path(), "src/b/index.ts", "export const b = 2;\n");

        let specs = vec![
            spec_with_boundaries(
                "z",
                "src/z/**/*",
                Boundaries {
                    allow_imports_from: vec!["z".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "y",
                "src/y/**/*",
                Boundaries {
                    allow_imports_from: vec!["y".to_string()],
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("a", "src/a/**/*", Boundaries::default()),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        let keys = violations
            .iter()
            .map(|v| {
                (
                    normalize_repo_relative(temp.path(), &v.from_file),
                    v.line.unwrap_or(0),
                    v.column.unwrap_or(0),
                    v.rule.clone(),
                )
            })
            .collect::<Vec<_>>();

        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }
}
