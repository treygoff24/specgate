use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use globset::GlobBuilder;
use serde::Deserialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;
use crate::spec::{
    Result, SpecConfig, SpecError,
    config::{DEFAULT_EXCLUDED_DIRS, include_dir_set},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspacePackage {
    pub module: String,
    pub relative_dir: String,
    pub module_path: String,
}

#[derive(Debug, Deserialize)]
struct PnpmWorkspaceConfig {
    #[serde(default)]
    packages: Vec<String>,
}

pub fn discover_workspace_packages_best_effort(project_root: &Path) -> Vec<WorkspacePackage> {
    discover_workspace_packages_strict(project_root, &SpecConfig::default()).unwrap_or_default()
}

pub fn discover_workspace_packages_with_config_best_effort(
    project_root: &Path,
    config: &SpecConfig,
) -> Vec<WorkspacePackage> {
    discover_workspace_packages_strict(project_root, config).unwrap_or_default()
}

pub fn discover_workspace_packages_strict(
    project_root: &Path,
    config: &SpecConfig,
) -> Result<Vec<WorkspacePackage>> {
    let include_dirs = include_dir_set(&config.include_dirs);
    let mut patterns = workspace_patterns(project_root)?;
    if patterns.is_empty() {
        return Ok(Vec::new());
    }

    patterns.sort();
    patterns.dedup();

    let mut candidate_dirs = BTreeSet::new();
    for pattern in patterns {
        candidate_dirs.extend(expand_workspace_pattern(project_root, &pattern)?);
    }

    let mut packages = Vec::new();
    let mut used_modules = BTreeSet::new();

    for relative_dir in candidate_dirs {
        if should_skip_workspace_dir(&relative_dir, &include_dirs) {
            continue;
        }

        let absolute_dir = project_root.join(&relative_dir);
        if !absolute_dir.is_dir() {
            continue;
        }

        if !absolute_dir.join("package.json").exists() && !absolute_dir.join("src").is_dir() {
            continue;
        }

        let base_module = relative_dir
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .unwrap_or("workspace")
            .to_string();

        let module = if used_modules.insert(base_module.clone()) {
            base_module
        } else {
            let disambiguated = relative_dir.replace('/', "__");
            used_modules.insert(disambiguated.clone());
            disambiguated
        };

        let module_path = if absolute_dir.join("src").is_dir() {
            format!("{relative_dir}/src/**/*")
        } else {
            format!("{relative_dir}/**/*")
        };

        packages.push(WorkspacePackage {
            module,
            relative_dir,
            module_path,
        });
    }

    packages.sort_by(|a, b| {
        a.relative_dir
            .cmp(&b.relative_dir)
            .then_with(|| a.module.cmp(&b.module))
    });
    Ok(packages)
}

fn workspace_patterns(project_root: &Path) -> Result<Vec<String>> {
    let mut patterns = Vec::new();
    patterns.extend(read_pnpm_workspace_patterns(project_root)?);
    patterns.extend(read_package_json_workspace_patterns(project_root)?);
    Ok(patterns)
}

fn read_pnpm_workspace_patterns(project_root: &Path) -> Result<Vec<String>> {
    let path = project_root.join("pnpm-workspace.yaml");
    let Ok(contents) = fs::read_to_string(path) else {
        return Ok(Vec::new());
    };

    let parsed = yaml_serde::from_str::<PnpmWorkspaceConfig>(&contents).map_err(|source| {
        SpecError::WorkspaceYamlParse {
            path: project_root.join("pnpm-workspace.yaml"),
            source,
        }
    })?;

    Ok(parsed
        .packages
        .into_iter()
        .map(|pattern| normalize_pattern(&pattern))
        .filter(|pattern| !pattern.is_empty())
        .collect())
}

fn read_package_json_workspace_patterns(project_root: &Path) -> Result<Vec<String>> {
    let path = project_root.join("package.json");
    let Ok(contents) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };

    let parsed = serde_json::from_str::<Value>(&contents).map_err(|source| {
        SpecError::WorkspaceJsonParse {
            path: path.clone(),
            source,
        }
    })?;

    let Some(workspaces) = parsed.get("workspaces") else {
        return Ok(Vec::new());
    };

    match workspaces {
        Value::Array(entries) => {
            let entries = entries
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .ok_or_else(|| SpecError::WorkspaceConfigInvalid {
                            path: path.clone(),
                            message: "workspaces array entries must be strings".to_string(),
                        })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(entries
                .into_iter()
                .map(normalize_pattern)
                .filter(|pattern| !pattern.is_empty())
                .collect())
        }
        Value::Object(map) => {
            let Some(entries) = map.get("packages") else {
                return Err(SpecError::WorkspaceConfigInvalid {
                    path: path.clone(),
                    message: "workspaces object must contain a 'packages' array".to_string(),
                });
            };
            let Some(entries) = entries.as_array() else {
                return Err(SpecError::WorkspaceConfigInvalid {
                    path: path.clone(),
                    message: "workspaces.packages must be an array".to_string(),
                });
            };

            let entries = entries
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .ok_or_else(|| SpecError::WorkspaceConfigInvalid {
                            path: path.clone(),
                            message: "workspaces.packages entries must be strings".to_string(),
                        })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(entries
                .into_iter()
                .map(normalize_pattern)
                .filter(|pattern| !pattern.is_empty())
                .collect())
        }
        _ => Err(SpecError::WorkspaceConfigInvalid {
            path,
            message: "workspaces must be an array or an object with a 'packages' array".to_string(),
        }),
    }
}

