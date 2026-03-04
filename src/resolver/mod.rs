use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use miette::Diagnostic;
use oxc_resolver::{ResolveOptions, TsconfigOptions, TsconfigReferences};
use thiserror::Error;
use walkdir::WalkDir;

use crate::deterministic::normalize_path;
use crate::resolver::classify::{
    classify_resolution, extract_package_name, is_explicit_node_builtin, is_node_builtin,
};
use crate::spec::{
    SpecFile,
    config::{DEFAULT_EXCLUDED_DIRS, include_dir_set},
};

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

pub struct ModuleResolverOptions {
    pub include_dirs: Vec<String>,
    pub tsconfig_filename: String,
}

impl Default for ModuleResolverOptions {
    fn default() -> Self {
        Self {
            include_dirs: Vec::new(),
            tsconfig_filename: "tsconfig.json".to_string(),
        }
    }
}

pub struct ModuleResolver {
    project_root: PathBuf,
    resolver_without_tsconfig: oxc_resolver::Resolver,
    resolvers_by_tsconfig: BTreeMap<PathBuf, oxc_resolver::Resolver>,
    tsconfig_cache: BTreeMap<PathBuf, Option<PathBuf>>,
    cache: BTreeMap<(PathBuf, String), ResolvedImport>,
    module_map: BTreeMap<PathBuf, String>,
    module_map_overlaps: Vec<ModuleMapOverlap>,
    tsconfig_filename: String,
}

impl ModuleResolver {
    /// Create resolver from project root + loaded specs.
    pub fn new(project_root: &Path, specs: &[SpecFile]) -> Result<Self> {
        Self::new_with_options(project_root, specs, ModuleResolverOptions::default())
    }

    /// Create resolver with explicit directory re-includes for default excluded directories.
    pub fn new_with_include_dirs(
        project_root: &Path,
        specs: &[SpecFile],
        include_dirs: &[String],
    ) -> Result<Self> {
        Self::new_with_options(
            project_root,
            specs,
            ModuleResolverOptions {
                include_dirs: include_dirs.to_vec(),
                tsconfig_filename: "tsconfig.json".to_string(),
            },
        )
    }

