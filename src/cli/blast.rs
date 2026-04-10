use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use globset::GlobSet;
use walkdir::WalkDir;

use crate::cli::LoadedProject;

use crate::deterministic::normalize_repo_relative;
use crate::git_blast::BlastRadius;
use crate::graph::DependencyGraph;
use crate::resolver::{ModuleResolver, ModuleResolverOptions};
use crate::spec::config::{
    build_exclude_matcher, include_dir_set, path_matches_exclude, should_skip_default_dir,
};

/// Data needed for blast-radius computation.
pub(crate) struct BlastRadiusData {
    module_to_files: BTreeMap<String, BTreeSet<String>>,
    file_to_module: BTreeMap<String, String>,
    importer_graph: BTreeMap<String, BTreeSet<String>>,
}

pub(crate) fn build_blast_radius(
    loaded: &LoadedProject,
    since_ref: Option<&str>,
    edge_pairs: &BTreeSet<(String, String)>,
) -> std::result::Result<Option<BlastRadius>, String> {
    let Some(since_ref) = since_ref else {
        return Ok(None);
    };

    let blast_data = build_blast_radius_data(loaded, edge_pairs)?;
    let radius = crate::git_blast::compute_blast_radius(
        &loaded.project_root,
        since_ref,
        &blast_data.module_to_files,
        &blast_data.file_to_module,
        &blast_data.importer_graph,
    );

    if let Some(error) = &radius.error {
        return Err(error.clone());
    }

    Ok(Some(radius))
}

pub(crate) fn derive_blast_edge_pairs(
    loaded: &LoadedProject,
    since_ref: Option<&str>,
) -> std::result::Result<BTreeSet<(String, String)>, String> {
    if since_ref.is_none() {
        return Ok(BTreeSet::new());
    }

    let mut resolver = ModuleResolver::new_with_options(
        &loaded.project_root,
        &loaded.specs,
        ModuleResolverOptions {
            include_dirs: loaded.config.include_dirs.clone(),
            tsconfig_filename: loaded.config.tsconfig_filename.clone(),
        },
    )
    .map_err(|error| format!("failed to initialize module resolver for blast radius: {error}"))?;

    let graph = DependencyGraph::build(&loaded.project_root, &mut resolver, &loaded.config)
        .map_err(|error| format!("failed to build dependency graph for blast radius: {error}"))?;

    Ok(graph
        .dependency_edges()
        .into_iter()
        .map(|edge| {
            (
                normalize_repo_relative(&loaded.project_root, &edge.from),
                normalize_repo_relative(&loaded.project_root, &edge.to),
            )
        })
        .collect())
}

pub(crate) fn build_blast_radius_data(
    loaded: &LoadedProject,
    edge_pairs: &BTreeSet<(String, String)>,
) -> std::result::Result<BlastRadiusData, String> {
    let exclude_matcher = build_exclude_matcher(&loaded.config.exclude)
        .map_err(|error| format!("failed to compile blast-radius exclude globs: {error}"))?;
    let include_dirs = include_dir_set(&loaded.config.include_dirs);
    let mut module_to_files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut file_to_module: BTreeMap<String, String> = BTreeMap::new();
    let mut project_files = Vec::new();
    for entry in WalkDir::new(&loaded.project_root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| {
            !should_skip_blast_entry(
                &loaded.project_root,
                entry.path(),
                entry.file_type().is_dir(),
                &include_dirs,
                &exclude_matcher,
            )
        })
    {
        let entry = entry.map_err(|error| {
            format!(
                "failed to discover files for blast radius under {}: {error}",
                loaded.project_root.display()
            )
        })?;
        if entry.file_type().is_file()
            && !path_matches_exclude(&loaded.project_root, entry.path(), false, &exclude_matcher)
        {
            project_files.push(normalize_repo_relative(&loaded.project_root, entry.path()));
        }
    }

    // Map files to modules based on spec boundaries
    for spec in &loaded.specs {
        let module_id = spec.module.clone();
        let files_set = module_to_files.entry(module_id.clone()).or_default();

        // Include spec file itself
        if let Some(spec_path) = &spec.spec_path {
            let relative = normalize_repo_relative(&loaded.project_root, spec_path);
            files_set.insert(relative.clone());
            file_to_module.insert(relative, module_id.clone());
        }

        // Include files matched by boundaries.path
        if let Some(boundaries) = &spec.boundaries {
            if let Some(path_glob) = &boundaries.path {
                if let Ok(glob) = globset::Glob::new(path_glob) {
                    if let Ok(set) = globset::GlobSetBuilder::new().add(glob).build() {
                        for relative in &project_files {
                            if set.is_match(relative) {
                                files_set.insert(relative.clone());
                                file_to_module.insert(relative.clone(), module_id.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    let mut importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (from_file, to_file) in edge_pairs {
        let Some(from_module) = file_to_module.get(from_file) else {
            continue;
        };
        let Some(to_module) = file_to_module.get(to_file) else {
            continue;
        };

        if from_module == to_module {
            continue;
        }

        importer_graph
            .entry(to_module.clone())
            .or_default()
            .insert(from_module.clone());
    }

    Ok(BlastRadiusData {
        module_to_files,
        file_to_module,
        importer_graph,
    })
}

fn should_skip_blast_entry(
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
