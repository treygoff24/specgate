use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use globset::{Glob, GlobSet};

use crate::rules::{
    GlobCompileError, RuleContext, RuleViolation, compile_optional_globset_strict,
    matches_test_file, sort_violations_stable,
};
use crate::spec::config::{DenyDeepImportEntry, TestBoundaryMode};
use crate::spec::types::ModuleDenyDeepImportEntry;
use crate::spec::{Severity, SpecFile};

pub const HYGIENE_DEEP_THIRD_PARTY_RULE_ID: &str = "hygiene.deep_third_party_import";
pub const HYGIENE_TEST_IN_PRODUCTION_RULE_ID: &str = "hygiene.test_in_production";
pub const HYGIENE_CONFIG_ERROR_RULE_ID: &str = "hygiene.config_error";
pub const HYGIENE_UNRESOLVED_IMPORT_RULE_ID: &str = "hygiene.unresolved_import";

#[derive(Debug, Clone, Copy)]
struct DeepImportDecision<'a> {
    package_name: &'a str,
    max_depth: usize,
    severity: Severity,
}

/// Parse the package name from a specifier, returning (package_name, Option<subpath>).
///
/// - `express/lib/router` -> ("express", Some("lib/router"))
/// - `express` -> ("express", None)
/// - `@org/pkg/internal` -> ("@org/pkg", Some("internal"))
/// - `@org/pkg` -> ("@org/pkg", None)
pub fn parse_package_name(specifier: &str) -> (&str, Option<&str>) {
    if specifier.starts_with('@') {
        let mut slash_count = 0;
        let mut second_slash = None;
        for (idx, ch) in specifier.char_indices() {
            if ch == '/' {
                slash_count += 1;
                if slash_count == 2 {
                    second_slash = Some(idx);
                    break;
                }
            }
        }
        match second_slash {
            Some(pos) => (&specifier[..pos], Some(&specifier[pos + 1..])),
            None => (specifier, None),
        }
    } else {
        match specifier.find('/') {
            Some(pos) => (&specifier[..pos], Some(&specifier[pos + 1..])),
            None => (specifier, None),
        }
    }
}

fn build_combined_test_matcher(
    config_patterns: &[String],
    extra_patterns: &[String],
) -> std::result::Result<Option<GlobSet>, String> {
    let mut all = Vec::new();
    all.extend_from_slice(config_patterns);
    all.extend_from_slice(extra_patterns);
    compile_optional_globset_strict(&all).map_err(|error| match error {
        GlobCompileError::InvalidPattern { pattern, source } => {
            format!("invalid test pattern glob: pattern '{pattern}' is invalid: {source}")
        }
        GlobCompileError::Build { source } => {
            format!("invalid test pattern glob: failed to build matcher: {source}")
        }
    })
}

fn pattern_matches_specifier(pattern: &str, specifier: &str, package_name: &str) -> bool {
    if pattern == package_name || pattern == specifier {
        return true;
    }

    let Ok(glob) = Glob::new(pattern) else {
        return false;
    };
    let matcher = glob.compile_matcher();
    matcher.is_match(specifier) || matcher.is_match(package_name)
}

fn subpath_depth(subpath: &str) -> usize {
    subpath
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

fn config_entry_for_specifier<'a>(
    specifier: &str,
    package_name: &str,
    entries: &'a [DenyDeepImportEntry],
) -> Option<&'a DenyDeepImportEntry> {
    entries
        .iter()
        .find(|entry| pattern_matches_specifier(&entry.pattern, specifier, package_name))
}

fn module_entry_for_specifier<'a>(
    specifier: &str,
    package_name: &str,
    entries: &'a [ModuleDenyDeepImportEntry],
) -> Option<&'a ModuleDenyDeepImportEntry> {
    entries
        .iter()
        .find(|entry| pattern_matches_specifier(&entry.pattern, specifier, package_name))
}