    /// Create resolver with full options.
    pub fn new_with_options(
        project_root: &Path,
        specs: &[SpecFile],
        options: ModuleResolverOptions,
    ) -> Result<Self> {
        let project_root =
            fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
        let include_dirs = include_dir_set(&options.include_dirs);
        let module_map_build =
            Self::build_module_map_with_diagnostics(&project_root, specs, &include_dirs)?;
        let resolver_without_tsconfig = oxc_resolver::Resolver::new(build_resolve_options(None));

        Ok(Self {
            project_root,
            resolver_without_tsconfig,
            resolvers_by_tsconfig: BTreeMap::new(),
            tsconfig_cache: BTreeMap::new(),
            cache: BTreeMap::new(),
            module_map: module_map_build.module_map,
            module_map_overlaps: module_map_build.overlaps,
            tsconfig_filename: options.tsconfig_filename,
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
        Ok(
            Self::build_module_map_with_diagnostics(project_root, specs, &BTreeSet::new())?
                .module_map,
        )
    }

    fn build_module_map_with_diagnostics(
        project_root: &Path,
        specs: &[SpecFile],
        include_dirs: &BTreeSet<String>,
    ) -> Result<ModuleMapBuild> {
        let mut files = Vec::new();
        for entry in WalkDir::new(project_root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| {
                !should_skip_module_map_entry(project_root, entry.path(), include_dirs)
            })
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

            let glob = GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
                .map_err(|source| ResolverError::InvalidModuleGlob {
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

    /// Current nearest-tsconfig cache size (for tests/diagnostics).
    pub fn tsconfig_cache_len(&self) -> usize {
        self.tsconfig_cache.len()
    }

    /// Number of distinct tsconfig-backed resolvers instantiated (for tests/diagnostics).
    pub fn tsconfig_resolver_count(&self) -> usize {
        self.resolvers_by_tsconfig.len()
    }

    fn resolve_uncached(&mut self, containing_dir: &Path, specifier: &str) -> ResolvedImport {
        if is_explicit_node_builtin(specifier) {
            return ResolvedImport::ThirdParty {
                package_name: extract_package_name(specifier).to_string(),
            };
        }

        let resolution = {
            let resolver = self.resolver_for_containing_dir(containing_dir);
            resolver.resolve(containing_dir, specifier)
        };

        match resolution {
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

    fn resolver_for_containing_dir(
        &mut self,
        containing_dir: &Path,
    ) -> &mut oxc_resolver::Resolver {
        let Some(tsconfig_path) = self.nearest_tsconfig_for_dir(containing_dir) else {
            return &mut self.resolver_without_tsconfig;
        };
        let tsconfig_key = fs::canonicalize(&tsconfig_path).unwrap_or(tsconfig_path);
        self.resolvers_by_tsconfig
            .entry(tsconfig_key.clone())
            .or_insert_with(|| {
                oxc_resolver::Resolver::new(build_resolve_options(Some(tsconfig_key.clone())))
            })
    }

    fn nearest_tsconfig_for_dir(&mut self, containing_dir: &Path) -> Option<PathBuf> {
        let cache_key = if containing_dir.starts_with(&self.project_root) {
            containing_dir.to_path_buf()
        } else {
            self.project_root.clone()
        };

        if let Some(cached) = self.tsconfig_cache.get(&cache_key) {
            return cached.clone();
        }

        let resolved = nearest_tsconfig_for_dir_uncached(
            &self.project_root,
            &cache_key,
            &self.tsconfig_filename,
        );
        self.tsconfig_cache.insert(cache_key, resolved.clone());
        resolved
    }
}

fn sorted_globset_matches(globset: &GlobSet, candidate: &str) -> Vec<usize> {
    let mut matches = globset.matches(candidate).to_vec();
    matches.sort_unstable();
    matches
}

fn build_resolve_options(tsconfig_path: Option<PathBuf>) -> ResolveOptions {
    let tsconfig = tsconfig_path.map(|path| TsconfigOptions {
        config_file: path,
        references: TsconfigReferences::Auto,
    });

    ResolveOptions {
        extensions: vec![
            ".ts".to_string(),
            ".tsx".to_string(),
            ".js".to_string(),
            ".jsx".to_string(),
            ".json".to_string(),
        ],
        extension_alias: vec![
            (
                ".js".to_string(),
                vec![
                    ".ts".to_string(),
                    ".tsx".to_string(),
                    ".js".to_string(),
                    ".jsx".to_string(),
                ],
            ),
            (
                ".mjs".to_string(),
                vec![".mts".to_string(), ".mjs".to_string()],
            ),
            (
                ".cjs".to_string(),
                vec![".cts".to_string(), ".cjs".to_string()],
            ),
            (
                ".jsx".to_string(),
                vec![".tsx".to_string(), ".jsx".to_string()],
            ),
        ],
        condition_names: vec![
            "import".to_string(),
            "require".to_string(),
            "node".to_string(),
            "types".to_string(),
            "default".to_string(),
        ],
        main_fields: vec!["module".to_string(), "main".to_string()],
        modules: vec!["node_modules".to_string()],
        symlinks: true,
        tsconfig,
        ..ResolveOptions::default()
    }
}

pub(crate) fn nearest_tsconfig_for_dir_uncached(
    project_root: &Path,
    containing_dir: &Path,
    tsconfig_filename: &str,
) -> Option<PathBuf> {
    let mut current = if containing_dir.starts_with(project_root) {
        containing_dir.to_path_buf()
    } else {
        project_root.to_path_buf()
    };

    loop {
        let candidate = current.join(tsconfig_filename);
        if candidate.exists() {
            return Some(candidate);
        }

        if current == project_root {
            break;
        }

        let Some(parent) = current.parent() else {
            break;
        };
        if !parent.starts_with(project_root) {
            break;
        }
        current = parent.to_path_buf();
    }

    None
}

pub(crate) fn excluded_dir_names() -> &'static [&'static str] {
    DEFAULT_EXCLUDED_DIRS
}

fn should_skip_module_map_entry(
    project_root: &Path,
    path: &Path,
    include_dirs: &BTreeSet<String>,
) -> bool {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return false;
    };

    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();
        let segment = name.as_ref();
        excluded_dir_names().contains(&segment) && !include_dirs.contains(segment)
    })
}

fn containing_dir(from_file: &Path, project_root: &Path) -> PathBuf {
    let absolute_from_file = if from_file.is_absolute() {
        from_file.to_path_buf()
    } else {
        project_root.join(from_file)
    };
    let normalized_from_file = fs::canonicalize(&absolute_from_file).unwrap_or(absolute_from_file);

    normalized_from_file
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
    fn literal_separator_glob_does_not_cross_directories() {
        let glob = GlobBuilder::new("src/server/*.ts")
            .literal_separator(true)
            .build()
            .expect("glob");
        let matcher = glob.compile_matcher();

        assert!(matcher.is_match("src/server/index.ts"));
        assert!(!matcher.is_match("src/server/bridge/index.ts"));
    }

    #[test]
    fn nested_module_paths_do_not_overlap_when_parent_uses_single_star() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/server/bridge")).expect("mkdir");
        fs::write(
            temp.path().join("src/server/index.ts"),
            "export const server = 1;\n",
        )
        .expect("write server");
        fs::write(
            temp.path().join("src/server/bridge/index.ts"),
            "export const bridge = 1;\n",
        )
        .expect("write bridge");

        let specs = vec![
            base_spec("server", "src/server/*.ts"),
            base_spec("server/bridge", "src/server/bridge/**"),
        ];

        let resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        assert_eq!(
            resolver.module_for_file(&temp.path().join("src/server/index.ts")),
            Some("server")
        );
        assert_eq!(
            resolver.module_for_file(&temp.path().join("src/server/bridge/index.ts")),
            Some("server/bridge")
        );
        assert!(resolver.module_map_overlaps().is_empty());
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

    #[test]
    fn resolver_and_workspace_discovery_share_default_excluded_dirs() {
        assert_eq!(
            excluded_dir_names(),
            crate::spec::workspace_discovery::excluded_dir_names()
        );
        assert_eq!(
            excluded_dir_names(),
            crate::spec::config::DEFAULT_EXCLUDED_DIRS
        );
    }

    #[test]
    fn module_map_can_reinclude_default_excluded_vendor_dir() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("vendor/lib")).expect("mkdir vendor");
        fs::write(
            temp.path().join("vendor/lib/x.ts"),
            "export const vendor = 1;\n",
        )
        .expect("write vendor");

        let specs = vec![base_spec("vendor", "vendor/**/*")];
        let resolver =
            ModuleResolver::new_with_include_dirs(temp.path(), &specs, &["vendor".to_string()])
                .expect("resolver");

        assert_eq!(
            resolver.module_for_file(&temp.path().join("vendor/lib/x.ts")),
            Some("vendor")
        );
    }

    #[test]
    fn module_map_prunes_heavy_directories() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(temp.path().join("node_modules/pkg")).expect("mkdir node_modules");
        fs::create_dir_all(temp.path().join("target/generated")).expect("mkdir target");
        fs::create_dir_all(temp.path().join("vendor/lib")).expect("mkdir vendor");

        fs::write(temp.path().join("src/app.ts"), "export const app = 1;\n").expect("write app");
        fs::write(
            temp.path().join("node_modules/pkg/index.ts"),
            "export const pkg = 1;\n",
        )
        .expect("write node_modules");
        fs::write(
            temp.path().join("target/generated/out.ts"),
            "export const generated = 1;\n",
        )
        .expect("write target");
        fs::write(
            temp.path().join("vendor/lib/x.ts"),
            "export const vendor = 1;\n",
        )
        .expect("write vendor");

        let specs = vec![base_spec("core", "**/*.ts")];
        let resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        assert_eq!(
            resolver.module_for_file(&temp.path().join("src/app.ts")),
            Some("core")
        );
        assert_eq!(
            resolver.module_for_file(&temp.path().join("node_modules/pkg/index.ts")),
            None
        );
        assert_eq!(
            resolver.module_for_file(&temp.path().join("target/generated/out.ts")),
            None
        );
        assert_eq!(
            resolver.module_for_file(&temp.path().join("vendor/lib/x.ts")),
            None
        );
    }

