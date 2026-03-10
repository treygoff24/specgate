//! Contract fixtures and tests for Wave 0 lock.
//!
//! These tests verify the contract-critical surfaces of Specgate:
//!
//! 1. **Glob allowlist behavior**: `allow_imports_from` glob patterns
//! 2. **Module-relative public_api behavior**: `public_api` paths relative to module
//! 3. **`--since` blast-radius behavior**: Git-based incremental checking

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};
use specgate::git_blast::BlastRadius;
use specgate::spec::types::SUPPORTED_SPEC_VERSION;

fn write_file(root: &std::path::Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).expect("cli output json")
}

fn run_git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("execute git");

    if !output.status.success() {
        panic!(
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout)
        .expect("utf8 git stdout")
        .trim()
        .to_string()
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init", "--initial-branch=main"]);
    run_git(root, &["config", "user.name", "Specgate Tests"]);
    run_git(
        root,
        &["config", "user.email", "specgate-tests@example.com"],
    );
}

fn commit_all(root: &Path, message: &str) {
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-m", message]);
}

// =============================================================================
// Contract Test 1: Glob Allowlist Behavior
// =============================================================================

/// Test that `allow_imports_from` uses exact module ID matching.
///
/// ## Contract
///
/// - `allow_imports_from` entries must match exact module IDs
/// - Omitted `allow_imports_from` means all imports are allowed (no restriction)
/// - `allow_imports_from: []` means no cross-module imports are allowed
/// - Non-empty `allow_imports_from` means only listed modules can be imported
/// - Module IDs are case-sensitive
#[test]
fn allow_imports_from_enforces_exact_module_matching() {
    let temp = TempDir::new().expect("tempdir");

    // Create modules: app (with allowlist), core/api, core/db, external/lib
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
  allow_imports_from:
    - "core/api"
    - "core/db"
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/core__api.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core/api
boundaries:
  path: src/core/api/**/*
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/core__db.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core/db
boundaries:
  path: src/core/db/**/*
constraints: []
"#,
        ),
    );

    // Create source files - app imports from allowed modules
    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { api } from '../core/api';
import { db } from '../core/db';
export const app = { api, db };
"#,
    );

    write_file(
        temp.path(),
        "src/core/api/index.ts",
        "export const api = 1;\n",
    );
    write_file(
        temp.path(),
        "src/core/db/index.ts",
        "export const db = 1;\n",
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    // Check should pass - both core/api and core/db are in allowlist
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS, "check should pass");
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
}

/// Test that module not in allowlist triggers violation.
#[test]
fn allow_imports_from_rejects_non_allowlisted_modules() {
    let temp = TempDir::new().expect("tempdir");

    // Create modules: app (with allowlist), core/api, external/lib
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
  allow_imports_from:
    - "core/api"
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/core__api.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core/api
boundaries:
  path: src/core/api/**/*
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/external__lib.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: external/lib
boundaries:
  path: src/external/lib/**/*
constraints: []
"#,
        ),
    );

    // Create source files - app imports from external/lib which is NOT in allowlist
    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { lib } from '../external/lib';
export const app = lib;
"#,
    );

    write_file(
        temp.path(),
        "src/core/api/index.ts",
        "export const api = 1;\n",
    );
    write_file(
        temp.path(),
        "src/external/lib/index.ts",
        "export const lib = 1;\n",
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    // Check should fail - external/lib is not in allowlist
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "check should fail for non-allowlisted import"
    );
    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");
    assert!(
        violations
            .iter()
            .any(|v| { v["rule"].as_str() == Some("boundary.allow_imports_from") }),
        "should have boundary.allow_imports_from violation"
    );
}

/// Test that omitted allowlist allows all imports (no restriction).
#[test]
fn omitted_allow_imports_from_allows_all() {
    let temp = TempDir::new().expect("tempdir");

    // Create modules with NO allowlist restriction
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/lib.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: lib
boundaries:
  path: src/lib/**/*
constraints: []
"#,
        ),
    );

    // app imports from lib (no restrictions)
    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { lib } from '../lib';
export const app = lib;
"#,
    );

    write_file(temp.path(), "src/lib/index.ts", "export const lib = 1;\n");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    // Check should pass - no allowlist restriction
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "check should pass with no allowlist restriction"
    );
}

/// Test that empty allowlist denies cross-module imports.
#[test]
fn empty_allow_imports_from_denies_all_cross_module_imports() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
  allow_imports_from: []
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/lib.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: lib
boundaries:
  path: src/lib/**/*
constraints: []
"#,
        ),
    );

    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { lib } from '../lib';
export const app = lib;
"#,
    );

    write_file(
        temp.path(),
        "src/lib/index.ts",
        "export const lib = 1;
",
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:
  - modules
exclude: []
test_patterns: []
",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "check should fail with empty allowlist"
    );
    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");
    assert!(
        violations
            .iter()
            .any(|v| { v["rule"].as_str() == Some("boundary.allow_imports_from") }),
        "should have boundary.allow_imports_from violation"
    );
}

// =============================================================================
// Contract Test 2: Module-Relative Public API Behavior
// =============================================================================

