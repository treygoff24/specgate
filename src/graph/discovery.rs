use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use miette::Diagnostic;
use thiserror::Error;
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

/// Discover all TypeScript/JavaScript source files in deterministic order.
///
/// Included extensions: `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs`.
/// Excluded paths are matched against repo-relative normalized paths.
pub fn discover_source_files(
    project_root: &Path,
    exclude_patterns: &[String],
) -> Result<Vec<PathBuf>> {
    let exclude_matcher = build_globset(exclude_patterns)?;

    let mut files = BTreeSet::new();

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
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

    Ok(files.into_iter().collect())
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

        let files =
            discover_source_files(temp.path(), &["**/ignored/**".to_string()]).expect("discover");

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
    }
}
