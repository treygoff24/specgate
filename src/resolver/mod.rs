use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use miette::Diagnostic;
use oxc_resolver::{ResolveOptions, TsconfigOptions, TsconfigReferences};
use thiserror::Error;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;
use crate::resolver::classify::{
    classify_resolution, extract_package_name, is_explicit_node_builtin, is_node_builtin,
};
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

/// Module-map diagnostic emitted when a file matches multiple module globs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMapOverlap {
    pub file: PathBuf,
    pub selected_module: String,
    pub matched_modules: Vec<String>,
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
    #[error("failed to build module glob set")]
    InvalidModuleGlobSet {
        #[source]
        source: globset::Error,
    },
}

pub type Result<T> = std::result::Result<T, ResolverError>;

struct CompiledModuleSpec {
    module: String,
}

struct ModuleMapBuild {
    module_map: BTreeMap<PathBuf, String>,
    overlaps: Vec<ModuleMapOverlap>,
}

pub struct ModuleResolver {
    project_root: PathBuf,
    oxc_resolver: oxc_resolver::Resolver,
    cache: BTreeMap<(PathBuf, String), ResolvedImport>,
    module_map: BTreeMap<PathBuf, String>,
    module_map_overlaps: Vec<ModuleMapOverlap>,
}

impl ModuleResolver {
    /// Create resolver from project root + loaded specs.
    pub fn new(project_root: &Path, specs: &[SpecFile]) -> Result<Self> {
        let project_root =
            fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
        let module_map_build = Self::build_module_map_with_diagnostics(&project_root, specs)?;
        let options = build_resolve_options(&project_root);

        Ok(Self {
            project_root,
            oxc_resolver: oxc_resolver::Resolver::new(options),
            cache: BTreeMap::new(),
            module_map: module_map_build.module_map,
            module_map_overlaps: module_map_build.overlaps,
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

        if is_explicit_node_builtin(specifier) {
            steps.push("explicit node: builtin detected".to_string());
        } else if is_node_builtin(specifier) {
            steps.push("bare builtin candidate; resolver consulted first".to_string());
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
        Ok(Self::build_module_map_with_diagnostics(project_root, specs)?.module_map)
    }

    fn build_module_map_with_diagnostics(
        project_root: &Path,
        specs: &[SpecFile],
    ) -> Result<ModuleMapBuild> {
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

        let mut globset_builder = GlobSetBuilder::new();
        let mut compiled_specs = Vec::with_capacity(specs_sorted.len());

        for spec in specs_sorted {
            let pattern = spec
                .boundaries
                .as_ref()
                .and_then(|b| b.path.clone())
                .unwrap_or_else(|| format!("{}/**/*", spec.module.trim_matches('/')));

            let glob = Glob::new(&pattern).map_err(|source| ResolverError::InvalidModuleGlob {
                module: spec.module.clone(),
                pattern,
                source,
            })?;

            globset_builder.add(glob);
            compiled_specs.push(CompiledModuleSpec {
                module: spec.module,
            });
        }

        let globset = globset_builder
            .build()
            .map_err(|source| ResolverError::InvalidModuleGlobSet { source })?;

        let mut module_map = BTreeMap::new();
        let mut overlaps = Vec::new();

        for file in files {
            let relative = normalize_relative(project_root, &file);
            let matched_indices = sorted_globset_matches(&globset, &relative);
            if matched_indices.is_empty() {
                continue;
            }

            let canonical = fs::canonicalize(&file).unwrap_or(file);
            let selected_module = compiled_specs[matched_indices[0]].module.clone();
            module_map.insert(canonical.clone(), selected_module.clone());

            if matched_indices.len() > 1 {
                let mut matched_modules = matched_indices
                    .iter()
                    .map(|idx| compiled_specs[*idx].module.clone())
                    .collect::<Vec<_>>();
                matched_modules.dedup();

                if matched_modules.len() > 1 {
                    overlaps.push(ModuleMapOverlap {
                        file: canonical,
                        selected_module,
                        matched_modules,
                    });
                }
            }
        }

        Ok(ModuleMapBuild {
            module_map,
            overlaps,
        })
    }

    /// Inspect module assignment for a concrete file path.
    pub fn module_for_file(&self, path: &Path) -> Option<&str> {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.module_map.get(&canonical).map(String::as_str)
    }

    /// Module-map overlap diagnostics collected during construction.
    pub fn module_map_overlaps(&self) -> &[ModuleMapOverlap] {
        &self.module_map_overlaps
    }

    /// Current cache size (for tests/diagnostics).
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Invalidate all cached resolutions.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    fn resolve_uncached(&self, containing_dir: &Path, specifier: &str) -> ResolvedImport {
        if is_explicit_node_builtin(specifier) {
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
            Err(_error) if is_node_builtin(specifier) => ResolvedImport::ThirdParty {
                package_name: extract_package_name(specifier).to_string(),
            },
            Err(error) => ResolvedImport::Unresolvable {
                specifier: specifier.to_string(),
                reason: error.to_string(),
            },
        }
    }
}

fn sorted_globset_matches(globset: &GlobSet, candidate: &str) -> Vec<usize> {
    let mut matches = globset.matches(candidate).to_vec();
    matches.sort_unstable();
    matches
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
    fn clear_cache_removes_cached_entries() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "import './b';\n").expect("write a");
        fs::write(temp.path().join("src/b.ts"), "export const b = 1;\n").expect("write b");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        let _ = resolver.resolve(&temp.path().join("src/a.ts"), "./b");
        assert_eq!(resolver.cache_len(), 1);

        resolver.clear_cache();
        assert_eq!(resolver.cache_len(), 0);
    }

    #[test]
    fn module_map_overlap_is_reported_with_deterministic_precedence() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "export const a = 1;\n").expect("write a");

        let specs = vec![
            base_spec("alpha", "src/**/*"),
            base_spec("beta", "src/**/*.ts"),
        ];

        let resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        assert_eq!(
            resolver.module_for_file(&temp.path().join("src/a.ts")),
            Some("alpha")
        );

        let overlaps = resolver.module_map_overlaps();
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].selected_module, "alpha");
        assert_eq!(overlaps[0].matched_modules, vec!["alpha", "beta"]);
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
    fn bare_builtin_falls_back_to_third_party_when_unresolved() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(temp.path().join("src/a.ts"), "console.log('x');\n").expect("write");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let resolved = resolver.resolve(&temp.path().join("src/a.ts"), "path");

        assert!(matches!(
            resolved,
            ResolvedImport::ThirdParty { ref package_name } if package_name == "path"
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
