//! Git blast-radius analysis for incremental checking.
//!
//! This module provides utilities for determining which modules are affected
//! by changes since a given git reference. This enables faster CI checks by
//! only validating the "blast radius" of changed code.
//!
//! ## Contract: `--since` Blast-Radius Mode
//!
//! When `--since <git-ref>` is provided:
//! 1. Resolve git diff from `<git-ref>` to HEAD
//! 2. Identify modules containing changed files
//! 3. Compute transitive importers of affected modules
//! 4. Only include violations from files in the blast radius
//!
//! The blast radius includes:
//! - Files in modules that have changed
//! - Files in modules that import from changed modules (transitively)

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// Result of git blast-radius analysis.
#[derive(Debug, Clone, Default)]
pub struct BlastRadius {
    /// Files that have changed since the reference (repo-relative paths).
    pub changed_files: BTreeSet<String>,
    /// Modules directly affected by changed files.
    pub affected_modules: BTreeSet<String>,
    /// All modules affected including transitive importers.
    pub affected_with_importers: BTreeSet<String>,
    /// Error message if git operations failed.
    pub error: Option<String>,
}

impl BlastRadius {
    /// Returns true if blast radius analysis succeeded.
    pub fn is_valid(&self) -> bool {
        self.error.is_none()
    }

    /// Returns true if a file path is in the blast radius.
    pub fn contains_file(&self, file: &str, module: Option<&str>) -> bool {
        // Check if file is directly changed
        if self.changed_files.contains(file) {
            return true;
        }

        // Check if the file's module is in the affected set
        if let Some(m) = module {
            self.affected_with_importers.contains(m)
        } else {
            false
        }
    }
}

/// Compute the blast radius from a git reference.
///
/// This determines:
/// 1. Which files have changed since the reference
/// 2. Which modules contain those changed files
/// 3. Which modules import from the affected modules (transitively)
///
/// # Arguments
///
/// * `project_root` - Path to the git repository root
/// * `since_ref` - Git reference (e.g., "HEAD~1", "main", "abc123")
/// * `module_to_files` - Map from module ID to set of file paths
/// * `file_to_module` - Map from file path to module ID
/// * `importer_graph` - Map from module ID to set of modules that import from it
pub fn compute_blast_radius(
    project_root: &Path,
    since_ref: &str,
    module_to_files: &BTreeMap<String, BTreeSet<String>>,
    file_to_module: &BTreeMap<String, String>,
    importer_graph: &BTreeMap<String, BTreeSet<String>>,
) -> BlastRadius {
    // Get changed files from git
    let changed_files = match get_changed_files(project_root, since_ref) {
        Ok(files) => files,
        Err(error) => {
            return BlastRadius {
                changed_files: BTreeSet::new(),
                affected_modules: BTreeSet::new(),
                affected_with_importers: BTreeSet::new(),
                error: Some(error),
            };
        }
    };

    // Find directly affected modules
    let mut affected_modules: BTreeSet<String> = BTreeSet::new();
    for file in &changed_files {
        if let Some(module) = file_to_module.get(file) {
            affected_modules.insert(module.clone());
        }
    }

    // Also include modules whose spec files changed
    for (module, files) in module_to_files {
        for file in files {
            if file.ends_with(".spec.yml") && changed_files.contains(file) {
                affected_modules.insert(module.clone());
            }
        }
    }

    // Compute transitive importers
    let affected_with_importers = compute_transitive_importers(&affected_modules, importer_graph);

    BlastRadius {
        changed_files,
        affected_modules,
        affected_with_importers,
        error: None,
    }
}

/// Get files changed since a git reference.
fn get_changed_files(project_root: &Path, since_ref: &str) -> Result<BTreeSet<String>, String> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=ACMRT", since_ref])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("failed to execute git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: BTreeSet<String> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .collect();

    Ok(files)
}

/// Compute all modules that transitively import from the affected modules.
fn compute_transitive_importers(
    seed_modules: &BTreeSet<String>,
    importer_graph: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut result = seed_modules.clone();
    let mut queue: Vec<String> = seed_modules.iter().cloned().collect();

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

    result
}

/// Validate that a git reference exists.
pub fn validate_git_ref(project_root: &Path, git_ref: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", git_ref])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("failed to execute git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "invalid git reference '{}': {}",
            git_ref,
            stderr.trim()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blast_radius_default_is_empty() {
        let radius = BlastRadius::default();
        assert!(radius.changed_files.is_empty());
        assert!(radius.affected_modules.is_empty());
        assert!(radius.affected_with_importers.is_empty());
        assert!(radius.error.is_none());
    }

    #[test]
    fn blast_radius_is_valid_when_no_error() {
        let radius = BlastRadius::default();
        assert!(radius.is_valid());

        let radius_with_error = BlastRadius {
            error: Some("test error".to_string()),
            ..Default::default()
        };
        assert!(!radius_with_error.is_valid());
    }

    #[test]
    fn blast_radius_contains_file_checks_module() {
        let mut radius = BlastRadius::default();
        radius.affected_with_importers.insert("app".to_string());

        assert!(radius.contains_file("src/app/foo.ts", Some("app")));
        assert!(!radius.contains_file("src/core/bar.ts", Some("core")));
    }

    #[test]
    fn blast_radius_contains_file_checks_changed_files() {
        let mut radius = BlastRadius::default();
        radius.changed_files.insert("src/app/foo.ts".to_string());

        assert!(radius.contains_file("src/app/foo.ts", None));
        assert!(!radius.contains_file("src/app/bar.ts", None));
    }

    #[test]
    fn compute_transitive_importers_handles_chain() {
        // a -> b -> c (c is affected, b imports c, a imports b)
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

        let seed: BTreeSet<String> = vec!["c".to_string()].into_iter().collect();
        let result = compute_transitive_importers(&seed, &importer_graph);

        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("c"));
    }

    #[test]
    fn compute_transitive_importers_handles_multiple_importers() {
        // a -> c, b -> c (c is affected)
        let mut importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        importer_graph.insert("c".to_string(), {
            let mut set = BTreeSet::new();
            set.insert("a".to_string());
            set.insert("b".to_string());
            set
        });

        let seed: BTreeSet<String> = vec!["c".to_string()].into_iter().collect();
        let result = compute_transitive_importers(&seed, &importer_graph);

        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert!(result.contains("c"));
    }

    #[test]
    fn compute_transitive_importers_handles_no_importers() {
        let importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        let seed: BTreeSet<String> = vec!["isolated".to_string()].into_iter().collect();
        let result = compute_transitive_importers(&seed, &importer_graph);

        assert_eq!(result.len(), 1);
        assert!(result.contains("isolated"));
    }

    #[test]
    fn compute_transitive_importers_handles_cycles() {
        // a -> b -> a (cyclic, should still terminate)
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

        let seed: BTreeSet<String> = vec!["a".to_string()].into_iter().collect();
        let result = compute_transitive_importers(&seed, &importer_graph);

        assert!(result.contains("a"));
        assert!(result.contains("b"));
        assert_eq!(result.len(), 2);
    }
}
