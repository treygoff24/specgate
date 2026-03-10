use std::fs;
use std::path::Path;

use globset::GlobSet;

use crate::rules::{
    GlobCompileError, RuleContext, RuleViolation, compile_optional_globset_strict,
    matches_test_file, sort_violations_stable,
};

pub const HYGIENE_DEEP_THIRD_PARTY_RULE_ID: &str = "hygiene.deep_third_party_import";
pub const HYGIENE_TEST_IN_PRODUCTION_RULE_ID: &str = "hygiene.test_in_production";
pub const HYGIENE_CONFIG_ERROR_RULE_ID: &str = "hygiene.config_error";

/// Parse the package name from a specifier, returning (package_name, Option<subpath>).
///
/// - `express/lib/router` → ("express", Some("lib/router"))
/// - `express` → ("express", None)
/// - `@org/pkg/internal` → ("@org/pkg", Some("internal"))
/// - `@org/pkg` → ("@org/pkg", None)
pub fn parse_package_name(specifier: &str) -> (&str, Option<&str>) {
    if specifier.starts_with('@') {
        // Scoped package: @scope/name[/subpath]
        let mut slash_count = 0;
        let mut second_slash = None;
        for (i, c) in specifier.char_indices() {
            if c == '/' {
                slash_count += 1;
                if slash_count == 2 {
                    second_slash = Some(i);
                    break;
                }
            }
        }
        match second_slash {
            Some(pos) => (&specifier[..pos], Some(&specifier[pos + 1..])),
            None => (specifier, None),
        }
    } else {
        // Regular package: name[/subpath]
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
    compile_optional_globset_strict(&all).map_err(|e| match e {
        GlobCompileError::InvalidPattern { pattern, source } => {
            format!("invalid test pattern glob: pattern '{pattern}' is invalid: {source}")
        }
        GlobCompileError::Build { source } => {
            format!("invalid test pattern glob: failed to build matcher: {source}")
        }
    })
}

fn check_deep_import(
    specifier: &str,
    deny_deep_imports: &[String],
    from_file: &Path,
    position: Option<(u32, u32)>,
) -> Option<RuleViolation> {
    let (pkg, subpath) = parse_package_name(specifier);
    if subpath.is_some() && deny_deep_imports.iter().any(|p| p == pkg) {
        Some(build_violation(
            HYGIENE_DEEP_THIRD_PARTY_RULE_ID,
            format!("deep import into '{pkg}' is not allowed: '{specifier}'"),
            from_file,
            None,
            position,
        ))
    } else {
        None
    }
}

fn build_violation(
    rule: &str,
    message: String,
    from_file: &Path,
    to_file: Option<&Path>,
    position: Option<(u32, u32)>,
) -> RuleViolation {
    RuleViolation {
        rule: rule.to_string(),
        message,
        from_file: from_file.to_path_buf(),
        to_file: to_file.map(Path::to_path_buf),
        from_module: None,
        to_module: None,
        line: position.map(|(l, _)| l),
        column: position.map(|(_, c)| c),
    }
}

/// Evaluate import hygiene rules: deep third-party imports and test-production boundary.
pub fn evaluate_hygiene_rules(ctx: &RuleContext<'_>) -> Vec<RuleViolation> {
    let hygiene = &ctx.config.import_hygiene;
    let mut violations = Vec::new();
    let canonical_root =
        fs::canonicalize(ctx.project_root).unwrap_or_else(|_| ctx.project_root.to_path_buf());

    let test_matcher = match build_combined_test_matcher(
        &ctx.config.test_patterns,
        &hygiene.test_boundary.test_patterns,
    ) {
        Ok(matcher) => matcher,
        Err(error) => {
            violations.push(build_violation(
                HYGIENE_CONFIG_ERROR_RULE_ID,
                error,
                &ctx.project_root.join("specgate.config.yml"),
                None,
                None,
            ));
            None
        }
    };

    for node in ctx.graph.files() {
        if !hygiene.deny_deep_imports.is_empty() {
            for import in &node.analysis.imports {
                if let Some(v) = check_deep_import(
                    &import.specifier,
                    &hygiene.deny_deep_imports,
                    &node.path,
                    Some((import.line, import.column)),
                ) {
                    violations.push(v);
                }
            }

            for re_export in &node.analysis.re_exports {
                if let Some(v) = check_deep_import(
                    &re_export.specifier,
                    &hygiene.deny_deep_imports,
                    &node.path,
                    Some((re_export.line, re_export.column)),
                ) {
                    violations.push(v);
                }
            }

            for require in &node.analysis.require_calls {
                if let Some(v) = check_deep_import(
                    &require.specifier,
                    &hygiene.deny_deep_imports,
                    &node.path,
                    Some((require.line, require.column)),
                ) {
                    violations.push(v);
                }
            }

            for dynamic in &node.analysis.dynamic_imports {
                if let Some(v) = check_deep_import(
                    &dynamic.specifier,
                    &hygiene.deny_deep_imports,
                    &node.path,
                    Some((dynamic.line, dynamic.column)),
                ) {
                    violations.push(v);
                }
            }
        }

        if hygiene.test_boundary.deny_production_imports {
            let importer_is_test =
                matches_test_file(ctx.project_root, &node.path, test_matcher.as_ref());
            if importer_is_test {
                continue;
            }

            for edge in ctx.graph.dependencies_from(&node.path) {
                let target_is_test =
                    matches_test_file(ctx.project_root, &edge.to, test_matcher.as_ref());
                if target_is_test {
                    violations.push(build_violation(
                        HYGIENE_TEST_IN_PRODUCTION_RULE_ID,
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
            }
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
    use crate::rules::write_test_file;
    use crate::spec::config::{ImportHygieneConfig, TestBoundaryConfig};
    use crate::spec::{Boundaries, SpecConfig, SpecFile};

    use super::*;

    fn spec_with_path(module: &str, path: &str) -> SpecFile {
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

    // ---- parse_package_name helper tests ----

    #[test]
    fn test_parse_package_name_simple() {
        let (pkg, sub) = parse_package_name("express/lib/router");
        assert_eq!(pkg, "express");
        assert_eq!(sub, Some("lib/router"));
    }

    #[test]
    fn test_parse_package_name_scoped() {
        let (pkg, sub) = parse_package_name("@org/pkg/deep");
        assert_eq!(pkg, "@org/pkg");
        assert_eq!(sub, Some("deep"));
    }

    #[test]
    fn test_parse_package_name_no_subpath() {
        let (pkg, sub) = parse_package_name("express");
        assert_eq!(pkg, "express");
        assert_eq!(sub, None);
    }

    // ---- Config tests ----

    #[test]
    fn test_import_hygiene_defaults_empty() {
        let config = SpecConfig::default();
        assert!(config.import_hygiene.deny_deep_imports.is_empty());
        assert!(!config.import_hygiene.test_boundary.deny_production_imports);
        assert!(config.import_hygiene.test_boundary.test_patterns.is_empty());
    }

    #[test]
    fn test_import_hygiene_config_parse() {
        let parsed: SpecConfig = yaml_serde::from_str(
            r#"
import_hygiene:
  deny_deep_imports:
    - express
    - lodash
  test_boundary:
    deny_production_imports: true
    test_patterns:
      - "**/*.fixture.ts"
"#,
        )
        .expect("parse config");

        assert_eq!(
            parsed.import_hygiene.deny_deep_imports,
            vec!["express", "lodash"]
        );
        assert!(parsed.import_hygiene.test_boundary.deny_production_imports);
        assert_eq!(
            parsed.import_hygiene.test_boundary.test_patterns,
            vec!["**/*.fixture.ts"]
        );
    }

    #[test]
    fn test_config_backward_compat_without_hygiene() {
        let parsed: SpecConfig = yaml_serde::from_str("spec_dirs:\n  - specs\n").expect("parse");
        assert!(parsed.import_hygiene.deny_deep_imports.is_empty());
        assert!(!parsed.import_hygiene.test_boundary.deny_production_imports);
    }

    // ---- Deep import detection tests ----

    #[test]
    fn test_deep_import_detected_for_configured_package() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { Router } from 'express/lib/router';\nexport const x = Router;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["express".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
        assert!(violations[0].message.contains("express"));
        assert!(violations[0].message.contains("express/lib/router"));
    }

    #[test]
    fn test_top_level_import_not_flagged() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import express from 'express';\nexport const x = express;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["express".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_scoped_package_deep_import() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { x } from '@org/pkg/internal';\nexport const y = x;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["@org/pkg".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
        assert!(violations[0].message.contains("@org/pkg"));
    }

    #[test]
    fn test_scoped_package_top_level_ok() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { x } from '@org/pkg';\nexport const y = x;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["@org/pkg".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_unconfigured_package_not_flagged() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { x } from 'lodash/fp';\nexport const y = x;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["express".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_deep_import_with_index_suffix() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { x } from 'lodash/fp/index';\nexport const y = x;\n",
        );
        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["lodash".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
    }

    // ---- Test-production boundary tests ----

    #[test]
    fn test_production_importing_test_file_flagged() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { helper } from '../__tests__/helpers';\nexport const x = helper;\n",
        );
        write_test_file(
            temp.path(),
            "src/__tests__/helpers.ts",
            "export const helper = 'test-helper';\n",
        );

        let specs = vec![
            spec_with_path("app", "src/app/**/*"),
            spec_with_path("tests", "src/__tests__/**/*"),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    deny_production_imports: true,
                    ..TestBoundaryConfig::default()
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_TEST_IN_PRODUCTION_RULE_ID);
    }

    #[test]
    fn test_test_file_importing_test_ok() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.test.ts",
            "import { helper } from '../__tests__/helpers';\nexport const x = helper;\n",
        );
        write_test_file(
            temp.path(),
            "src/__tests__/helpers.ts",
            "export const helper = 'test-helper';\n",
        );

        let specs = vec![
            spec_with_path("app", "src/app/**/*"),
            spec_with_path("tests", "src/__tests__/**/*"),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    deny_production_imports: true,
                    ..TestBoundaryConfig::default()
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_production_importing_production_ok() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { util } from '../lib/util';\nexport const x = util;\n",
        );
        write_test_file(temp.path(), "src/lib/util.ts", "export const util = 1;\n");

        let specs = vec![
            spec_with_path("app", "src/app/**/*"),
            spec_with_path("lib", "src/lib/**/*"),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    deny_production_imports: true,
                    ..TestBoundaryConfig::default()
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_disabled_test_boundary_no_flags() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { helper } from '../__tests__/helpers';\nexport const x = helper;\n",
        );
        write_test_file(
            temp.path(),
            "src/__tests__/helpers.ts",
            "export const helper = 'test-helper';\n",
        );

        let specs = vec![
            spec_with_path("app", "src/app/**/*"),
            spec_with_path("tests", "src/__tests__/**/*"),
        ];
        // deny_production_imports defaults to false
        let config = SpecConfig::default();

        let violations = run_hygiene(&temp, config, specs);
        assert!(violations.is_empty());
    }

    // ---- Finding #4: invalid test pattern surfaces config error ----

    #[test]
    fn test_invalid_test_pattern_surfaces_config_error() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(temp.path(), "src/app/main.ts", "export const x = 1;\n");

        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    test_patterns: vec!["[invalid".to_string()],
                    deny_production_imports: true,
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        let config_errors: Vec<_> = violations
            .iter()
            .filter(|v| v.rule == HYGIENE_CONFIG_ERROR_RULE_ID)
            .collect();
        assert_eq!(
            config_errors.len(),
            1,
            "expected 1 config_error violation, got {}: {violations:?}",
            config_errors.len()
        );
        assert!(
            config_errors[0]
                .message
                .contains("invalid test pattern glob"),
            "expected config error message, got: {}",
            config_errors[0].message
        );
    }

    // ---- Finding #5: deep imports via re-exports and require calls ----

    #[test]
    fn test_deep_import_via_reexport_detected() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "export { x } from 'express/lib/thing';\n",
        );

        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["express".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(
            violations.len(),
            1,
            "expected 1 violation, got: {violations:?}"
        );
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
        assert!(
            violations[0].message.contains("express/lib/thing"),
            "expected specifier in message: {}",
            violations[0].message
        );
    }

    #[test]
    fn test_deep_import_via_require_detected() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.js",
            "const x = require('express/lib/thing');\nmodule.exports = x;\n",
        );

        let specs = vec![spec_with_path("app", "src/app/**/*")];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                deny_deep_imports: vec!["express".to_string()],
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(
            violations.len(),
            1,
            "expected 1 violation, got: {violations:?}"
        );
        assert_eq!(violations[0].rule, HYGIENE_DEEP_THIRD_PARTY_RULE_ID);
        assert!(
            violations[0].message.contains("express/lib/thing"),
            "expected specifier in message: {}",
            violations[0].message
        );
    }

    // ---- Finding #11: violation message uses normalized repo-relative paths ----

    #[test]
    fn test_hygiene_violation_uses_relative_paths() {
        let temp = TempDir::new().expect("tempdir");
        write_test_file(
            temp.path(),
            "src/app/main.ts",
            "import { helper } from '../__tests__/helpers';\nexport const x = helper;\n",
        );
        write_test_file(
            temp.path(),
            "src/__tests__/helpers.ts",
            "export const helper = 'test-helper';\n",
        );

        let specs = vec![
            spec_with_path("app", "src/app/**/*"),
            spec_with_path("tests", "src/__tests__/**/*"),
        ];
        let config = SpecConfig {
            import_hygiene: ImportHygieneConfig {
                test_boundary: TestBoundaryConfig {
                    deny_production_imports: true,
                    ..TestBoundaryConfig::default()
                },
                ..ImportHygieneConfig::default()
            },
            ..SpecConfig::default()
        };

        let violations = run_hygiene(&temp, config, specs);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, HYGIENE_TEST_IN_PRODUCTION_RULE_ID);

        let message = &violations[0].message;
        assert!(
            !message.contains(temp.path().to_str().unwrap()),
            "message contains absolute path: {message}"
        );
        assert!(
            message.contains("src/__tests__/helpers.ts"),
            "message missing relative path: {message}"
        );
    }
}
