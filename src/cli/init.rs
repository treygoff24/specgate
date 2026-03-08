//! Init command support.

use clap::Args;

use super::*;
use crate::spec::types::CURRENT_SPEC_VERSION;

#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Directory where starter `.spec.yml` files are written.
    #[arg(long, default_value = "modules")]
    spec_dir: PathBuf,
    /// Optional starter module id override.
    #[arg(long)]
    module: Option<String>,
    /// Optional starter module boundary glob override.
    #[arg(long)]
    module_path: Option<String>,
    /// Overwrite existing scaffold files.
    #[arg(long)]
    force: bool,
}

pub(super) fn handle_init(args: InitArgs) -> CliRunResult {
    let project_root = fs::canonicalize(&args.common.project_root)
        .unwrap_or_else(|_| args.common.project_root.clone());
    let module_override = args.module.as_ref().map(|module| module.trim().to_string());
    if module_override
        .as_ref()
        .is_some_and(|module| module.is_empty())
    {
        return runtime_error_json("init", "module must be non-empty", Vec::new());
    }
    let module_path_override = args
        .module_path
        .as_ref()
        .map(|module_path| module_path.trim().to_string())
        .filter(|module_path| !module_path.is_empty());

    if let Err(error) = fs::create_dir_all(&project_root) {
        return runtime_error_json(
            "init",
            "failed to prepare project root",
            vec![format!("{}: {error}", project_root.display())],
        );
    }

    let normalized_spec_dir = normalize_path(&args.spec_dir);
    let config_path = project_root.join("specgate.config.yml");
    let scaffold_specs = infer_init_scaffold_specs(
        &project_root,
        module_override.as_deref(),
        module_path_override.as_deref(),
    );

    let config_content = format!(
        "spec_dirs:\n  - \"{}\"\nexclude: []\ntest_patterns: []\n",
        escape_yaml_double_quoted(&normalized_spec_dir)
    );

    let mut created = Vec::new();
    let mut skipped_existing = Vec::new();

    if let Err(error) = write_scaffold_file(
        &project_root,
        &config_path,
        &config_content,
        args.force,
        &mut created,
        &mut skipped_existing,
    ) {
        return runtime_error_json("init", "failed to write scaffold", vec![error]);
    }

    for scaffold in scaffold_specs {
        let spec_file_stem = scaffold.module.replace('/', "__");
        let spec_path = resolve_against_root(
            &project_root,
            &args.spec_dir.join(format!("{spec_file_stem}.spec.yml")),
        );
        let spec_content = format!(
            "version: \"{}\"\nmodule: \"{}\"\nboundaries:\n  path: \"{}\"\n  contracts: []\nconstraints: []\n",
            CURRENT_SPEC_VERSION,
            escape_yaml_double_quoted(&scaffold.module),
            escape_yaml_double_quoted(&scaffold.path)
        );

        if let Err(error) = write_scaffold_file(
            &project_root,
            &spec_path,
            &spec_content,
            args.force,
            &mut created,
            &mut skipped_existing,
        ) {
            return runtime_error_json("init", "failed to write scaffold", vec![error]);
        }
    }

    CliRunResult::json(
        EXIT_CODE_PASS,
        &InitOutput {
            schema_version: "2.2".to_string(),
            status: "ok".to_string(),
            project_root: normalize_path(&project_root),
            created,
            skipped_existing,
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InitScaffoldSpec {
    module: String,
    path: String,
}

pub(crate) const INIT_COMMON_ROOT_MODULE_DIRS: [&str; 11] = [
    "lib",
    "routes",
    "ws",
    "api",
    "app",
    "services",
    "controllers",
    "middleware",
    "handlers",
    "utils",
    "helpers",
];

pub(crate) fn infer_init_scaffold_specs(
    project_root: &Path,
    module_override: Option<&str>,
    module_path_override: Option<&str>,
) -> Vec<InitScaffoldSpec> {
    if module_override.is_some() || module_path_override.is_some() {
        return vec![InitScaffoldSpec {
            module: module_override.unwrap_or("app").to_string(),
            path: module_path_override
                .map(ToString::to_string)
                .unwrap_or_else(|| infer_single_module_path(project_root)),
        }];
    }

    let workspace_packages = spec::workspace_discovery::discover_workspace_packages(project_root);
    if !workspace_packages.is_empty() {
        let mut scaffolds = Vec::new();
        if let Some(root_path) = infer_root_module_path(project_root) {
            scaffolds.push(InitScaffoldSpec {
                module: "root".to_string(),
                path: root_path,
            });
        }

        scaffolds.extend(
            workspace_packages
                .into_iter()
                .map(|workspace| InitScaffoldSpec {
                    module: workspace.module,
                    path: workspace.module_path,
                }),
        );

        if !scaffolds.is_empty() {
            return scaffolds;
        }
    }

    if project_root.join("src").join("app").is_dir() {
        return vec![InitScaffoldSpec {
            module: "app".to_string(),
            path: "src/app/**/*".to_string(),
        }];
    }

    if project_root.join("src").is_dir() {
        return vec![InitScaffoldSpec {
            module: "app".to_string(),
            path: "src/**/*".to_string(),
        }];
    }

    let matched_dirs = INIT_COMMON_ROOT_MODULE_DIRS
        .iter()
        .copied()
        .filter(|dir| project_root.join(dir).is_dir())
        .collect::<Vec<_>>();

    if !matched_dirs.is_empty() {
        return matched_dirs
            .into_iter()
            .map(|dir| InitScaffoldSpec {
                module: dir.to_string(),
                path: format!("{dir}/**/*"),
            })
            .collect();
    }

    vec![InitScaffoldSpec {
        module: "app".to_string(),
        path: "src/app/**/*".to_string(),
    }]
}

pub(crate) fn infer_single_module_path(project_root: &Path) -> String {
    infer_root_module_path(project_root).unwrap_or_else(|| "src/app/**/*".to_string())
}

pub(crate) fn infer_root_module_path(project_root: &Path) -> Option<String> {
    if project_root.join("src").join("app").is_dir() {
        return Some("src/app/**/*".to_string());
    }

    if project_root.join("src").is_dir() {
        return Some("src/**/*".to_string());
    }

    INIT_COMMON_ROOT_MODULE_DIRS
        .iter()
        .copied()
        .find(|dir| project_root.join(dir).is_dir())
        .map(|dir| format!("{dir}/**/*"))
}

pub(crate) fn write_scaffold_file(
    project_root: &Path,
    path: &Path,
    content: &str,
    force: bool,
    created: &mut Vec<String>,
    skipped_existing: &mut Vec<String>,
) -> std::result::Result<(), String> {
    let relative = normalize_repo_relative(project_root, path);

    if path.exists() && !force {
        skipped_existing.push(relative);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create directory {}: {error}", parent.display()))?;
    }

    fs::write(path, content)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;

    created.push(relative);
    Ok(())
}

pub(crate) fn escape_yaml_double_quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                escaped.push_str(&format!("\\u{:04X}", control as u32));
            }
            _ => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn init_args_defaults_are_reasonable() {
        let args = InitArgs {
            common: CommonProjectArgs {
                project_root: PathBuf::from("."),
            },
            spec_dir: PathBuf::from("modules"),
            module: None,
            module_path: None,
            force: false,
        };

        assert_eq!(args.spec_dir, PathBuf::from("modules"));
        assert_eq!(args.module, None);
        assert_eq!(args.module_path, None);
        assert!(!args.force);
    }

    #[test]
    fn escape_yaml_double_quoted_escapes_control_chars() {
        let escaped = escape_yaml_double_quoted("line1\nline2\r\t\"\\");
        assert_eq!(escaped, "line1\\nline2\\r\\t\\\"\\\\");
    }

    #[test]
    fn init_quotes_spec_dir_with_special_chars() {
        let temp = TempDir::new().expect("tempdir");
        let spec_dir = "modules:#prod";

        let result = run([
            "specgate",
            "init",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--spec-dir",
            spec_dir,
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let config = fs::read_to_string(temp.path().join("specgate.config.yml"))
            .expect("scaffold config exists");
        assert!(config.contains("spec_dirs:\n  - \"modules:#prod\"\n"));
        assert!(temp.path().join("modules:#prod/app.spec.yml").exists());
    }
}
