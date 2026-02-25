use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use miette::Diagnostic;
use thiserror::Error;
use walkdir::Error as WalkDirError;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;

#[derive(Debug, Error, Diagnostic)]
pub enum DiscoveryError {
    #[error("invalid glob pattern '{pattern}': {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

pub type Result<T> = std::result::Result<T, DiscoveryError>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscoveryWarning {
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<DiscoveryWarning>,
}

/// Discover all TypeScript/JavaScript source files in deterministic order.
///
/// Included extensions: `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs`.
/// Excluded paths are matched against repo-relative normalized paths.
pub fn discover_source_files(
    project_root: &Path,
    exclude_patterns: &[String],
) -> Result<DiscoveryResult> {
    let exclude_matcher = build_globset(exclude_patterns)?;

    let mut files = BTreeSet::new();
    let mut warnings = Vec::new();

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warnings.push(warning_from_walkdir_error(project_root, &error));
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_source_file(path) {
            continue;
        }

        let normalized_relative = normalize_relative(project_root, path);
        if exclude_matcher.is_match(&normalized_relative) {
            continue;
        }

        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        files.insert(canonical);
    }

    warnings.sort();

    Ok(DiscoveryResult {
        files: files.into_iter().collect(),
        warnings,
    })
}

fn warning_from_walkdir_error(project_root: &Path, error: &WalkDirError) -> DiscoveryWarning {
    let path = error.path().map(Path::to_path_buf);
    let path = path.map(|path| {
        path.strip_prefix(project_root)
            .map(Path::to_path_buf)
            .unwrap_or(path)
    });

    DiscoveryWarning {
        path,
        message: error.to_string(),
    }
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|source| DiscoveryError::InvalidGlob {
            pattern: pattern.clone(),
            source,
        })?;
        builder.add(glob);
    }

    builder
        .build()
        .map_err(|source| DiscoveryError::InvalidGlob {
            pattern: "<globset>".to_string(),
            source,
        })
}

fn normalize_relative(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(relative) => normalize_path(relative),
        Err(_) => normalize_path(path),
    }
}

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs")
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn discover_source_files_is_sorted_and_respects_excludes() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/ignored")).expect("mkdir");
        fs::create_dir_all(temp.path().join("src/nested")).expect("mkdir");

        fs::write(temp.path().join("src/z.ts"), "export {}\n").expect("write z");
        fs::write(temp.path().join("src/a.tsx"), "export {}\n").expect("write a");
        fs::write(temp.path().join("src/nested/c.jsx"), "export {}\n").expect("write c");
        fs::write(temp.path().join("src/ignored/skip.ts"), "export {}\n").expect("write skip");
        fs::write(temp.path().join("README.md"), "not a source file\n").expect("write readme");

        let result =
            discover_source_files(temp.path(), &["**/ignored/**".to_string()]).expect("discover");
        let files = result.files;

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");

        let rel: Vec<_> = files
            .iter()
            .map(|path| {
                normalize_path(
                    path.strip_prefix(&canonical_root)
                        .expect("path should be under temp root"),
                )
            })
            .collect();

        assert_eq!(
            rel,
            vec![
                "src/a.tsx".to_string(),
                "src/nested/c.jsx".to_string(),
                "src/z.ts".to_string(),
            ]
        );

        assert!(result.warnings.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn discover_source_files_reports_symlink_loop_as_warning() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/loop")).expect("mkdir");
        fs::write(temp.path().join("src/main.ts"), "export {}\n").expect("write main");

        symlink(temp.path().join("src"), temp.path().join("src/loop/back")).expect("symlink");

        let result = discover_source_files(temp.path(), &[]).expect("discover");

        assert_eq!(result.files.len(), 1);
        assert!(result.warnings.iter().any(|warning| {
            warning
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().contains("src/loop/back"))
                .unwrap_or(false)
        }));
    }
}