fn effective_deep_import_decision<'a>(
    specifier: &'a str,
    module_entries: &[ModuleDenyDeepImportEntry],
    config_entries: &[DenyDeepImportEntry],
) -> Option<DeepImportDecision<'a>> {
    let (package_name, subpath) = parse_package_name(specifier);
    subpath?;

    let config_match = config_entry_for_specifier(specifier, package_name, config_entries);
    if let Some(module_match) = module_entry_for_specifier(specifier, package_name, module_entries)
    {
        if module_match.allow {
            return None;
        }

        return Some(DeepImportDecision {
            package_name,
            max_depth: module_match
                .max_depth
                .unwrap_or_else(|| config_match.map_or(0, |entry| entry.max_depth)),
            severity: config_match
                .map_or(Severity::Warning, DenyDeepImportEntry::effective_severity),
        });
    }

    config_match.map(|entry| DeepImportDecision {
        package_name,
        max_depth: entry.max_depth,
        severity: entry.effective_severity(),
    })
}

fn deep_import_violation(
    specifier: &str,
    module_entries: &[ModuleDenyDeepImportEntry],
    config_entries: &[DenyDeepImportEntry],
    from_file: &Path,
    position: Option<(u32, u32)>,
) -> Option<RuleViolation> {
    let (_, Some(subpath)) = parse_package_name(specifier) else {
        return None;
    };

    let decision = effective_deep_import_decision(specifier, module_entries, config_entries)?;
    let depth = subpath_depth(subpath);
    if depth <= decision.max_depth {
        return None;
    }

    let message = if decision.max_depth == 0 {
        format!(
            "deep import into '{}' is not allowed: '{}'",
            decision.package_name, specifier
        )
    } else {
        format!(
            "deep import into '{}' exceeds max depth {}: '{}'",
            decision.package_name, decision.max_depth, specifier
        )
    };

    Some(build_violation(
        HYGIENE_DEEP_THIRD_PARTY_RULE_ID,
        decision.severity,
        message,
        from_file,
        None,
        position,
    ))
}

fn build_violation(
    rule: &str,
    severity: Severity,
    message: String,
    from_file: &Path,
    to_file: Option<&Path>,
    position: Option<(u32, u32)>,
) -> RuleViolation {
    RuleViolation {
        rule: rule.to_string(),
        severity: Some(severity),
        message,
        from_file: from_file.to_path_buf(),
        to_file: to_file.map(Path::to_path_buf),
        from_module: None,
        to_module: None,
        line: position.map(|(line, _)| line),
        column: position.map(|(_, column)| column),
    }
}

fn boundary_mode_for_file(
    spec: Option<&SpecFile>,
    config_mode: TestBoundaryMode,
) -> TestBoundaryMode {
    spec.and_then(|spec| spec.boundaries.as_ref())
        .and_then(|boundaries| boundaries.import_hygiene.as_ref())
        .and_then(|hygiene| hygiene.test_boundary.as_ref())
        .map_or(config_mode, |override_mode| override_mode.mode)
}

fn build_public_api_matchers(specs: &[SpecFile]) -> BTreeMap<String, Option<GlobSet>> {
    let mut matchers = BTreeMap::new();

    for spec in specs {
        let matcher = spec
            .boundaries
            .as_ref()
            .filter(|boundaries| !boundaries.public_api.is_empty())
            .and_then(|boundaries| compile_optional_globset_strict(&boundaries.public_api).ok())
            .flatten();
        matchers.insert(spec.module.clone(), matcher);
    }

    matchers
}

fn evaluate_deep_imports(
    node: &crate::graph::FileNode,
    module_entries: &[ModuleDenyDeepImportEntry],
    config_entries: &[DenyDeepImportEntry],
    violations: &mut Vec<RuleViolation>,
) {
    for import in &node.analysis.imports {
        if let Some(violation) = deep_import_violation(
            &import.specifier,
            module_entries,
            config_entries,
            &node.path,
            Some((import.line, import.column)),
        ) {
            violations.push(violation);
        }
    }

    for re_export in &node.analysis.re_exports {
        if let Some(violation) = deep_import_violation(
            &re_export.specifier,
            module_entries,
            config_entries,
            &node.path,
            Some((re_export.line, re_export.column)),
        ) {
            violations.push(violation);
        }
    }

    for require in &node.analysis.require_calls {
        if let Some(violation) = deep_import_violation(
            &require.specifier,
            module_entries,
            config_entries,
            &node.path,
            Some((require.line, require.column)),
        ) {
            violations.push(violation);
        }
    }

    for dynamic in &node.analysis.dynamic_imports {
        if let Some(violation) = deep_import_violation(
            &dynamic.specifier,
            module_entries,
            config_entries,
            &node.path,
            Some((dynamic.line, dynamic.column)),
        ) {
            violations.push(violation);
        }
    }
}

