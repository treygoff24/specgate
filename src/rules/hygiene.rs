use std::path::Path;

use globset::GlobSet;

use crate::rules::{
    RuleContext, RuleViolation, compile_optional_globset_strict, matches_test_file,
    sort_violations_stable,
};

pub const HYGIENE_DEEP_THIRD_PARTY_RULE_ID: &str = "hygiene.deep_third_party_import";
pub const HYGIENE_TEST_IN_PRODUCTION_RULE_ID: &str = "hygiene.test_in_production";

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
) -> Option<GlobSet> {
    let mut all = Vec::new();
    all.extend_from_slice(config_patterns);
    all.extend_from_slice(extra_patterns);
    compile_optional_globset_strict(&all).ok().flatten()
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

    let test_matcher = build_combined_test_matcher(
        &ctx.config.test_patterns,
        &hygiene.test_boundary.test_patterns,
    );

    for node in ctx.graph.files() {
        // Deep third-party import check: scan raw import specifiers
        if !hygiene.deny_deep_imports.is_empty() {
            for import in &node.analysis.imports {
                let specifier = &import.specifier;
                let (pkg, subpath) = parse_package_name(specifier);
                if subpath.is_some() && hygiene.deny_deep_imports.iter().any(|p| p == pkg) {
                    violations.push(build_violation(
                        HYGIENE_DEEP_THIRD_PARTY_RULE_ID,
                        format!("deep import into '{pkg}' is not allowed: '{specifier}'"),
                        &node.path,
                        None,
                        Some((import.line, import.column)),
                    ));
                }
            }
        }

        // Test-production boundary check: scan first-party graph edges
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
                            edge.to.display()
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
    use crate::spec::{Boundaries, SpecConfig, SpecFile};
    use crate::spec::config::{ImportHygieneConfig, TestBoundaryConfig};

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
}