    #[test]
    fn nodenext_extension_alias_resolves_js_specifier_to_ts_file() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::write(temp.path().join("src/a.ts"), "import './b.js';\n").expect("write a");
        fs::write(temp.path().join("src/b.ts"), "export const b = 1;\n").expect("write b");

        let specs = vec![base_spec("core", "src/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let resolved = resolver.resolve(&temp.path().join("src/a.ts"), "./b.js");

        assert!(matches!(
            resolved,
            ResolvedImport::FirstParty {
                module_id: Some(ref module),
                ..
            } if module == "core"
        ));
    }

    #[test]
    fn resolve_options_include_node_and_types_conditions_and_aliases() {
        let options = build_resolve_options(None);

        assert_eq!(
            options.condition_names,
            vec![
                "import".to_string(),
                "require".to_string(),
                "node".to_string(),
                "types".to_string(),
                "default".to_string()
            ]
        );

        assert!(
            options
                .extension_alias
                .iter()
                .any(|(ext, aliases)| ext == ".js" && aliases.contains(&".ts".to_string())),
            "expected .js extension alias to include .ts"
        );
    }

    #[test]
    fn nearest_tsconfig_prefers_nested_config() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("packages/web/src")).expect("mkdir nested");
        fs::write(temp.path().join("tsconfig.json"), "{}\n").expect("write root tsconfig");
        fs::write(temp.path().join("packages/web/tsconfig.json"), "{}\n")
            .expect("write nested tsconfig");

