use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::deterministic::normalize_repo_relative;
use crate::graph::{DependencyEdge, EdgeKind};
use crate::rules::{
    compile_optional_globset_strict, matches_test_file, sort_violations_stable, GlobCompileError,
    Rule, RuleContext, RuleViolation,
};
use crate::spec::{Boundaries, Visibility};

/// Canonical rule id for enforcing cross-module canonical import usage.
pub const BOUNDARY_CANONICAL_IMPORT_RULE_ID: &str = "boundary.canonical_import";
/// Backward-compatible alias for older emitted id.
pub const BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS: &str = "boundary.canonical_imports";
/// Rule id for contract version mismatch (contracts in 2.2 spec).
pub const BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID: &str = "boundary.contract_version_mismatch";

/// Boundary rule engine for importer/provider constraints.
#[derive(Debug, Default, Clone, Copy)]
pub struct BoundaryRule;

impl Rule for BoundaryRule {
    fn evaluate(&self, ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
        evaluate_boundary_rules(ctx)
    }
}

#[derive(Debug)]
enum PublicApiMatcher {
    Disabled,
    Compiled(GlobSet),
    Invalid,
}

#[derive(Debug)]
struct BoundaryMatcherCache {
    canonical_root: PathBuf,
    test_matcher: Option<GlobSet>,
    public_api_by_module: BTreeMap<String, PublicApiMatcher>,
}