fn normalize_pattern(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .replace('\\', "/")
}

fn expand_workspace_pattern(project_root: &Path, pattern: &str) -> Result<Vec<String>> {
    let normalized_pattern = normalize_pattern(pattern);
    if normalized_pattern.is_empty() {
        return Ok(Vec::new());
    }

    if !normalized_pattern.contains('*') {
        let candidate = project_root.join(&normalized_pattern);
        return Ok(if candidate.is_dir() {
            vec![normalized_pattern]
        } else {
            Vec::new()
        });
    }

    let matcher = GlobBuilder::new(&normalized_pattern)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|source| SpecError::WorkspaceGlobInvalid {
            path: workspace_source_path(project_root, pattern),
            pattern: normalized_pattern.clone(),
            source,
        })?;

    let first_wildcard = normalized_pattern.find('*').unwrap_or(0);
    let prefix = normalized_pattern[..first_wildcard].trim_end_matches('/');
    let search_root = if prefix.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(prefix)
    };
    if !search_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut candidates = BTreeSet::new();
    let mut walker = WalkDir::new(&search_root).follow_links(false).min_depth(1);
    if !normalized_pattern.contains("**") {
        walker = walker.max_depth(1);
    }

    for entry in walker {
        let entry = entry.map_err(|source| SpecError::WorkspaceTraversal {
            path: search_root.clone(),
            source,
        })?;
        if !entry.file_type().is_dir() {
            continue;
        }

        let Ok(relative) = entry.path().strip_prefix(project_root) else {
            continue;
        };
        let relative_norm = normalize_path(relative);
        if matcher.is_match(&relative_norm) {
            candidates.insert(relative_norm);
        }
    }

    Ok(candidates.into_iter().collect())
}

fn workspace_source_path(project_root: &Path, pattern: &str) -> std::path::PathBuf {
    if project_root.join("pnpm-workspace.yaml").is_file() {
        return project_root.join("pnpm-workspace.yaml");
    }
    if project_root.join("package.json").is_file() {
        return project_root.join("package.json");
    }
    project_root.join(pattern)
}

pub(crate) fn excluded_dir_names() -> &'static [&'static str] {
    DEFAULT_EXCLUDED_DIRS
}