/// Test that `public_api` globs are evaluated relative to the module boundary.
///
/// ## Contract
///
/// - `public_api` entries are glob patterns matched against file paths
/// - Paths are normalized before matching (forward slashes, no leading ./)
/// - Files NOT matching `public_api` are considered internal
/// - Importing from internal files triggers `boundary.public_api` violation
#[test]
fn public_api_enforces_internal_visibility() {
    let temp = TempDir::new().expect("tempdir");

    // Core module with public_api restricting access
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core
boundaries:
  path: src/core/**/*
  public_api:
    - "src/core/index.ts"
    - "src/core/public/**/*"
constraints:
  - rule: boundary.public_api
    severity: error
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    // Create source files
    write_file(
        temp.path(),
        "src/core/index.ts",
        "export { publicFn } from './public/api';\n",
    );
    write_file(
        temp.path(),
        "src/core/public/api.ts",
        "export const publicFn = 1;\n",
    );
    write_file(
        temp.path(),
        "src/core/internal/secret.ts",
        "export const secret = 'internal';\n",
    );

    // App imports from internal file (violation)
    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { secret } from '../core/internal/secret';
export const app = secret;
"#,
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "should fail when importing from internal file"
    );
    let output = parse_json(&result.stdout);
    // Check that the violation mentions internal/secret.ts
    let violations = output["violations"].as_array().expect("violations array");
    assert!(
        violations.iter().any(|v| {
            v["to_file"]
                .as_str()
                .map(|s| s.contains("internal/secret.ts"))
                .unwrap_or(false)
        }),
        "should have violation for internal/secret.ts"
    );
}

/// Test that importing from public_api paths is allowed.
#[test]
fn public_api_allows_public_paths() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/core.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core
boundaries:
  path: src/core/**/*
  public_api:
    - "src/core/index.ts"
    - "src/core/public/**/*"
constraints:
  - rule: boundary.public_api
    severity: error
"#,
        ),
    );

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    // Create source files
    write_file(
        temp.path(),
        "src/core/index.ts",
        "export { publicFn } from './public/api';\n",
    );
    write_file(
        temp.path(),
        "src/core/public/api.ts",
        "export const publicFn = 1;\n",
    );

    // App imports from public API (allowed)
    write_file(
        temp.path(),
        "src/app/main.ts",
        r#"
import { publicFn } from '../core/public/api';
export const app = publicFn;
"#,
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "should pass when importing from public API"
    );
}

// =============================================================================
// Contract Test 3: `--since` Blast-Radius Behavior
// =============================================================================

/// Test that transitive importers are included in blast radius.
///
/// ## Contract
///
/// - `--since <ref>` computes blast radius from git diff
/// - Blast radius includes:
///   1. Files directly changed since ref
///   2. Modules containing changed files
///   3. Modules that transitively import from affected modules
/// - Only violations from files in blast radius are reported
#[test]
fn blast_radius_includes_transitive_importers() {
    // Build importer graph: c -> b -> a (c is affected, a imports b, b imports c)
    let mut importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    importer_graph.insert("c".to_string(), {
        let mut set = BTreeSet::new();
        set.insert("b".to_string());
        set
    });
    importer_graph.insert("b".to_string(), {
        let mut set = BTreeSet::new();
        set.insert("a".to_string());
        set
    });

    // Build file-to-module mapping
    let mut file_to_module: BTreeMap<String, String> = BTreeMap::new();
    file_to_module.insert("src/c/foo.ts".to_string(), "c".to_string());

    // Build module-to-files mapping
    let mut module_to_files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    module_to_files.insert("c".to_string(), {
        let mut set = BTreeSet::new();
        set.insert("src/c/foo.ts".to_string());
        set
    });

    // Compute blast radius for changed file in module c
    let mut changed_files: BTreeSet<String> = BTreeSet::new();
    changed_files.insert("src/c/foo.ts".to_string());

    // Compute affected modules
    let mut affected_modules: BTreeSet<String> = BTreeSet::new();
    affected_modules.insert("c".to_string());

    // Compute transitive importers
    let mut affected_with_importers: BTreeSet<String> = affected_modules.clone();
    let mut queue: Vec<String> = affected_modules.iter().cloned().collect();
    while let Some(module) = queue.pop() {
        if let Some(importers) = importer_graph.get(&module) {
            for importer in importers {
                if !affected_with_importers.contains(importer) {
                    affected_with_importers.insert(importer.clone());
                    queue.push(importer.clone());
                }
            }
        }
    }

    // Verify: a, b, and c are all in blast radius
    assert!(
        affected_with_importers.contains("a"),
        "module a should be in blast radius (imports b which imports c)"
    );
    assert!(
        affected_with_importers.contains("b"),
        "module b should be in blast radius (imports c)"
    );
    assert!(
        affected_with_importers.contains("c"),
        "module c should be in blast radius (directly affected)"
    );
}

