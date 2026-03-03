use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use globset::GlobBuilder;
use serde::Deserialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;

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

pub fn discover_workspace_packages(project_root: &Path) -> Vec<WorkspacePackage> {
    let mut patterns = workspace_patterns(project_root);
    if patterns.is_empty() {
        return Vec::new();
    }

    patterns.sort();
    patterns.dedup();

    let mut candidate_dirs = BTreeSet::new();
    for pattern in patterns {
        candidate_dirs.extend(expand_workspace_pattern(project_root, &pattern));
    }

    let mut packages = Vec::new();
    let mut used_modules = BTreeSet::new();

    for relative_dir in candidate_dirs {
        if should_skip_workspace_dir(&relative_dir) {
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
    packages
}

fn workspace_patterns(project_root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();
    patterns.extend(read_pnpm_workspace_patterns(project_root));
    patterns.extend(read_package_json_workspace_patterns(project_root));
    patterns
}

fn read_pnpm_workspace_patterns(project_root: &Path) -> Vec<String> {
    let path = project_root.join("pnpm-workspace.yaml");
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let Ok(parsed) = yaml_serde::from_str::<PnpmWorkspaceConfig>(&contents) else {
        return Vec::new();
    };

    parsed
        .packages
        .into_iter()
        .map(|pattern| normalize_pattern(&pattern))
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

fn read_package_json_workspace_patterns(project_root: &Path) -> Vec<String> {
    let path = project_root.join("package.json");
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let Ok(parsed) = serde_json::from_str::<Value>(&contents) else {
        return Vec::new();
    };

    let Some(workspaces) = parsed.get("workspaces") else {
        return Vec::new();
    };

    match workspaces {
        Value::Array(entries) => entries
            .iter()
            .filter_map(Value::as_str)
            .map(normalize_pattern)
            .filter(|pattern| !pattern.is_empty())
            .collect(),
        Value::Object(map) => map
            .get("packages")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(normalize_pattern)
                    .filter(|pattern| !pattern.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn normalize_pattern(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .replace('\\', "/")
}

fn expand_workspace_pattern(project_root: &Path, pattern: &str) -> Vec<String> {
    let normalized_pattern = normalize_pattern(pattern);
    if normalized_pattern.is_empty() {
        return Vec::new();
    }

    if !normalized_pattern.contains('*') {
        let candidate = project_root.join(&normalized_pattern);
        return if candidate.is_dir() {
            vec![normalized_pattern]
        } else {
            Vec::new()
        };
    }

    let Ok(matcher) = GlobBuilder::new(&normalized_pattern)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
    else {
        return Vec::new();
    };

    let first_wildcard = normalized_pattern.find('*').unwrap_or(0);
    let prefix = normalized_pattern[..first_wildcard].trim_end_matches('/');
    let search_root = if prefix.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(prefix)
    };
    if !search_root.is_dir() {
        return Vec::new();
    }

    let mut candidates = BTreeSet::new();
    let mut walker = WalkDir::new(&search_root).follow_links(false).min_depth(1);
    if !normalized_pattern.contains("**") {
        walker = walker.max_depth(1);
    }

    for entry in walker.into_iter().filter_map(std::result::Result::ok) {
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

    candidates.into_iter().collect()
}

fn should_skip_workspace_dir(relative_dir: &str) -> bool {
    relative_dir.split('/').any(|segment| {
        matches!(
            segment,
            "node_modules" | "dist" | "build" | ".git" | "generated" | "target" | "coverage"
        )
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

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

        let packages = discover_workspace_packages(temp.path());
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

        let packages = discover_workspace_packages(temp.path());
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].module, "alpha");
        assert_eq!(packages[0].relative_dir, "packages/alpha");
        assert_eq!(packages[0].module_path, "packages/alpha/src/**/*");
    }
}