pub fn evaluate_boundary_rules(ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
    let spec_by_module = ctx
        .specs
        .iter()
        .map(|spec| (spec.module.as_str(), spec))
        .collect::<BTreeMap<_, _>>();

    let mut violations = Vec::new();
    let matcher_cache = build_matcher_cache(ctx, &spec_by_module, &mut violations);

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
        let importer_is_test = matches_test_file(
            ctx.project_root,
            &node.path,
            matcher_cache.test_matcher.as_ref(),
        );

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

            if import_is_ignored(&edge, node.analysis.imports.as_slice()) {
                continue;
            }

            let position = import_position(&edge, node.analysis.imports.as_slice());

            // Precedence model:
            // - Importer-side and provider-side checks each short-circuit internally.
            // - Both sides are intentionally evaluated for the same edge so callers can
            //   receive both policy violations when both modules are misconfigured.
            if !importer_is_test || importer_boundaries.enforce_in_tests {
                check_importer_side(
                    edge.kind,
                    importer_module,
                    &importer_boundaries,
                    provider_module,
                    &edge.to,
                    matcher_cache.public_api_by_module.get(provider_module),
                    &matcher_cache.canonical_root,
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

fn build_matcher_cache(
    ctx: &RuleContext<'_>,
    spec_by_module: &BTreeMap<&str, &crate::spec::SpecFile>,
    violations: &mut Vec<RuleViolation>,
) -> BoundaryMatcherCache {
    let canonical_root =
        fs::canonicalize(ctx.project_root).unwrap_or_else(|_| ctx.project_root.to_path_buf());

    let test_matcher = match compile_optional_globset_strict(&ctx.config.test_patterns) {
        Ok(matcher) => matcher,
        Err(error) => {
            violations.push(build_violation(
                "boundary.config.invalid_test_glob",
                format!(
                    "invalid config.test_patterns glob pattern: {}",
                    render_glob_error(&error)
                ),
                ctx.project_root,
                None,
                None,
                None,
                None,
            ));
            None
        }
    };

    let mut public_api_by_module = BTreeMap::new();

    for (module, spec) in spec_by_module {
        let boundaries = spec.boundaries.as_ref().cloned().unwrap_or_default();
        if boundaries.public_api.is_empty() {
            public_api_by_module.insert((*module).to_string(), PublicApiMatcher::Disabled);
            continue;
        }

        match compile_optional_globset_strict(&boundaries.public_api) {
            Ok(Some(matcher)) => {
                public_api_by_module
                    .insert((*module).to_string(), PublicApiMatcher::Compiled(matcher));
            }
            Ok(None) => {
                public_api_by_module.insert((*module).to_string(), PublicApiMatcher::Disabled);
            }
            Err(error) => {
                public_api_by_module.insert((*module).to_string(), PublicApiMatcher::Invalid);
                let spec_path = spec.spec_path.as_deref().unwrap_or(ctx.project_root);
                violations.push(build_violation(
                    "boundary.config.invalid_public_api_glob",
                    format!(
                        "module '{}' has invalid boundaries.public_api glob pattern: {}",
                        module,
                        render_glob_error(&error)
                    ),
                    spec_path,
                    None,
                    Some(module),
                    None,
                    None,
                ));
            }
        }
    }

    BoundaryMatcherCache {
        canonical_root,
        test_matcher,
        public_api_by_module,
    }
}

fn render_glob_error(error: &GlobCompileError) -> String {
    match error {
        GlobCompileError::InvalidPattern { pattern, source } => {
            format!("'{pattern}' ({source})")
        }
        GlobCompileError::Build { source } => format!("<globset> ({source})"),
    }
}

#[allow(clippy::too_many_arguments)]
fn check_importer_side(
    edge_kind: EdgeKind,
    importer_module: &str,
    importer_boundaries: &Boundaries,
    provider_module: &str,
    provider_file: &Path,
    public_api_matcher: Option<&PublicApiMatcher>,
    canonical_root: &Path,
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

    if let Some(allow_imports_from) = importer_boundaries.allow_imports_from.as_deref() {
        let allow_set = as_set(allow_imports_from);
        if !allow_set.contains(provider_module) {
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
    }

    if let Some(PublicApiMatcher::Compiled(public_api_matcher)) = public_api_matcher {
        let target_rel = normalize_repo_relative(canonical_root, provider_file);
        let is_public_api = public_api_matcher.is_match(&target_rel);

        if !is_public_api {
            violations.push(build_violation(
                "boundary.public_api",
                format!(
                    "module '{importer_module}' imported non-public file '{target_rel}' from '{provider_module}'",
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

#[allow(clippy::too_many_arguments)]
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
            BOUNDARY_CANONICAL_IMPORT_RULE_ID,
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

fn import_is_ignored(edge: &DependencyEdge, imports: &[crate::parser::ImportInfo]) -> bool {
    if !matches!(
        edge.kind,
        EdgeKind::RuntimeImport | EdgeKind::TypeOnlyImport
    ) {
        return false;
    }

    if edge.ignored_by_comment {
        return true;
    }

    imports.iter().any(|import| {
        import.ignore_comment.is_some()
            && import.specifier == edge.specifier
            && edge.span_start.zip(edge.span_end) == Some((import.span_start, import.span_end))
    })
}

fn import_position(
    edge: &DependencyEdge,
    imports: &[crate::parser::ImportInfo],
) -> Option<(u32, u32)> {
    edge.line.zip(edge.column).or_else(|| {
        imports
            .iter()
            .find(|import| {
                import.specifier == edge.specifier
                    && edge
                        .span_start
                        .zip(edge.span_end)
                        .is_some_and(|span| span == (import.span_start, import.span_end))
            })
            .map(|import| (import.line, import.column))
    })
}

fn as_set(values: &[String]) -> BTreeSet<&str> {
    values.iter().map(String::as_str).collect()
}

pub fn is_canonical_import_rule_id(rule_id: &str) -> bool {
    matches!(
        rule_id,
        BOUNDARY_CANONICAL_IMPORT_RULE_ID | BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS
    )
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
    RuleViolation {
        rule: rule.to_string(),
        message,
        from_file: from_file.to_path_buf(),
        to_file: to_file.map(Path::to_path_buf),
        from_module: from_module.map(str::to_string),
        to_module: to_module.map(str::to_string),
        line: position.map(|(line, _)| line),
        column: position.map(|(_, column)| column),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::graph::DependencyGraph;
    use crate::resolver::ModuleResolver;
    use crate::rules::write_test_file;
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
    fn omitted_allow_imports_from_allows_cross_module_imports() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';
export const x = b;
",
        );
        write_test_file(
            temp.path(),
            "src/b/index.ts",
            "export const b = 1;
",
        );

        let specs = vec![
            spec_with_boundaries("a", "src/a/**/*", Boundaries::default()),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn empty_allow_imports_from_denies_cross_module_imports() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';
export const x = b;
",
        );
        write_test_file(
            temp.path(),
            "src/b/index.ts",
            "export const b = 1;
",
        );

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec![]),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.allow_imports_from");
    }

    #[test]
    fn allow_imports_from_specific_modules_allows_listed_and_blocks_others() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { lib } from '../lib/index';
import { health } from '../routes/health';
export const x = { lib, health };
",
        );
        write_test_file(
            temp.path(),
            "src/lib/index.ts",
            "export const lib = 1;
",
        );
        write_test_file(
            temp.path(),
            "src/routes/health.ts",
            "export const health = 'ok';
",
        );

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["lib".to_string()]),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("lib", "src/lib/**/*", Boundaries::default()),
            spec_with_boundaries("routes", "src/routes/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.allow_imports_from");
        assert_eq!(violations[0].to_module.as_deref(), Some("routes"));
    }

    #[test]
    fn self_imports_are_always_allowed_even_with_empty_allowlist() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { local } from './local';
export const x = local;
",
        );
        write_test_file(
            temp.path(),
            "src/a/local.ts",
            "export const local = 1;
",
        );

        let specs = vec![spec_with_boundaries(
            "a",
            "src/a/**/*",
            Boundaries {
                allow_imports_from: Some(vec![]),
                ..Boundaries::default()
            },
        )];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn enforces_importer_allow_and_never_with_type_only_carve_out() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';\nimport type { T } from '../types/index';\nimport { n } from '../never/index';\nexport const x = b + n;\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");
        write_test_file(
            temp.path(),
            "src/types/index.ts",
            "export type T = string;\n",
        );
        write_test_file(temp.path(), "src/never/index.ts", "export const n = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["b".to_string()]),
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

        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { ok } from '../provider/public/index';\nimport { nope } from '../provider/internal/secret';\nexport const x = ok + nope;\n",
        );
        write_test_file(
            temp.path(),
            "src/provider/public/index.ts",
            "export const ok = 1;\n",
        );
        write_test_file(
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

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { p } from '../provider/index';\nexport const x = p;\n",
        );
        write_test_file(
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

        write_test_file(
            temp.path(),
            "src/friend/main.ts",
            "import { i } from '../internal/index';\nimport { p } from '../private/index';\nexport const x = i + p;\n",
        );
        write_test_file(
            temp.path(),
            "src/stranger/main.ts",
            "import { i } from '../internal/index';\nexport const y = i;\n",
        );
        write_test_file(
            temp.path(),
            "src/internal/index.ts",
            "export const i = 1;\n",
        );
        write_test_file(temp.path(), "src/private/index.ts", "export const p = 2;\n");

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

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { b } from '../b/index';\nimport { c } from '@acme/c/index';\nexport const x = b + c;\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");
        write_test_file(temp.path(), "src/c/index.ts", "export const c = 2;\n");

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
        assert_eq!(violations[0].rule, BOUNDARY_CANONICAL_IMPORT_RULE_ID);
    }

    #[test]
    fn canonical_import_rule_id_alias_is_recognized() {
        assert!(is_canonical_import_rule_id(
            BOUNDARY_CANONICAL_IMPORT_RULE_ID
        ));
        assert!(is_canonical_import_rule_id(
            BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS
        ));
        assert!(!is_canonical_import_rule_id("boundary.never_imports"));
    }

    #[test]
    fn test_files_are_exempt_unless_enforce_in_tests_is_true() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/example.test.ts",
            "import { b } from '../b/index';\nexport const x = b;\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec![]),
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

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "// @specgate-ignore: temporary\nimport { b } from '../b/index';\nexport const x = b;\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

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
    fn ignore_is_scoped_to_single_import_occurrence() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "// @specgate-ignore: temporary\nimport { b as ignored } from '../b/index';\nimport { b as enforced } from '../b/index';\nexport const x = ignored + enforced;\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

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
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.never_imports");
        assert_eq!(violations[0].line, Some(3));
    }

    #[test]
    fn reports_importer_and_provider_violations_for_same_edge() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "import { p } from '../provider/index';\nexport const x = p;\n",
        );
        write_test_file(
            temp.path(),
            "src/provider/index.ts",
            "export const p = 1;\n",
        );

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["a".to_string()]),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "provider",
                "src/provider/**/*",
                Boundaries {
                    deny_imported_by: vec!["a".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        let rules = violations
            .iter()
            .map(|violation| violation.rule.as_str())
            .collect::<Vec<_>>();

        assert_eq!(violations.len(), 2);
        assert!(rules.contains(&"boundary.allow_imports_from"));
        assert!(rules.contains(&"boundary.deny_imported_by"));
    }

    #[test]
    fn invalid_public_api_glob_is_reported_as_config_violation() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { p } from '../provider/index';\nexport const x = p;\n",
        );
        write_test_file(
            temp.path(),
            "src/provider/index.ts",
            "export const p = 1;\n",
        );

        let specs = vec![
            spec_with_boundaries("app", "src/app/**/*", Boundaries::default()),
            spec_with_boundaries(
                "provider",
                "src/provider/**/*",
                Boundaries {
                    public_api: vec!["[".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].rule,
            "boundary.config.invalid_public_api_glob"
        );
        assert!(violations[0].message.contains("boundaries.public_api"));
    }

    #[test]
    fn uses_edge_position_metadata_for_non_import_edges() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/a/main.ts",
            "export * from '../b/index';\n",
        );
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 1;\n");

        let specs = vec![
            spec_with_boundaries(
                "a",
                "src/a/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["a".to_string()]),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("b", "src/b/**/*", Boundaries::default()),
        ];

        let violations = run_engine(&temp, SpecConfig::default(), specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "boundary.allow_imports_from");
        assert_ne!(
            (
                violations[0].line.unwrap_or(0),
                violations[0].column.unwrap_or(0)
            ),
            (0, 0)
        );
    }

    #[test]
    fn violations_are_deterministically_sorted() {
        let temp = TempDir::new().expect("tempdir");

        write_test_file(
            temp.path(),
            "src/z/main.ts",
            "import { a } from '../a/index';\nimport { b } from '../b/index';\nexport const z = a + b;\n",
        );
        write_test_file(
            temp.path(),
            "src/y/main.ts",
            "import { a } from '../a/index';\nexport const y = a;\n",
        );
        write_test_file(temp.path(), "src/a/index.ts", "export const a = 1;\n");
        write_test_file(temp.path(), "src/b/index.ts", "export const b = 2;\n");

        let specs = vec![
            spec_with_boundaries(
                "z",
                "src/z/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["z".to_string()]),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries(
                "y",
                "src/y/**/*",
                Boundaries {
                    allow_imports_from: Some(vec!["y".to_string()]),
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