/// Evaluate import hygiene rules: deep third-party imports and test-production boundary.
pub fn evaluate_hygiene_rules(ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
    let hygiene = &ctx.config.import_hygiene;
    let mut violations = Vec::new();
    let canonical_root =
        fs::canonicalize(ctx.project_root).unwrap_or_else(|_| ctx.project_root.to_path_buf());
    let spec_by_module = ctx
        .specs
        .iter()
        .map(|spec| (spec.module.as_str(), spec))
        .collect::<BTreeMap<_, _>>();
    let public_api_matchers = build_public_api_matchers(ctx.specs);

    let test_matcher = match build_combined_test_matcher(
        &ctx.config.test_patterns,
        &hygiene.test_boundary.test_patterns,
    ) {
        Ok(matcher) => matcher,
        Err(error) => {
            violations.push(build_violation(
                HYGIENE_CONFIG_ERROR_RULE_ID,
                Severity::Error,
                error,
                &ctx.project_root.join("specgate.config.yml"),
                None,
                None,
            ));
            None
        }
    };

    let config_test_boundary_mode = hygiene.test_boundary.effective_mode();

    for node in ctx.graph.files() {
        let importer_spec = node
            .module_id
            .as_deref()
            .and_then(|module| spec_by_module.get(module).copied());
        let module_entries = importer_spec
            .and_then(|spec| spec.boundaries.as_ref())
            .and_then(|boundaries| boundaries.import_hygiene.as_ref())
            .map_or(&[][..], |hygiene| hygiene.deny_deep_imports.as_slice());

        evaluate_deep_imports(
            node,
            module_entries,
            &hygiene.deny_deep_imports,
            &mut violations,
        );

        let effective_mode = boundary_mode_for_file(importer_spec, config_test_boundary_mode);
        if effective_mode == TestBoundaryMode::Off {
            continue;
        }

        let importer_is_test =
            matches_test_file(ctx.project_root, &node.path, test_matcher.as_ref());

        for edge in ctx.graph.dependencies_from(&node.path) {
            let target_is_test =
                matches_test_file(ctx.project_root, &edge.to, test_matcher.as_ref());

            if !importer_is_test {
                if target_is_test {
                    violations.push(build_violation(
                        HYGIENE_TEST_IN_PRODUCTION_RULE_ID,
                        Severity::Error,
                        format!(
                            "production file imports from test file '{}'",
                            crate::deterministic::normalize_repo_relative(
                                &canonical_root,
                                &edge.to
                            )
                        ),
                        &node.path,
                        Some(&edge.to),
                        edge.line.zip(edge.column),
                    ));
                }
                continue;
            }

            if effective_mode != TestBoundaryMode::Bidirectional {
                continue;
            }

            let Some(target_module) = ctx.graph.module_of_file(&edge.to) else {
                continue;
            };
            if node.module_id.as_deref() == Some(target_module) {
                continue;
            }

            let Some(Some(public_api_matcher)) = public_api_matchers.get(target_module) else {
                continue;
            };

            let target_rel =
                crate::deterministic::normalize_repo_relative(&canonical_root, &edge.to);
            if public_api_matcher.is_match(&target_rel) {
                continue;
            }

            violations.push(build_violation(
                HYGIENE_TEST_IN_PRODUCTION_RULE_ID,
                Severity::Error,
                format!(
                    "test file imports non-public file '{target_rel}' from module '{target_module}'"
                ),
                &node.path,
                Some(&edge.to),
                edge.line.zip(edge.column),
            ));
        }
    }

    sort_violations_stable(&mut violations);
    violations
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::graph::DependencyGraph;
    use crate::resolver::ModuleResolver;
    use crate::rules::test_support::build_spec_with_boundaries;
    use crate::rules::write_test_file;
    use crate::spec::config::{DenyDeepImportEntry, ImportHygieneConfig, TestBoundaryConfig};
    use crate::spec::types::{
        Boundaries, ModuleDenyDeepImportEntry, ModuleImportHygiene, ModuleTestBoundaryOverride,
    };
    use crate::spec::{SpecConfig, SpecFile};

    use super::*;

    fn spec_with_boundaries(module: &str, path: &str, boundaries: Boundaries) -> SpecFile {
        build_spec_with_boundaries("2.3", module, path, boundaries)
    }

    fn run_hygiene(temp: &TempDir, config: SpecConfig, specs: Vec<SpecFile>) -> Vec<RuleViolation> {
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("graph");
        let ctx = RuleContext {
            project_root: temp.path(),
            config: &config,
            specs: &specs,
            graph: &graph,
        };
        evaluate_hygiene_rules(&ctx)
    }

    #[test]
    fn parse_package_name_handles_unscoped_and_scoped_packages() {
        assert_eq!(
            parse_package_name("express/lib/router"),
            ("express", Some("lib/router"))
        );
        assert_eq!(parse_package_name("express"), ("express", None));
        assert_eq!(
            parse_package_name("@org/pkg/internal"),
            ("@org/pkg", Some("internal"))
        );
        assert_eq!(parse_package_name("@org/pkg"), ("@org/pkg", None));
    }

    #[test]
    fn test_import_hygiene_defaults_empty() {
        let config = SpecConfig::default();
        assert!(config.import_hygiene.deny_deep_imports.is_empty());
        assert_eq!(
            config.import_hygiene.test_boundary.effective_mode(),
            TestBoundaryMode::Off
        );
        assert!(config.import_hygiene.test_boundary.test_patterns.is_empty());
    }

    #[test]
    fn deep_import_respects_max_depth_and_emits_warning_by_default() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { value } from 'lodash/internal/deep';\nexport const x = value;\n",
        );

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries::default(),
        )];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec![DenyDeepImportEntry {
                    pattern: "lodash/**".to_string(),
                    max_depth: 1,
                    severity: None,
                }],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
        assert_eq!(violations[0].severity, Some(Severity::Warning));
    }

    #[test]
    fn module_override_allow_suppresses_matching_config_entry() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { helper } from 'internal-sdk/testing';\nexport const x = helper;\n",
        );

        let specs = vec![spec_with_boundaries(
            "app",
            "src/app/**/*",
            Boundaries {
                import_hygiene: Some(ModuleImportHygiene {
                    deny_deep_imports: vec![ModuleDenyDeepImportEntry {
                        pattern: "internal-sdk/**".to_string(),
                        max_depth: None,
                        allow: true,
                    }],
                    test_boundary: None,
                }),
                ..Boundaries::default()
            },
        )];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec![DenyDeepImportEntry {
                    pattern: "*".to_string(),
                    max_depth: 0,
                    severity: Some(Severity::Error),
                }],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn bidirectional_mode_blocks_cross_module_test_internal_imports() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.test.ts",
            "import { secret } from '../core/internal';\nexport const value = secret;\n",
        );
        write_test_file(
            temp.path(),
            "src/core/internal.ts",
            "export const secret = 1;\n",
        );
        write_test_file(temp.path(), "src/core/index.ts", "export const api = 1;\n");

        let specs = vec![
            spec_with_boundaries("app", "src/app/**/*", Boundaries::default()),
            spec_with_boundaries(
                "core",
                "src/core/**/*",
                Boundaries {
                    public_api: vec!["src/core/index.ts".to_string()],
                    ..Boundaries::default()
                },
            ),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    enabled: true,
                    mode: TestBoundaryMode::Bidirectional,
                    test_patterns: Vec::new(),
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_TEST_IN_PRODUCTION_RULE_ID);
        assert!(violations[0].message.contains("non-public file"));
    }

    #[test]
    fn module_override_can_disable_test_boundary_checks() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { helper } from '../tests/helper';\nexport const x = helper;\n",
        );
        write_test_file(
            temp.path(),
            "src/tests/helper.ts",
            "export const helper = 1;\n",
        );

        let specs = vec![
            spec_with_boundaries(
                "app",
                "src/app/**/*",
                Boundaries {
                    import_hygiene: Some(ModuleImportHygiene {
                        deny_deep_imports: Vec::new(),
                        test_boundary: Some(ModuleTestBoundaryOverride {
                            mode: TestBoundaryMode::Off,
                        }),
                    }),
                    ..Boundaries::default()
                },
            ),
            spec_with_boundaries("tests", "src/tests/**/*", Boundaries::default()),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    enabled: true,
                    mode: TestBoundaryMode::ProductionOnly,
                    test_patterns: vec!["src/tests/**/*".to_string()],
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }
}
