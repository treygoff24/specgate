use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use miette::Diagnostic;
use thiserror::Error;
use walkdir::Error as WalkDirError;
use walkdir::WalkDir;

use crate::spec::config::{
    build_exclude_matcher, include_dir_set, path_matches_exclude, should_skip_default_dir,
};

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
    discover_source_files_with_options(project_root, exclude_patterns, &[])
}

/// Discover all TypeScript/JavaScript source files with directory-pruning options.
///
/// In addition to explicit exclude globs, this always prunes Specgate's default
/// excluded directories (for example nested `node_modules`) unless they are
/// re-included through `include_dirs`.
pub fn discover_source_files_with_options(
    project_root: &Path,
    exclude_patterns: &[String],
    include_dirs: &[String],
) -> Result<DiscoveryResult> {
    let exclude_matcher =
        build_exclude_matcher(exclude_patterns).map_err(|source| DiscoveryError::InvalidGlob {
            pattern: "<globset>".to_string(),
            source,
        })?;
    let include_dirs = include_dir_set(include_dirs);

    let mut files = BTreeSet::new();
    let mut warnings = Vec::new();

    for entry in WalkDir::new(project_root)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            !should_skip_entry(
                project_root,
                entry.path(),
                entry.file_type().is_dir(),
                &include_dirs,
                &exclude_matcher,
            )
        })
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

        if path_matches_exclude(project_root, path, false, &exclude_matcher) {
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

fn should_skip_entry(
    project_root: &Path,
    path: &Path,
    is_dir: bool,
    include_dirs: &BTreeSet<String>,
    exclude_matcher: &GlobSet,
) -> bool {
    if should_skip_default_dir(project_root, path, include_dirs) {
        return true;
    }

    if !is_dir {
        return false;
    }

    path_matches_exclude(project_root, path, true, exclude_matcher)
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

    use crate::deterministic::normalize_path;

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

    #[test]
    fn discover_source_files_prunes_nested_default_excluded_dirs() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("apps/web/node_modules/pkg")).expect("mkdir nested");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");

        fs::write(temp.path().join("src/main.ts"), "export const main = 1;\n").expect("write");
        fs::write(
            temp.path().join("apps/web/node_modules/pkg/index.js"),
            "module.exports = {};\n",
        )
        .expect("write nested dep");

        let result =
            discover_source_files_with_options(temp.path(), &["node_modules/**".to_string()], &[])
                .expect("discover");

        let canonical_root = fs::canonicalize(temp.path()).expect("canonical root");
        let rel: Vec<_> = result
            .files
            .iter()
            .map(|path| {
                normalize_path(
                    path.strip_prefix(&canonical_root)
                        .expect("path should be under temp root"),
                )
            })
            .collect();

        assert_eq!(rel, vec!["src/main.ts".to_string()]);
    }

    #[test]
    fn discover_source_files_can_reinclude_default_excluded_dirs() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("vendor/pkg")).expect("mkdir vendor");
        fs::write(
            temp.path().join("vendor/pkg/index.ts"),
            "export const v = 1;\n",
        )
        .expect("write vendor file");

        let result = discover_source_files_with_options(temp.path(), &[], &["vendor".to_string()])
            .expect("discover");

        assert_eq!(result.files.len(), 1);
        assert!(
            result.files[0].ends_with(Path::new("vendor/pkg/index.ts")),
            "vendor file should remain discoverable when re-included"
        );
    }
}