        let selected = nearest_tsconfig_for_dir_uncached(
            temp.path(),
            &temp.path().join("packages/web/src"),
            "tsconfig.json",
        )
        .expect("nearest tsconfig");
        assert_eq!(
            selected,
            temp.path().join("packages/web/tsconfig.json"),
            "nested context should take precedence over root tsconfig"
        );
    }

    #[test]
    fn tsconfig_lookup_is_cached_per_directory() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("packages/web/src")).expect("mkdir nested src");
        fs::write(temp.path().join("tsconfig.json"), "{}\n").expect("write root tsconfig");
        fs::write(temp.path().join("packages/web/tsconfig.json"), "{}\n")
            .expect("write nested tsconfig");

        fs::write(
            temp.path().join("packages/web/src/a.ts"),
            "import './b'; import './c';\n",
        )
        .expect("write a");
        fs::write(
            temp.path().join("packages/web/src/b.ts"),
            "export const b = 1;\n",
        )
        .expect("write b");
        fs::write(
            temp.path().join("packages/web/src/c.ts"),
            "export const c = 1;\n",
        )
        .expect("write c");

        let specs = vec![base_spec("web", "packages/web/**/*")];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
        let from = temp.path().join("packages/web/src/a.ts");

        let _ = resolver.resolve(&from, "./b");
        let _ = resolver.resolve(&from, "./c");

        assert_eq!(
            resolver.tsconfig_cache_len(),
            1,
            "nearest tsconfig walk should be cached per containing dir"
        );
    }

    #[test]
    fn resolve_uses_nearest_tsconfig_context_for_aliases() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(temp.path().join("packages/web/src")).expect("mkdir nested src");
        fs::create_dir_all(temp.path().join("src/root")).expect("mkdir root alias dir");
        fs::create_dir_all(temp.path().join("packages/web/nested"))
            .expect("mkdir nested alias dir");

        fs::write(
            temp.path().join("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@cfg/*": ["src/root/*"]
    }
  }
}
"#,
        )
        .expect("write root tsconfig");
        fs::write(
            temp.path().join("packages/web/tsconfig.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@cfg/*": ["nested/*"]
    }
  }
}
"#,
        )
        .expect("write nested tsconfig");

        fs::write(
            temp.path().join("src/root/value.ts"),
            "export const rootTarget = 1;\n",
        )
        .expect("write root target");
        fs::write(
            temp.path().join("packages/web/nested/value.ts"),
            "export const nestedTarget = 1;\n",
        )
        .expect("write nested target");
        fs::write(
            temp.path().join("packages/web/src/app.ts"),
            "import '@cfg/value';\n",
        )
        .expect("write app");
        fs::write(
            temp.path().join("src/root-user.ts"),
            "import '@cfg/value';\n",
        )
        .expect("write root user");

        let specs = vec![
            base_spec("root", "src/**/*"),
            base_spec("web", "packages/web/**/*"),
        ];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        let nested = resolver.resolve(&temp.path().join("packages/web/src/app.ts"), "@cfg/value");
        assert!(
            matches!(
                nested,
                ResolvedImport::FirstParty {
                    module_id: Some(ref module),
                    resolved_path: ref path,
                } if module == "web" && path.ends_with(Path::new("packages/web/nested/value.ts"))
            ),
            "unexpected nested resolution: {nested:?}"
        );

        let root = resolver.resolve(&temp.path().join("src/root-user.ts"), "@cfg/value");
        assert!(
            matches!(
                root,
                ResolvedImport::FirstParty {
                    module_id: Some(ref module),
                    resolved_path: ref path,
                } if module == "root" && path.ends_with(Path::new("src/root/value.ts"))
            ),
            "unexpected root resolution: {root:?}"
        );
    }

    #[test]
    fn nearest_tsconfig_finds_custom_filename() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::write(temp.path().join("tsconfig.base.json"), "{}\n").expect("write custom tsconfig");

        let selected = nearest_tsconfig_for_dir_uncached(
            temp.path(),
            &temp.path().join("src"),
            "tsconfig.base.json",
        )
        .expect("nearest tsconfig");

        assert_eq!(selected, temp.path().join("tsconfig.base.json"));
    }

    #[test]
    fn nearest_tsconfig_finds_custom_filename_ignores_default() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::write(temp.path().join("tsconfig.json"), "{}\n").expect("write default tsconfig");

        let result = nearest_tsconfig_for_dir_uncached(
            temp.path(),
            &temp.path().join("src"),
            "tsconfig.base.json",
        );

        assert!(
            result.is_none(),
            "should not find tsconfig.json when searching for tsconfig.base.json"
        );
    }

    #[test]
    fn resolver_uses_custom_tsconfig_filename() {
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(temp.path().join("lib")).expect("mkdir lib");

        fs::write(
            temp.path().join("tsconfig.build.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@lib/*": ["lib/*"]
    }
  }
}
"#,
        )
        .expect("write custom tsconfig");

        fs::write(
            temp.path().join("lib/utils.ts"),
            "export const utils = 1;\n",
        )
        .expect("write lib target");
        fs::write(temp.path().join("src/app.ts"), "import '@lib/utils';\n").expect("write app");

        let specs = vec![base_spec("lib", "lib/**/*"), base_spec("app", "src/**/*")];
        let mut resolver = ModuleResolver::new_with_options(
            temp.path(),
            &specs,
            ModuleResolverOptions {
                include_dirs: Vec::new(),
                tsconfig_filename: "tsconfig.build.json".to_string(),
            },
        )
        .expect("resolver");

        let resolved = resolver.resolve(&temp.path().join("src/app.ts"), "@lib/utils");
        assert!(
            matches!(
                resolved,
                ResolvedImport::FirstParty {
                    module_id: Some(ref module),
                    ..
                } if module == "lib"
            ),
            "expected alias resolution via tsconfig.build.json, got: {resolved:?}"
        );
    }

    #[test]
    fn tsconfig_cache_avoids_redundant_walks_across_packages() {
        // 2-package monorepo: packages/alpha and packages/beta, each with their own tsconfig
        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("packages/alpha/src")).expect("mkdir alpha/src");
        fs::create_dir_all(temp.path().join("packages/alpha/lib")).expect("mkdir alpha/lib");
        fs::create_dir_all(temp.path().join("packages/beta/src")).expect("mkdir beta/src");
        fs::create_dir_all(temp.path().join("packages/beta/lib")).expect("mkdir beta/lib");

        fs::write(temp.path().join("packages/alpha/tsconfig.json"), "{}\n")
            .expect("write alpha tsconfig");
        fs::write(temp.path().join("packages/beta/tsconfig.json"), "{}\n")
            .expect("write beta tsconfig");

        // Source files — 3 unique dirs for alpha, 2 for beta (5 total unique dirs)
        for (path, content) in &[
            ("packages/alpha/src/a.ts", "export const a = 1;\n"),
            ("packages/alpha/src/b.ts", "export const b = 1;\n"),
            ("packages/alpha/lib/util.ts", "export const util = 1;\n"),
            ("packages/beta/src/x.ts", "export const x = 1;\n"),
            ("packages/beta/lib/helper.ts", "export const helper = 1;\n"),
        ] {
            fs::write(temp.path().join(path), content).expect("write source file");
        }

        let specs = vec![
            base_spec("alpha", "packages/alpha/**/*"),
            base_spec("beta", "packages/beta/**/*"),
        ];
        let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");

        // Resolve from 4 different directories across both packages
        // alpha/src (a.ts and b.ts share dir), alpha/lib, beta/src, beta/lib = 4 unique dirs
        let _ = resolver.resolve(&temp.path().join("packages/alpha/src/a.ts"), "./b");
        let _ = resolver.resolve(&temp.path().join("packages/alpha/src/b.ts"), "./a");
        let _ = resolver.resolve(&temp.path().join("packages/alpha/lib/util.ts"), "./missing");
        let _ = resolver.resolve(&temp.path().join("packages/beta/src/x.ts"), "./missing");
        let _ = resolver.resolve(
            &temp.path().join("packages/beta/lib/helper.ts"),
            "./missing",
        );

        // Each unique containing dir gets one cache entry — 4 unique dirs total
        // (alpha/src, alpha/lib, beta/src, beta/lib)
        assert_eq!(
            resolver.tsconfig_cache_len(),
            4,
            "tsconfig cache should have one entry per unique queried directory"
        );

        // Only 2 distinct tsconfig files → only 2 resolver instances
        assert_eq!(
            resolver.tsconfig_resolver_count(),
            2,
            "should instantiate one resolver per unique tsconfig (one per package)"
        );
    }
}