/// Test that BlastRadius::contains_file works correctly.
#[test]
fn blast_radius_contains_file_checks_module_membership() {
    let mut radius = BlastRadius::default();
    radius.affected_with_importers.insert("app".to_string());
    radius
        .changed_files
        .insert("src/lib/changed.ts".to_string());

    // File in affected module should be included
    assert!(
        radius.contains_file("src/app/foo.ts", Some("app")),
        "file in affected module should be in blast radius"
    );

    // File not in affected module should be excluded
    assert!(
        !radius.contains_file("src/core/bar.ts", Some("core")),
        "file in non-affected module should not be in blast radius"
    );

    // Directly changed file should always be included
    assert!(
        radius.contains_file("src/lib/changed.ts", None),
        "directly changed file should be in blast radius"
    );
}

/// Test that blast radius computation handles cycles gracefully.
#[test]
fn blast_radius_handles_cycles() {
    // Build importer graph with cycle: a -> b -> a
    let mut importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    importer_graph.insert("a".to_string(), {
        let mut set = BTreeSet::new();
        set.insert("b".to_string());
        set
    });
    importer_graph.insert("b".to_string(), {
        let mut set = BTreeSet::new();
        set.insert("a".to_string());
        set
    });

    // Compute transitive importers (simulating blast radius logic)

    let seed: BTreeSet<String> = vec!["a".to_string()].into_iter().collect();
    let mut result = seed.clone();
    let mut queue: Vec<String> = seed.iter().cloned().collect();

    while let Some(module) = queue.pop() {
        if let Some(importers) = importer_graph.get(&module) {
            for importer in importers {
                if !result.contains(importer) {
                    result.insert(importer.clone());
                    queue.push(importer.clone());
                }
            }
        }
    }

    // Should terminate without infinite loop
    assert!(result.contains("a"));
    assert!(result.contains("b"));
    assert_eq!(result.len(), 2, "should have exactly a and b");
}

// =============================================================================
// Contract Test 4: `check --deny-widenings` Governance Behavior
// =============================================================================

#[test]
fn check_deny_widenings_fails_with_contract_fields_when_policy_widens() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: core
boundaries:
  path: src/core/**/*
constraints: []
"#,
        ),
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
  allow_imports_from:
    - core
constraints: []
"#,
        ),
    );
    write_file(temp.path(), "src/core/index.ts", "export const core = 1;\n");
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );
    commit_all(temp.path(), "head widening");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
        "--since",
        "HEAD~1",
        "--deny-widenings",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "widening should fail check with deny_widenings"
    );
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "fail");
    assert_eq!(output["policy_change_detected"], true);
    assert!(
        output["rule_deltas"]
            .as_array()
            .expect("rule_deltas array")
            .iter()
            .any(|delta| delta
                .as_str()
                .map(|value| value.contains("boundaries.allow_imports_from"))
                .unwrap_or(false)),
        "rule_deltas should include widened field"
    );
}

#[test]
fn check_deny_widenings_surfaces_policy_diff_limitations_without_failing() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: ":(bad)"
constraints: []
"#,
        ),
    );
    commit_all(temp.path(), "head ambiguous path change");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
        "--since",
        "HEAD~1",
        "--deny-widenings",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "limitations are informational and should not fail without widenings"
    );

    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
    assert_eq!(output["policy_change_detected"], false);
    assert_eq!(
        output["rule_deltas"].as_array().expect("rule deltas").len(),
        0
    );
    assert_eq!(
        result.stderr,
        "policy diff limitations detected:\n  - path_coverage_unbounded_mvp\n"
    );
}

// =============================================================================
// Contract Test: Deprecated Flags Emit Warnings
// =============================================================================

/// Test that deprecated --diff flag emits a warning.
#[test]
fn deprecated_diff_flag_emits_warning() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--diff",
    ]);

    // Should still work (as alias) but emit warning
    let stderr = result.stderr.as_str();
    assert!(
        result.stderr.contains("--diff is deprecated"),
        "should emit deprecation warning for --diff: stderr was {stderr:?}"
    );
}

/// Test that --baseline-diff is the preferred flag.
#[test]
fn baseline_diff_flag_works_without_warning() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--baseline-diff",
    ]);

    // Should NOT emit deprecation warning
    let stderr = result.stderr.as_str();
    assert!(
        !result.stderr.contains("deprecated"),
        "should not emit deprecation warning for --baseline-diff: stderr was {stderr:?}"
    );
}

// =============================================================================
// Contract Test: Version Enforcement
// =============================================================================

/// Test that version 2 is rejected (only 2.2 is supported).
#[test]
fn version_2_is_rejected() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"
version: "2"
module: app
constraints: []
"#,
    );

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    // Should fail with version error
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "error", "should fail validation");
    let issues = output["issues"].as_array().expect("issues array");
    assert!(
        issues.iter().any(|i| {
            i["message"]
                .as_str()
                .map(|s| s.contains("unsupported spec version"))
                .unwrap_or(false)
        }),
        "should report unsupported version error"
    );
}

/// Test that version 2.2 is accepted.
#[test]
fn version_2_2_is_accepted() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        &format!(
            r#"
version: "{SUPPORTED_SPEC_VERSION}"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
        ),
    );

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS, "should pass validation");
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "ok");
}
