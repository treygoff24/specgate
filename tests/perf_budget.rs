//! Performance Budget Tests
//!
//! Tests for CLI performance under various project sizes and constraint
//! configurations.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn fixture_root(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

fn copy_dir_recursive(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from).into_iter() {
        let entry = entry.expect("walkdir");
        let source_path = entry.path();
        let relative_path = source_path
            .strip_prefix(from)
            .expect("source inside fixture");
        let target_path = to.join(relative_path);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target_path).expect("create directory");
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).expect("create parent directory");
            }
            fs::copy(source_path, target_path).expect("copy fixture file");
        }
    }
}

/// Build a clean perf fixture with `module_count` modules and `files_per_module` files each.
/// All inter-module imports are allowed (no policy violations).
fn build_tier1_perf_fixture(root: &Path, module_count: usize, files_per_module: usize) {
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    for idx in 0..module_count {
        let module = format!("m{idx:03}");
        let module_dir = format!("src/{module}");
        write_file(
            root,
            &format!("modules/{module}.spec.yml"),
            &format!(
                "version: \"2.2\"\nmodule: {module}\nboundaries:\n  path: {module_dir}/**/*\n"
            ),
        );

        for file_idx in 0..files_per_module {
            let next_module = format!("m{:03}", (idx + 1) % module_count);
            write_file(
                root,
                &format!("{module_dir}/f{file_idx}.ts"),
                &format!(
                    "import {{ v as nextV }} from '../{next_module}/f0';\nexport const v = nextV + {idx};\n"
                ),
            );
        }
    }
}

/// Build a small fixture that includes a policy violation (forbidden cross-module import).
/// Used to exercise the policy evaluation path in perf tests.
fn build_violation_perf_fixture(root: &Path, module_count: usize, files_per_module: usize) {
    build_tier1_perf_fixture(root, module_count, files_per_module);

    // Add a module with a never_imports constraint and a file that violates it.
    // m000 is guaranteed to exist from build_tier1_perf_fixture when module_count >= 1.
    write_file(
        root,
        "modules/restricted.spec.yml",
        "version: \"2.2\"\nmodule: restricted\nboundaries:\n  path: src/restricted/**/*\n  never_imports:\n    - m000\nconstraints:\n  - rule: boundary.never_imports\n    severity: error\n",
    );
    write_file(
        root,
        "src/restricted/index.ts",
        "import { v } from '../m000/f0';\nexport const result = v;\n",
    );
}

