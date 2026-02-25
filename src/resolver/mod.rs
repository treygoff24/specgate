use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use globset::Glob;
use miette::Diagnostic;
use oxc_resolver::{ResolveOptions, TsconfigOptions, TsconfigReferences};
use thiserror::Error;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;
use crate::resolver::classify::{classify_resolution, extract_package_name, is_node_builtin};
use crate::spec::SpecFile;

pub mod classify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedImport {
    /// Resolves to a file within project boundaries.
    FirstParty {
        resolved_path: PathBuf,
        module_id: Option<String>,
    },
    /// Resolves to a third-party package or builtin.
    ThirdParty { package_name: String },
    /// Could not be resolved.
    Unresolvable { specifier: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionExplanation {
    pub specifier: String,
    pub from_file: PathBuf,
    pub result: ResolvedImport,
    pub steps: Vec<String>,
}

#[derive(Debug, Error, Diagnostic)]
pub enum ResolverError {
    #[error("invalid module glob for '{module}': '{pattern}'")]
    InvalidModuleGlob {
        module: String,
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

pub type Result<T> = std::result::Result<T, ResolverError>;

pub struct ModuleResolver {
    project_root: PathBuf,
    oxc_resolver: oxc_resolver::Resolver,
    cache: BTreeMap<(PathBuf, String), ResolvedImport>,
    module_map: BTreeMap<PathBuf, String>,
}

impl ModuleResolver {
    /// Create resolver from project root + loaded specs.
    pub fn new(project_root: &Path, specs: &[SpecFile]) -> Result<Self> {
        let project_root =
            fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
        let module_map = Self::build_module_map(&project_root, specs)?;
        let options = build_resolve_options(&project_root);

        Ok(Self {
            project_root,
            oxc_resolver: oxc_resolver::Resolver::new(options),
            cache: BTreeMap::new(),
            module_map,
        })
    }

    /// Resolve an import specifier from a source file.
    pub fn resolve(&mut self, from_file: &Path, specifier: &str) -> ResolvedImport {
        let containing_dir = containing_dir(from_file, &self.project_root);
        let cache_key = (containing_dir.clone(), specifier.to_string());
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }

        let resolved = self.resolve_uncached(&containing_dir, specifier);
        self.cache.insert(cache_key, resolved.clone());
        resolved
    }

    /// Explain how a resolution was produced for diagnostics.
    pub fn explain_resolution(
        &mut self,
        from_file: &Path,
        specifier: &str,
    ) -> ResolutionExplanation {
        let containing_dir = containing_dir(from_file, &self.project_root);
        let cache_key = (containing_dir.clone(), specifier.to_string());
        let mut steps = Vec::new();

        steps.push(format!("from directory: {}", containing_dir.display()));
        steps.push(format!("requested specifier: {specifier}"));

        if let Some(cached) = self.cache.get(&cache_key) {
            steps.push("cache hit".to_string());
            return ResolutionExplanation {
                specifier: specifier.to_string(),
                from_file: from_file.to_path_buf(),
                result: cached.clone(),
                steps,
            };
        }

        if is_node_builtin(specifier) {
            steps.push("node builtin detected".to_string());
        }

        let result = self.resolve_uncached(&containing_dir, specifier);
        steps.push(format!("resolver output: {result:?}"));

        self.cache.insert(cache_key, result.clone());

        ResolutionExplanation {
            specifier: specifier.to_string(),
            from_file: from_file.to_path_buf(),
            result,
            steps,
        }
    }

    /// Build module membership map (absolute file path -> module id).
    pub fn build_module_map(
        project_root: &Path,
        specs: &[SpecFile],
    ) -> Result<BTreeMap<PathBuf, String>> {
        let mut files = Vec::new();
        for entry in WalkDir::new(project_root)
            .follow_links(true)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if entry.file_type().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }

        files.sort();

        let mut specs_sorted = specs.to_vec();
        specs_sorted.sort_by(|a, b| a.module.cmp(&b.module));

        let mut module_map = BTreeMap::new();
        for spec in specs_sorted {
            let pattern = spec
                .boundaries
                .as_ref()
                .and_then(|b| b.path.clone())
                .unwrap_or_else(|| format!("{}/**/*", spec.module.trim_matches('/')));

            let matcher = Glob::new(&pattern)
                .map_err(|source| ResolverError::InvalidModuleGlob {
                    module: spec.module.clone(),
                    pattern: pattern.clone(),
                    source,
                })?
                .compile_matcher();

            for file in &files {
                let relative = normalize_relative(project_root, file);
                if matcher.is_match(&relative) {
                    let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
                    module_map
                        .entry(canonical)
                        .or_insert_with(|| spec.module.clone());
                }
            }
        }

        Ok(module_map)
    }

    /// Inspect module assignment for a concrete file path.
    pub fn module_for_file(&self, path: &Path) -> Option<&str> {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.module_map.get(&canonical).map(String::as_str)
    }

    /// Current cache size (for tests/diagnostics).
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn resolve_uncached(&self, containing_dir: &Path, specifier: &str) -> ResolvedImport {
        if is_node_builtin(specifier) {
            return ResolvedImport::ThirdParty {
                package_name: extract_package_name(specifier).to_string(),
            };
        }

        match self.oxc_resolver.resolve(containing_dir, specifier) {
            Ok(resolution) => {
                let mut classified =
                    classify_resolution(&self.project_root, resolution.path(), specifier);

                if let ResolvedImport::FirstParty {
                    resolved_path,
                    module_id,
                } = &mut classified
                {
                    let canonical =
                        fs::canonicalize(&*resolved_path).unwrap_or_else(|_| resolved_path.clone());
                    *resolved_path = canonical.clone();
                    *module_id = self.module_map.get(&canonical).cloned();
                }

                classified
            }
            Err(error) => ResolvedImport::Unresolvable {
                specifier: specifier.to_string(),
                reason: error.to_string(),
            },
        }
    }
}

fn build_resolve_options(project_root: &Path) -> ResolveOptions {
    let tsconfig_path = project_root.join("tsconfig.json");
    let tsconfig = if tsconfig_path.exists() {
        Some(TsconfigOptions {
            config_file: tsconfig_path,
            references: TsconfigReferences::Auto,
        })
    } else {
        None
    };

    ResolveOptions {
        extensions: vec![
            ".ts".to_string(),
            ".tsx".to_string(),
            ".js".to_string(),
            ".jsx".to_string(),
            ".json".to_string(),
        ],
        condition_names: vec![
            "import".to_string(),
            "require".to_string(),
            "default".to_string(),
        ],
        main_fields: vec!["module".to_string(), "main".to_string()],
        modules: vec!["node_modules".to_string()],
        symlinks: true,
        tsconfig,
        ..ResolveOptions::default()
    }
}

fn containing_dir(from_file: &Path, project_root: &Path) -> PathBuf {
    let absolute_from_file = if from_file.is_absolute() {
        from_file.to_path_buf()
    } else {
        project_root.join(from_file)
    };

    absolute_from_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_root.to_path_buf())
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

    use crate::spec::Boundaries;

    use super::*;

    fn base_spec(module: &str, path: &str) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(path.to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        }
    }

    #[test]
    fn resolves_relative_first_party_and_uses_cache() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "import './b';\n").expect("write a");
        fs::write(temp.path().join("src/b.ts"), "export const b = 1;\n").expect("write b");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        let from = temp.path().join("src/a.ts");
        let first = resolver.resolve(&from, "./b");
        assert!(matches!(
            first,
            ResolvedImport::FirstParty {
                module_id: Some(ref module),
                ..
            } if module == "core"
        ));
        assert_eq!(resolver.cache_len(), 1);

        let _second = resolver.resolve(&from, "./b");
        assert_eq!(resolver.cache_len(), 1, "second resolve should hit cache");
    }

    #[test]
    fn builtin_import_is_third_party() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "console.log('x');\n").expect("write");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let resolved = resolver.resolve(&temp.path().join("src/a.ts"), "node:fs");
        assert!(matches!(
            resolved,
            ResolvedImport::ThirdParty { ref package_name } if package_name == "fs"
        ));
    }

    #[test]
    fn explain_resolution_reports_steps() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "export const a = 1;\n").expect("write");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let explanation = resolver.explain_resolution(&temp.path().join("src/a.ts"), "node:path");
        assert!(!explanation.steps.is_empty());
    }
}
