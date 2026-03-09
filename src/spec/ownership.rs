use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::deterministic::normalize_path;
use crate::spec::types::SpecFile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipReport {
    pub total_source_files: usize,
    pub claimed_files: usize,
    pub unclaimed_files: Vec<String>,
    pub overlapping_files: Vec<OverlapEntry>,
    pub duplicate_module_ids: Vec<DuplicateModule>,
    pub orphaned_specs: Vec<OrphanedSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlapEntry {
    pub file: String,
    pub claimed_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateModule {
    pub module_id: String,
    pub spec_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanedSpec {
    pub module_id: String,
    pub spec_path: String,
    pub path_glob: String,
}

/// Validate ownership of source files across specs.
pub fn validate_ownership(
    project_root: &Path,
    specs: &[SpecFile],
    source_files: &[PathBuf],
) -> OwnershipReport {
    let duplicate_module_ids = detect_duplicate_module_ids(specs);

    // Build per-spec glob matchers for specs that have boundaries.path
    let spec_matchers: Vec<(String, String, GlobSet)> = specs
        .iter()
        .filter_map(|spec| {
            let boundaries = spec.boundaries.as_ref()?;
            let path_glob = boundaries.path.as_ref()?;
            let spec_path = spec
                .spec_path
                .as_ref()
                .map(|p| normalize_repo_relative(project_root, p))
                .unwrap_or_else(|| spec.module.clone());

            let glob = Glob::new(path_glob).ok()?;
            let mut builder = GlobSetBuilder::new();
            builder.add(glob);
            let globset = builder.build().ok()?;

            Some((spec.module.clone(), spec_path, globset))
        })
        .collect();

    let mut claimed_count = 0usize;
    let mut unclaimed_files: Vec<String> = Vec::new();
    let mut overlapping_files: Vec<OverlapEntry> = Vec::new();

    // Track which source files each spec actually matches (for orphan detection)
    let mut spec_match_counts: BTreeMap<String, usize> = spec_matchers
        .iter()
        .map(|(module_id, _, _)| (module_id.clone(), 0))
        .collect();

    for source_path in source_files {
        let rel = normalize_repo_relative(project_root, source_path);

        let mut claimants: Vec<String> = Vec::new();
        for (module_id, _, globset) in &spec_matchers {
            if globset.is_match(&rel) {
                claimants.push(module_id.clone());
            }
        }
        claimants.sort();

        match claimants.len() {
            0 => {
                unclaimed_files.push(rel);
            }
            1 => {
                claimed_count += 1;
                *spec_match_counts.get_mut(&claimants[0]).unwrap() += 1;
            }
            _ => {
                claimed_count += 1;
                for m in &claimants {
                    *spec_match_counts.get_mut(m).unwrap() += 1;
                }
                overlapping_files.push(OverlapEntry {
                    file: rel,
                    claimed_by: claimants,
                });
            }
        }
    }

    unclaimed_files.sort();
    overlapping_files.sort_by(|a, b| a.file.cmp(&b.file));

    let mut orphaned_specs: Vec<OrphanedSpec> = spec_matchers
        .iter()
        .filter(|(module_id, _, _)| spec_match_counts.get(module_id).copied().unwrap_or(0) == 0)
        .map(|(module_id, spec_path, _)| {
            let path_glob = specs
                .iter()
                .find(|s| &s.module == module_id)
                .and_then(|s| s.boundaries.as_ref())
                .and_then(|b| b.path.as_ref())
                .cloned()
                .unwrap_or_default();
            OrphanedSpec {
                module_id: module_id.clone(),
                spec_path: spec_path.clone(),
                path_glob,
            }
        })
        .collect();
    orphaned_specs.sort_by(|a, b| a.module_id.cmp(&b.module_id));

    OwnershipReport {
        total_source_files: source_files.len(),
        claimed_files: claimed_count,
        unclaimed_files,
        overlapping_files,
        duplicate_module_ids,
        orphaned_specs,
    }
}

fn detect_duplicate_module_ids(specs: &[SpecFile]) -> Vec<DuplicateModule> {
    let mut by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for spec in specs {
        let spec_path = spec
            .spec_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("<{}>", spec.module));
        by_module
            .entry(spec.module.clone())
            .or_default()
            .push(spec_path);
    }

    let mut duplicates: Vec<DuplicateModule> = by_module
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .map(|(module_id, mut spec_paths)| {
            spec_paths.sort();
            DuplicateModule {
                module_id,
                spec_paths,
            }
        })
        .collect();
    duplicates.sort_by(|a, b| a.module_id.cmp(&b.module_id));
    duplicates
}

fn normalize_repo_relative(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(relative) => normalize_path(relative),
        Err(_) => normalize_path(path),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::spec::types::{Boundaries, SpecFile};

    fn make_spec(module: &str, path_glob: Option<&str>) -> SpecFile {
        make_spec_with_path(module, path_glob, None)
    }

    fn make_spec_with_path(
        module: &str,
        path_glob: Option<&str>,
        spec_path: Option<&str>,
    ) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: path_glob.map(|p| Boundaries {
                path: Some(p.to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: spec_path.map(PathBuf::from),
        }
    }

    fn root() -> PathBuf {
        PathBuf::from("/project")
    }

    fn files(paths: &[&str]) -> Vec<PathBuf> {
        paths
            .iter()
            .map(|p| PathBuf::from("/project").join(p))
            .collect()
    }

    #[test]
    fn test_no_issues_clean_ownership() {
        let specs = vec![
            make_spec("api/core", Some("src/api/**")),
            make_spec("ui/shared", Some("src/ui/**")),
        ];
        let source = files(&["src/api/index.ts", "src/ui/button.tsx"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.total_source_files, 2);
        assert_eq!(report.claimed_files, 2);
        assert!(report.unclaimed_files.is_empty());
        assert!(report.overlapping_files.is_empty());
        assert!(report.duplicate_module_ids.is_empty());
        assert!(report.orphaned_specs.is_empty());
    }

    #[test]
    fn test_unclaimed_file_detected() {
        let specs = vec![make_spec("api/core", Some("src/api/**"))];
        let source = files(&["src/api/index.ts", "src/utils/orphan.ts"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.unclaimed_files, vec!["src/utils/orphan.ts"]);
        assert_eq!(report.claimed_files, 1);
        assert_eq!(report.total_source_files, 2);
    }

    #[test]
    fn test_overlapping_ownership_detected() {
        let specs = vec![
            make_spec("api/core", Some("src/shared/**")),
            make_spec("ui/shared", Some("src/shared/**")),
        ];
        let source = files(&["src/shared/utils.ts"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.overlapping_files.len(), 1);
        let overlap = &report.overlapping_files[0];
        assert_eq!(overlap.file, "src/shared/utils.ts");
        assert!(overlap.claimed_by.contains(&"api/core".to_string()));
        assert!(overlap.claimed_by.contains(&"ui/shared".to_string()));
        assert!(report.unclaimed_files.is_empty());
    }

    #[test]
    fn test_duplicate_module_id_detected() {
        let specs = vec![
            make_spec_with_path("api/core", None, Some("/project/specs/a/api-core.spec.yml")),
            make_spec_with_path("api/core", None, Some("/project/specs/b/api-core.spec.yml")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.duplicate_module_ids.len(), 1);
        assert_eq!(report.duplicate_module_ids[0].module_id, "api/core");
        assert_eq!(report.duplicate_module_ids[0].spec_paths.len(), 2);
    }

    #[test]
    fn test_orphaned_spec_detected() {
        let specs = vec![make_spec("old-module", Some("src/old/**"))];
        let source = files(&["src/new/index.ts"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.orphaned_specs.len(), 1);
        assert_eq!(report.orphaned_specs[0].module_id, "old-module");
        assert_eq!(report.orphaned_specs[0].path_glob, "src/old/**");
        assert_eq!(report.unclaimed_files, vec!["src/new/index.ts"]);
    }

    #[test]
    fn test_spec_without_boundaries_skipped() {
        let specs = vec![
            make_spec("api/core", None),
            make_spec("ui/shared", Some("src/ui/**")),
        ];
        let source = files(&["src/api/index.ts", "src/ui/button.tsx"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.unclaimed_files, vec!["src/api/index.ts"]);
        assert_eq!(report.claimed_files, 1);
        assert!(report.orphaned_specs.is_empty());
    }

    #[test]
    fn test_report_serialization() {
        let report = OwnershipReport {
            total_source_files: 5,
            claimed_files: 3,
            unclaimed_files: vec!["src/a.ts".to_string()],
            overlapping_files: vec![OverlapEntry {
                file: "src/b.ts".to_string(),
                claimed_by: vec!["mod/a".to_string(), "mod/b".to_string()],
            }],
            duplicate_module_ids: vec![],
            orphaned_specs: vec![OrphanedSpec {
                module_id: "old".to_string(),
                spec_path: "specs/old.spec.yml".to_string(),
                path_glob: "src/old/**".to_string(),
            }],
        };

        let json = serde_json::to_string(&report).expect("serialize");
        let restored: OwnershipReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, report);
    }
}