#[test]
fn tier1_perf_budget_check_mode() {
    let temp = TempDir::new().expect("tempdir");
    let module_count = std::env::var("SPECGATE_PERF_MODULES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(120);
    let files_per_module = std::env::var("SPECGATE_PERF_FILES_PER_MODULE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4);
    let budget_ms = std::env::var("SPECGATE_PERF_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse::<u128>().ok())
        .unwrap_or(7_000);

    build_tier1_perf_fixture(temp.path(), module_count, files_per_module);

    let start = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    let elapsed_ms = start.elapsed().as_millis();

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "specgate check should pass"
    );
    assert!(
        elapsed_ms <= budget_ms,
        "perf budget exceeded: elapsed={elapsed_ms}ms budget={budget_ms}ms modules={module_count} files/module={files_per_module}"
    );
}

/// Verifies that an impossibly low perf budget correctly triggers the
/// budget-exceeded assertion. Uses a 1ms budget which no real run can meet.
#[test]
#[should_panic(expected = "perf budget exceeded")]
fn tier1_perf_budget_exceeded() {
    let temp = TempDir::new().expect("tempdir");
    build_tier1_perf_fixture(temp.path(), 10, 2);

    let budget_ms: u128 = 1;
    let start = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    let elapsed_ms = start.elapsed().as_millis();

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "specgate check should pass"
    );
    assert!(
        elapsed_ms <= budget_ms,
        "perf budget exceeded: elapsed={elapsed_ms}ms budget={budget_ms}ms"
    );
}

/// Build a monorepo fixture with two packages, each having their own tsconfig.
/// This exercises the multi-tsconfig resolution path and the per-tsconfig resolver pool.
fn build_monorepo_multi_tsconfig_fixture(root: &Path, files_per_package: usize) {
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    for pkg in &["alpha", "beta"] {
        // Minimal tsconfig per package
        write_file(root, &format!("packages/{pkg}/tsconfig.json"), "{}\n");

        write_file(
            root,
            &format!("modules/{pkg}.spec.yml"),
            &format!(
                "version: \"2.2\"\nmodule: {pkg}\nboundaries:\n  path: packages/{pkg}/src/**/*\n"
            ),
        );

        for i in 0..files_per_package {
            write_file(
                root,
                &format!("packages/{pkg}/src/f{i}.ts"),
                &format!("export const v{i} = {i};\n"),
            );
        }
    }
}

/// Verifies that a project with multiple tsconfigs (monorepo) completes within a generous
/// time budget. This is a regression guard for the multi-tsconfig resolver pool path.
#[test]
fn monorepo_tsconfig_resolution_within_budget() {
    let temp = TempDir::new().expect("tempdir");
    let budget_ms: u128 = 5_000;

    build_monorepo_multi_tsconfig_fixture(temp.path(), 20);

    let start = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    let elapsed_ms = start.elapsed().as_millis();

    assert!(
        result.exit_code == EXIT_CODE_PASS || result.exit_code == EXIT_CODE_POLICY_VIOLATIONS,
        "specgate check should not error out (got exit code {})",
        result.exit_code
    );
    assert!(
        elapsed_ms <= budget_ms,
        "perf budget exceeded for monorepo multi-tsconfig: elapsed={elapsed_ms}ms budget={budget_ms}ms"
    );
}

#[test]
fn openclaw_scale_fixture_within_budget() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let budget_ms: u128 = std::env::var("SPECGATE_OPENCLAW_PERF_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);

    let start = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    let elapsed_ms = start.elapsed().as_millis();

    assert!(
        result.exit_code == EXIT_CODE_PASS || result.exit_code == EXIT_CODE_POLICY_VIOLATIONS,
        "check should not error (got exit {}); stderr={}",
        result.exit_code,
        result.stderr
    );
    assert!(
        elapsed_ms <= budget_ms,
        "openclaw-scale budget exceeded: elapsed={elapsed_ms}ms budget={budget_ms}ms"
    );
}

/// Verifies single-root-tsconfig performance has not regressed by running check 3 times
/// and asserting the mean is within a generous budget. This is a regression guard, not a
/// precise benchmark.
#[test]
fn single_root_tsconfig_perf_not_regressed() {
    let temp = TempDir::new().expect("tempdir");
    let budget_ms: u128 = 2_000;
    let runs = 3usize;

    // Use the tier1 fixture with a small project to keep CI fast
    build_tier1_perf_fixture(temp.path(), 20, 3);

    let total_ms: u128 = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let _ = run([
                "specgate",
                "check",
                "--project-root",
                temp.path().to_str().expect("utf8"),
            ]);
            start.elapsed().as_millis()
        })
        .sum();
    let mean_ms = total_ms / runs as u128;

    assert!(
        mean_ms <= budget_ms,
        "single-root tsconfig perf regressed: mean={mean_ms}ms budget={budget_ms}ms over {runs} runs"
    );
}

/// Exercises the policy evaluation path (violations present) to ensure perf holds
/// even when specgate must evaluate constraints and classify violations.
#[test]
fn tier1_perf_budget_policy_violation_path() {
    let temp = TempDir::new().expect("tempdir");
    let budget_ms = std::env::var("SPECGATE_PERF_BUDGET_MS")
        .ok()
        .and_then(|v| v.parse::<u128>().ok())
        .unwrap_or(7_000);

    build_violation_perf_fixture(temp.path(), 10, 2);

    // Run without baseline so the violation causes a policy failure (exit code 1).
    let start = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    let elapsed_ms = start.elapsed().as_millis();

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "specgate check should fail due to policy violation (restricted module imports m000)"
    );
    assert!(
        elapsed_ms <= budget_ms,
        "perf budget exceeded: elapsed={elapsed_ms}ms budget={budget_ms}ms"
    );
}
