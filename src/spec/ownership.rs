use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::deterministic::normalize_path;
use crate::spec::types::SpecFile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidGlob {
    pub module_id: String,
    pub pattern: String,
    pub error: String,
}

/// Two ownership globs from different spec files that overlap — any path matching
/// one glob could also match the other, indicating contradictory ownership claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContradictoryGlob {
    /// First module defining the glob.
    pub module_a: String,
    /// Glob pattern from the first module.
    pub glob_a: String,
    /// Spec file path for the first module.
    pub spec_path_a: String,
    /// Second module defining the glob.
    pub module_b: String,
    /// Glob pattern from the second module.
    pub glob_b: String,
    /// Spec file path for the second module.
    pub spec_path_b: String,
    /// Human-readable description of the contradiction.
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipReport {
    pub total_source_files: usize,
    pub claimed_files: usize,
    pub unclaimed_files: Vec<String>,
    pub overlapping_files: Vec<OverlapEntry>,
    pub duplicate_module_ids: Vec<DuplicateModule>,
    pub orphaned_specs: Vec<OrphanedSpec>,
    /// Globs that failed to compile — surfaced instead of silently dropped.
    pub invalid_globs: Vec<InvalidGlob>,
    /// Ownership globs from different spec files that structurally overlap/contradict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradictory_globs: Vec<ContradictoryGlob>,
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
    let duplicate_module_ids = detect_duplicate_module_ids(project_root, specs);

    // Build per-spec glob matchers for specs that have boundaries.path.
    // Invalid globs are recorded instead of silently skipped (Finding #2).
    let mut spec_matchers: Vec<(String, String, String, GlobSet)> = Vec::new();
    let mut invalid_globs: Vec<InvalidGlob> = Vec::new();

    for spec in specs {
        let boundaries = match spec.boundaries.as_ref() {
            Some(b) => b,
            None => continue,
        };
        let path_glob = match boundaries.path.as_ref() {
            Some(p) => p,
            None => continue,
        };
        let spec_path = spec
            .spec_path
            .as_ref()
            .map(|p| normalize_repo_relative(project_root, p))
            .unwrap_or_else(|| spec.module.clone());

        match Glob::new(path_glob) {
            Ok(glob) => {
                let mut builder = GlobSetBuilder::new();
                builder.add(glob);
                match builder.build() {
                    Ok(globset) => {
                        spec_matchers.push((
                            spec.module.clone(),
                            spec_path,
                            path_glob.clone(),
                            globset,
                        ));
                    }
                    Err(e) => {
                        invalid_globs.push(InvalidGlob {
                            module_id: spec.module.clone(),
                            pattern: path_glob.clone(),
                            error: e.to_string(),
                        });
                    }
                }
            }
            Err(e) => {
                invalid_globs.push(InvalidGlob {
                    module_id: spec.module.clone(),
                    pattern: path_glob.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    let mut claimed_count = 0usize;
    let mut unclaimed_files: Vec<String> = Vec::new();
    let mut overlapping_files: Vec<OverlapEntry> = Vec::new();

    // Track match counts by index to avoid collision when two specs share a module_id (Finding #3).
    let mut match_counts: Vec<usize> = vec![0; spec_matchers.len()];

    for source_path in source_files {
        let rel = normalize_repo_relative(project_root, source_path);

        let matched_indices: Vec<usize> = spec_matchers
            .iter()
            .enumerate()
            .filter(|(_, (_, _, _, globset))| globset.is_match(&rel))
            .map(|(i, _)| i)
            .collect();

        for &i in &matched_indices {
            match_counts[i] += 1;
        }

        match matched_indices.len() {
            0 => {
                unclaimed_files.push(rel);
            }
            1 => {
                claimed_count += 1;
            }
            _ => {
                claimed_count += 1;
                let mut claimants: Vec<String> = matched_indices
                    .iter()
                    .map(|&i| spec_matchers[i].0.clone())
                    .collect();
                claimants.sort();
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
        .enumerate()
        .filter(|(i, _)| match_counts[*i] == 0)
        .map(|(_, (module_id, spec_path, path_glob, _))| OrphanedSpec {
            module_id: module_id.clone(),
            spec_path: spec_path.clone(),
            path_glob: path_glob.clone(),
        })
        .collect();
    orphaned_specs.sort_by(|a, b| a.module_id.cmp(&b.module_id));

    let contradictory_globs =
        detect_contradictory_globs(&spec_matchers, source_files, project_root);

    OwnershipReport {
        total_source_files: source_files.len(),
        claimed_files: claimed_count,
        unclaimed_files,
        overlapping_files,
        duplicate_module_ids,
        orphaned_specs,
        invalid_globs,
        contradictory_globs,
    }
}

/// Detect contradictory ownership globs across different modules.
///
/// Two globs from different modules are considered contradictory when:
/// 1. They are identical patterns (exact match).
/// 2. One structurally contains the other (e.g., `src/**` vs `src/api/**`).
/// 3. They share witnessed file overlaps from the actual source tree.
///
/// Pairs where both entries belong to the same module are skipped — those are
/// not cross-module contradictions.
fn detect_contradictory_globs(
    spec_matchers: &[(String, String, String, GlobSet)],
    source_files: &[PathBuf],
    project_root: &Path,
) -> Vec<ContradictoryGlob> {
    use std::collections::BTreeSet;

    let mut contradictions = Vec::new();
    let mut seen_pairs: BTreeSet<(usize, usize)> = BTreeSet::new();

    for i in 0..spec_matchers.len() {
        for j in (i + 1)..spec_matchers.len() {
            let (module_a, spec_path_a, glob_a, globset_a) = &spec_matchers[i];
            let (module_b, spec_path_b, glob_b, globset_b) = &spec_matchers[j];

            // Skip pairs within the same module — not a cross-module contradiction.
            if module_a == module_b {
                continue;
            }

            // Deterministic pair key to avoid duplicates.
            let pair_key = (i, j);
            if seen_pairs.contains(&pair_key) {
                continue;
            }

            let is_contradictory = if glob_a == glob_b {
                // Identical patterns always contradict.
                true
            } else if globs_structurally_overlap(glob_a, glob_b) {
                // One glob structurally contains the other.
                true
            } else {
                // Fall back to file-witnessed overlap: check if any source file
                // matches both globs.
                source_files.iter().any(|path| {
                    let rel = normalize_repo_relative(project_root, path);
                    globset_a.is_match(&rel) && globset_b.is_match(&rel)
                })
            };

            if is_contradictory {
                seen_pairs.insert(pair_key);

                // Deterministic ordering: sort the pair by (module, glob) tuple.
                let (ma, ga, spa, mb, gb, spb) = if (module_a, glob_a) <= (module_b, glob_b) {
                    (module_a, glob_a, spec_path_a, module_b, glob_b, spec_path_b)
                } else {
                    (module_b, glob_b, spec_path_b, module_a, glob_a, spec_path_a)
                };

                contradictions.push(ContradictoryGlob {
                    module_a: ma.clone(),
                    glob_a: ga.clone(),
                    spec_path_a: spa.clone(),
                    module_b: mb.clone(),
                    glob_b: gb.clone(),
                    spec_path_b: spb.clone(),
                    description: format!(
                        "Glob '{ga}' (module '{ma}') and glob '{gb}' (module '{mb}') define overlapping ownership — files may match both patterns.",
                    ),
                });
            }
        }
    }

    // Sort for deterministic output.
    contradictions.sort_by(|a, b| {
        a.module_a
            .cmp(&b.module_a)
            .then_with(|| a.glob_a.cmp(&b.glob_a))
            .then_with(|| a.module_b.cmp(&b.module_b))
            .then_with(|| a.glob_b.cmp(&b.glob_b))
    });
    contradictions
}

/// Structural overlap check for two glob patterns.
///
/// Returns `true` when one pattern is clearly a subset of the other based on
/// common prefix analysis. For example:
/// - `src/**` vs `src/api/**` → overlap (one is a sub-tree of the other)
/// - `src/api/**` vs `src/api/**/*.ts` → overlap (more specific sub-tree)
/// - `src/api/**` vs `src/ui/**` → no overlap (disjoint prefixes)
fn globs_structurally_overlap(glob_a: &str, glob_b: &str) -> bool {
    // Extract the literal prefix before any glob metacharacter.
    let prefix_a = literal_prefix(glob_a);
    let prefix_b = literal_prefix(glob_b);

    // If one prefix starts with the other AND the longer pattern uses a glob
    // suffix, they structurally overlap.
    if prefix_a.is_empty() || prefix_b.is_empty() {
        // A pattern with no literal prefix (e.g., `**/*.ts`) is a wildcard
        // that can match anything — it overlaps with any other pattern.
        return true;
    }

    prefix_a.starts_with(&prefix_b) || prefix_b.starts_with(&prefix_a)
}

/// Extract the literal (non-glob) prefix of a glob pattern.
/// Stops at the first glob metacharacter (`*`, `?`, `[`, `{`).
fn literal_prefix(pattern: &str) -> String {
    let mut prefix = String::new();
    for ch in pattern.chars() {
        if ch == '*' || ch == '?' || ch == '[' || ch == '{' {
            break;
        }
        prefix.push(ch);
    }
    prefix
}

fn detect_duplicate_module_ids(project_root: &Path, specs: &[SpecFile]) -> Vec<DuplicateModule> {
    let mut by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for spec in specs {
        // Finding #12: normalize spec_paths so the same file with different path
        // representations (absolute vs relative, backslash vs forward-slash) is
        // detected as a duplicate.
        let spec_path = spec
            .spec_path
            .as_ref()
            .map(|p| normalize_repo_relative(project_root, p))
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
            invalid_globs: vec![],
            contradictory_globs: vec![],
        };

        let json = serde_json::to_string(&report).expect("serialize");
        let restored: OwnershipReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, report);
    }

    // --- Finding #2: invalid globs are surfaced in report.invalid_globs ---

    #[test]
    fn test_invalid_glob_reported_not_swallowed() {
        // "[invalid" is an unclosed bracket — globset rejects it
        let specs = vec![make_spec("bad-module", Some("[invalid"))];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.invalid_globs.len(), 1);
        assert_eq!(report.invalid_globs[0].module_id, "bad-module");
        assert_eq!(report.invalid_globs[0].pattern, "[invalid");
        assert!(
            !report.invalid_globs[0].error.is_empty(),
            "error message should be non-empty"
        );
        // The spec should NOT appear as an orphan (it wasn't added to matchers)
        assert!(report.orphaned_specs.is_empty());
    }

    // --- Finding #3: orphan detection per spec-matcher index, not per module_id ---

    #[test]
    fn test_orphan_detection_index_based_no_collision() {
        // Two specs with the same module_id but different globs.
        // One glob matches a file; the other matches nothing.
        let specs = vec![
            make_spec("auth", Some("src/auth/**")),
            make_spec("auth", Some("src/payments/**")),
        ];
        let source = files(&["src/auth/index.ts"]);

        let report = validate_ownership(&root(), &specs, &source);

        // The payments glob matches nothing -> should be orphaned
        assert_eq!(
            report.orphaned_specs.len(),
            1,
            "expected one orphan, got: {:?}",
            report.orphaned_specs
        );
        assert_eq!(report.orphaned_specs[0].module_id, "auth");
        assert_eq!(report.orphaned_specs[0].path_glob, "src/payments/**");
    }

    // --- Finding #12: duplicate_module_ids normalizes spec_paths ---

    #[test]
    fn test_duplicate_module_paths_are_normalized() {
        let specs = vec![
            make_spec_with_path("api/core", None, Some("/project/specs/a/api-core.spec.yml")),
            make_spec_with_path("api/core", None, Some("/project/specs/b/api-core.spec.yml")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.duplicate_module_ids.len(), 1);
        assert_eq!(report.duplicate_module_ids[0].module_id, "api/core");
        // Paths should be repo-relative, not absolute
        for path in &report.duplicate_module_ids[0].spec_paths {
            assert!(
                !path.starts_with('/'),
                "expected repo-relative path, got absolute: {path}"
            );
            assert!(
                path.starts_with("specs/"),
                "expected specs/... path, got: {path}"
            );
        }
    }

    // --- Contradictory ownership globs ---

    #[test]
    fn test_contradictory_globs_identical_patterns_different_modules() {
        let specs = vec![
            make_spec("api/core", Some("src/shared/**")),
            make_spec("ui/shared", Some("src/shared/**")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(
            report.contradictory_globs.len(),
            1,
            "expected one contradictory pair: {:?}",
            report.contradictory_globs
        );
        let entry = &report.contradictory_globs[0];
        assert!(entry.description.contains("overlapping ownership"));
    }

    #[test]
    fn test_contradictory_globs_structural_overlap() {
        // src/** contains src/api/** — structurally overlapping.
        let specs = vec![
            make_spec("wide", Some("src/**")),
            make_spec("narrow", Some("src/api/**")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(
            report.contradictory_globs.len(),
            1,
            "expected one contradictory pair: {:?}",
            report.contradictory_globs
        );
    }

    #[test]
    fn test_contradictory_globs_disjoint_patterns_no_contradiction() {
        let specs = vec![
            make_spec("api", Some("src/api/**")),
            make_spec("ui", Some("src/ui/**")),
        ];
        let source = files(&["src/api/index.ts", "src/ui/button.tsx"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert!(
            report.contradictory_globs.is_empty(),
            "expected no contradictory globs for disjoint patterns: {:?}",
            report.contradictory_globs
        );
    }

    #[test]
    fn test_contradictory_globs_witnessed_by_files() {
        // Patterns overlap only in specific corner — witnessed by actual file match.
        let specs = vec![
            make_spec("mod_a", Some("src/shared/*.ts")),
            make_spec("mod_b", Some("src/shared/utils.*")),
        ];
        let source = files(&["src/shared/utils.ts"]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(
            report.contradictory_globs.len(),
            1,
            "expected file-witnessed contradiction: {:?}",
            report.contradictory_globs
        );
    }

    #[test]
    fn test_contradictory_globs_same_module_not_reported() {
        // Two entries for the same module should not be reported as cross-module.
        let specs = vec![
            make_spec("auth", Some("src/auth/**")),
            make_spec("auth", Some("src/auth/internal/**")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert!(
            report.contradictory_globs.is_empty(),
            "same-module overlaps are not cross-module contradictions: {:?}",
            report.contradictory_globs
        );
    }

    #[test]
    fn test_contradictory_globs_deterministic_ordering() {
        let specs = vec![
            make_spec("z_module", Some("src/shared/**")),
            make_spec("a_module", Some("src/shared/**")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.contradictory_globs.len(), 1);
        let entry = &report.contradictory_globs[0];
        // Pair should be sorted: a_module before z_module.
        assert_eq!(entry.module_a, "a_module");
        assert_eq!(entry.module_b, "z_module");
    }

    #[test]
    fn test_contradictory_globs_multiple_pairs() {
        let specs = vec![
            make_spec("mod_a", Some("src/shared/**")),
            make_spec("mod_b", Some("src/shared/**")),
            make_spec("mod_c", Some("src/shared/**")),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        // 3 modules with identical patterns → 3 contradiction pairs (a-b, a-c, b-c).
        assert_eq!(
            report.contradictory_globs.len(),
            3,
            "expected 3 contradiction pairs: {:?}",
            report.contradictory_globs
        );
    }

    #[test]
    fn test_contradictory_globs_report_includes_spec_paths() {
        let specs = vec![
            make_spec_with_path(
                "api/core",
                Some("src/shared/**"),
                Some("/project/specs/api-core.spec.yml"),
            ),
            make_spec_with_path(
                "ui/shared",
                Some("src/shared/**"),
                Some("/project/specs/ui-shared.spec.yml"),
            ),
        ];
        let source = files(&[]);

        let report = validate_ownership(&root(), &specs, &source);

        assert_eq!(report.contradictory_globs.len(), 1);
        let entry = &report.contradictory_globs[0];
        assert_eq!(entry.spec_path_a, "specs/api-core.spec.yml");
        assert_eq!(entry.spec_path_b, "specs/ui-shared.spec.yml");
    }

    #[test]
    fn test_literal_prefix_extraction() {
        assert_eq!(super::literal_prefix("src/api/**"), "src/api/");
        assert_eq!(super::literal_prefix("**/*.ts"), "");
        assert_eq!(
            super::literal_prefix("src/shared/utils.*"),
            "src/shared/utils."
        );
        assert_eq!(super::literal_prefix("exact-path.ts"), "exact-path.ts");
        assert_eq!(super::literal_prefix(""), "");
    }

    #[test]
    fn test_globs_structurally_overlap_detection() {
        // One is a subtree of the other.
        assert!(super::globs_structurally_overlap("src/**", "src/api/**"));
        // Disjoint prefixes.
        assert!(!super::globs_structurally_overlap(
            "src/api/**",
            "src/ui/**"
        ));
        // Identical.
        assert!(super::globs_structurally_overlap(
            "src/shared/**",
            "src/shared/**"
        ));
        // Wildcard prefix matches everything.
        assert!(super::globs_structurally_overlap("**/*.ts", "src/api/**"));
    }
}
