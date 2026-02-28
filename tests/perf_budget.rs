use std::fs;
use std::path::Path;
use std::time::Instant;

use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, run};

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

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
                "version: \"2.2\"\nmodule: {module}\nboundaries:\n  path: {module_dir}/**/*\nconstraints: []\n"
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
        "--no-baseline",
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