fn should_skip_workspace_dir(relative_dir: &str, include_dirs: &BTreeSet<String>) -> bool {
    relative_dir
        .split('/')
        .any(|segment| excluded_dir_names().contains(&segment) && !include_dirs.contains(segment))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::spec::SpecConfig;

    use super::*;

    fn write_file(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn discovers_workspace_packages_from_pnpm_workspace() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - packages/*\n  - extensions/*\n",
        );
        write_file(
            temp.path(),
            "packages/web/package.json",
            "{\"name\":\"web\"}\n",
        );
        write_file(
            temp.path(),
            "packages/web/src/index.ts",
            "export const web = 1;\n",
        );
        write_file(
            temp.path(),
            "extensions/shared/package.json",
            "{\"name\":\"shared\"}\n",
        );
        write_file(
            temp.path(),
            "extensions/shared/src/index.ts",
            "export const shared = 1;\n",
        );

        let packages = discover_workspace_packages_best_effort(temp.path());
        assert_eq!(packages.len(), 2);

        assert_eq!(packages[0].module, "shared");
        assert_eq!(packages[0].relative_dir, "extensions/shared");
        assert_eq!(packages[0].module_path, "extensions/shared/src/**/*");

        assert_eq!(packages[1].module, "web");
        assert_eq!(packages[1].relative_dir, "packages/web");
        assert_eq!(packages[1].module_path, "packages/web/src/**/*");
    }

    #[test]
    fn discovers_workspace_packages_from_package_json_fallback() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "package.json",
            "{\"name\":\"root\",\"workspaces\":{\"packages\":[\"packages/*\"]}}",
        );
        write_file(
            temp.path(),
            "packages/alpha/package.json",
            "{\"name\":\"alpha\"}\n",
        );
        write_file(
            temp.path(),
            "packages/alpha/src/index.ts",
            "export const alpha = 1;\n",
        );

        let packages = discover_workspace_packages_best_effort(temp.path());
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].module, "alpha");
        assert_eq!(packages[0].relative_dir, "packages/alpha");
        assert_eq!(packages[0].module_path, "packages/alpha/src/**/*");
    }

    #[test]
    fn workspace_discovery_skips_vendor_dirs_by_default() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - vendor/*\n",
        );
        write_file(
            temp.path(),
            "vendor/lib/package.json",
            "{\"name\":\"lib\"}\n",
        );
        write_file(
            temp.path(),
            "vendor/lib/src/index.ts",
            "export const lib = 1;\n",
        );

        let packages = discover_workspace_packages_best_effort(temp.path());
        assert!(packages.is_empty(), "vendor should be excluded by default");
    }

    #[test]
    fn include_dirs_reincludes_default_excluded_vendor_workspace() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - vendor/*\n",
        );
        write_file(
            temp.path(),
            "vendor/lib/package.json",
            "{\"name\":\"lib\"}\n",
        );
        write_file(
            temp.path(),
            "vendor/lib/src/index.ts",
            "export const lib = 1;\n",
        );

        let config = SpecConfig {
            include_dirs: vec!["vendor".to_string()],
            ..SpecConfig::default()
        };

        let packages = discover_workspace_packages_with_config_best_effort(temp.path(), &config);
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].relative_dir, "vendor/lib");
    }

    #[test]
    fn include_dirs_noop_for_non_excluded_dirs() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - packages/*\n",
        );
        write_file(
            temp.path(),
            "packages/app/package.json",
            "{\"name\":\"app\"}\n",
        );
        write_file(
            temp.path(),
            "packages/app/src/index.ts",
            "export const app = 1;\n",
        );

        let default_packages = discover_workspace_packages_best_effort(temp.path());
        let include_packages = discover_workspace_packages_with_config_best_effort(
            temp.path(),
            &SpecConfig {
                include_dirs: vec!["packages".to_string()],
                ..SpecConfig::default()
            },
        );

        assert_eq!(include_packages, default_packages);
        assert_eq!(include_packages.len(), 1);
    }

    #[test]
    fn checked_discovery_rejects_malformed_package_json_workspaces() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "package.json",
            "{\"name\":\"root\",\"workspaces\":17}",
        );

        let error =
            discover_workspace_packages_strict(temp.path(), &SpecConfig::default())
                .expect_err("workspace discovery should fail");

        assert!(
            error.to_string().contains("workspaces"),
            "error should mention workspaces: {error}"
        );
    }

    #[test]
    fn best_effort_discovery_drops_workspace_config_errors() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "package.json",
            "{\"name\":\"root\",\"workspaces\":17}",
        );

        let packages = discover_workspace_packages_best_effort(temp.path());

        assert!(packages.is_empty(), "best-effort discovery should be lossy");
    }

    #[test]
    fn checked_discovery_rejects_invalid_workspace_glob() {
        let temp = TempDir::new().expect("tempdir");
        let error = expand_workspace_pattern(temp.path(), "[abc*").expect_err("invalid glob");

        assert!(
            error.to_string().contains("invalid workspace glob"),
            "error should mention invalid workspace glob: {error}"
        );
    }

    #[test]
    fn checked_discovery_allows_valid_but_empty_workspace_configs() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "package.json",
            "{\"workspaces\":[\"packages/*\"]}",
        );

        let packages =
            discover_workspace_packages_strict(temp.path(), &SpecConfig::default())
                .expect("workspace discovery should succeed");

        assert!(
            packages.is_empty(),
            "unmatched workspace globs should be allowed"
        );
    }
}
