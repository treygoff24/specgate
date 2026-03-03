use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use miette::Diagnostic;
use thiserror::Error;
use walkdir::WalkDir;

pub mod config;
pub mod types;
pub mod validation;
pub mod workspace_discovery;

pub use config::{EscapeHatchConfig, JestMockMode, SpecConfig};
pub use types::{Boundaries, Constraint, Severity, SpecFile, Visibility};
pub use validation::{ValidationIssue, ValidationLevel, ValidationReport, validate_specs};

use crate::deterministic::normalize_path;

#[derive(Debug, Error, Diagnostic)]
pub enum SpecError {
    #[error("failed to read file: {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse yaml file: {path}")]
    YamlParse {
        path: PathBuf,
        #[source]
        source: yaml_serde::Error,
    },
    #[error("invalid glob pattern '{pattern}': {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

pub type Result<T> = std::result::Result<T, SpecError>;

/// Load `specgate.config.yml` from project root. Returns defaults if file is absent.
pub fn load_config(project_root: &Path) -> Result<SpecConfig> {
    let config_path = project_root.join("specgate.config.yml");
    if !config_path.exists() {
        return Ok(SpecConfig::default());
    }

    let source = fs::read_to_string(&config_path).map_err(|source| SpecError::Io {
        path: config_path.clone(),
        source,
    })?;

    yaml_serde::from_str(&source).map_err(|source| SpecError::YamlParse {
        path: config_path,
        source,
    })
}

/// Load and parse a single `.spec.yml` file.
pub fn load_spec(path: &Path) -> Result<SpecFile> {
    let source = fs::read_to_string(path).map_err(|source| SpecError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut spec: SpecFile =
        yaml_serde::from_str(&source).map_err(|source| SpecError::YamlParse {
            path: path.to_path_buf(),
            source,
        })?;

    spec.spec_path = Some(path.to_path_buf());
    Ok(spec)
}

/// Discover all `.spec.yml` files under configured directories in deterministic order.
pub fn discover_specs(project_root: &Path, config: &SpecConfig) -> Result<Vec<SpecFile>> {
    let exclude_matcher = build_globset(&config.exclude)?;

    let mut specs = Vec::new();
    let mut seen = BTreeSet::new();

    for dir in &config.spec_dirs {
        let absolute_dir = project_root.join(dir);
        if !absolute_dir.exists() {
            continue;
        }

        for entry in WalkDir::new(&absolute_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let normalized_relative = normalize_relative(project_root, path);
            if exclude_matcher.is_match(&normalized_relative) {
                continue;
            }

            if !normalized_relative.ends_with(".spec.yml") {
                continue;
            }

            let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canonical.clone()) {
                continue;
            }

            let mut spec = load_spec(&canonical)?;
            spec.spec_path = Some(canonical);
            specs.push(spec);
        }
    }

    specs.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.spec_path.cmp(&b.spec_path))
    });
    Ok(specs)
}

/// Discover + validate specs. Validation warnings are retained in report.
pub fn discover_and_validate(
    project_root: &Path,
    config: &SpecConfig,
) -> Result<(Vec<SpecFile>, ValidationReport)> {
    let specs = discover_specs(project_root, config)?;
    let report = validate_specs(&specs);
    Ok((specs, report))
}

fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|source| SpecError::InvalidGlob {
            pattern: pattern.clone(),
            source,
        })?;
        builder.add(glob);
    }

    builder.build().map_err(|source| SpecError::InvalidGlob {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn loads_default_config_when_missing() {
        let temp = TempDir::new().expect("tempdir");
        let config = load_config(temp.path()).expect("config");
        assert_eq!(config, SpecConfig::default());
    }

    #[test]
    fn parses_explicit_config() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(
            temp.path().join("specgate.config.yml"),
            "spec_dirs:\n  - specs\njest_mock_mode: enforce\n",
        )
        .expect("write config");

        let config = load_config(temp.path()).expect("config");
        assert_eq!(config.spec_dirs, vec!["specs"]);
        assert_eq!(config.jest_mock_mode, JestMockMode::Enforce);
    }

    #[test]
    fn discovers_specs_sorted_by_module() {
        let temp = TempDir::new().expect("tempdir");
        let specs_dir = temp.path().join("specs");
        fs::create_dir_all(&specs_dir).expect("mkdir");

        fs::write(
            specs_dir.join("b.spec.yml"),
            "version: \"2.2\"\nmodule: b\nconstraints: []\n",
        )
        .expect("write b");
        fs::write(
            specs_dir.join("a.spec.yml"),
            "version: \"2.2\"\nmodule: a\nconstraints: []\n",
        )
        .expect("write a");

        let config = SpecConfig {
            spec_dirs: vec!["specs".to_string()],
            ..SpecConfig::default()
        };

        let specs = discover_specs(temp.path(), &config).expect("discover");
        let modules: Vec<_> = specs.into_iter().map(|s| s.module).collect();
        assert_eq!(modules, vec!["a", "b"]);
    }
}
