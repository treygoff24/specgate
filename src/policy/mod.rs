use std::collections::BTreeSet;
use std::path::Path;

pub mod classify;
pub mod compensate;
pub mod config_diff;
pub mod git;
pub mod render;
pub mod types;

pub use classify::{
    classify_fail_closed_operations, classify_spec_snapshot_pair,
    classify_spec_snapshot_pair_with_path_coverage, classify_spec_snapshot_pairs,
    classify_spec_snapshot_pairs_with_path_coverage,
};
pub use compensate::{dependency_edges_from_specs, find_compensation_candidates};
pub use config_diff::classify_config_changes;
pub use git::{
    DiscoveredAndLoadedSpecSnapshots, DiscoveredSpecFileChanges, FailClosedSpecOperation,
    LoadedSpecSnapshots, PolicyGitError, SpecSnapshotPair, discover_and_load_spec_snapshots,
    discover_config_changes, discover_spec_file_changes, list_tracked_files_scoped,
    load_config_from_ref, load_spec_snapshots_for_changed_paths, parse_name_status_z,
};
pub use render::{render_policy_diff_human, render_policy_diff_json, render_policy_diff_ndjson};
pub use types::PolicyDiffExit as PolicyDiffExitType;
pub use types::{
    ChangeClassification, ChangeScope, CompensationCandidate, CompensationResult,
    ConfigFieldChange, DependencyEdge, FieldChange, ModulePolicyDiff, POLICY_DIFF_SCHEMA_VERSION,
    PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport, PolicyDiffSummary,
    sort_compensation_candidates_deterministic, sort_config_field_changes_deterministic,
    sort_field_changes_deterministic, sort_module_policy_diffs_deterministic,
    sort_policy_diff_errors_deterministic,
};

pub fn build_policy_diff_report(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<PolicyDiffReport, PolicyGitError> {
    let discovered = discover_and_load_spec_snapshots(project_root, base_ref, head_ref)?;

    let mut diffs: Vec<ModulePolicyDiff> = Vec::new();
    diffs.extend(classify_fail_closed_operations(
        &discovered.discovered.fail_closed_operations,
    ));

    let mut semantic_diffs = classify_spec_snapshot_pairs_with_path_coverage(
        project_root,
        base_ref,
        head_ref,
        &discovered.loaded.snapshots,
    );
    diffs.append(&mut semantic_diffs);

    let mut summary = summarize_policy_diffs(&diffs);
    if !discovered.loaded.errors.is_empty() {
        diffs.clear();
        summary = PolicyDiffSummary::default();
    }

    let config_changes = if discovered.loaded.errors.is_empty() {
        discover_config_changes(project_root, base_ref, head_ref)?
    } else {
        Vec::new()
    };

    let mut report = PolicyDiffReport::new(
        base_ref.to_string(),
        head_ref.to_string(),
        diffs,
        summary,
        discovered.loaded.errors,
    );
    report.config_changes = config_changes;
    apply_config_changes_to_summary(&mut report.summary, &report.config_changes);
    report.net_classification = derive_net_classification(&report.summary);
    report.sort_deterministic();
    Ok(report)
}

pub fn derive_policy_diff_exit(report: &PolicyDiffReport) -> PolicyDiffExit {
    if !report.errors.is_empty() {
        PolicyDiffExit::RuntimeError
    } else if report.net_classification == ChangeClassification::Widening {
        PolicyDiffExit::Widening
    } else {
        PolicyDiffExit::Clean
    }
}

fn apply_config_changes_to_summary(
    summary: &mut PolicyDiffSummary,
    config_changes: &[ConfigFieldChange],
) {
    for change in config_changes {
        match change.classification {
            ChangeClassification::Widening => {
                summary.widening_changes += 1;
                summary.has_widening = true;
            }
            ChangeClassification::Narrowing => {
                summary.narrowing_changes += 1;
            }
            ChangeClassification::Structural => {
                summary.structural_changes += 1;
            }
        }
    }
}

fn derive_net_classification(summary: &PolicyDiffSummary) -> ChangeClassification {
    if summary.has_widening {
        ChangeClassification::Widening
    } else if summary.narrowing_changes > 0 {
        ChangeClassification::Narrowing
    } else {
        ChangeClassification::Structural
    }
}

fn summarize_policy_diffs(diffs: &[ModulePolicyDiff]) -> PolicyDiffSummary {
    let mut summary = PolicyDiffSummary::default();
    let mut modules = BTreeSet::new();
    let mut limitations = BTreeSet::new();

    for diff in diffs {
        modules.insert(diff.module.clone());

        for change in &diff.changes {
            match change.classification {
                ChangeClassification::Widening => {
                    summary.widening_changes += 1;
                    summary.has_widening = true;
                }
                ChangeClassification::Narrowing => {
                    summary.narrowing_changes += 1;
                }
                ChangeClassification::Structural => {
                    summary.structural_changes += 1;
                }
            }

            if change.detail.contains("path_coverage_unbounded_mvp") {
                limitations.insert("path_coverage_unbounded_mvp".to_string());
            }
        }
    }

    summary.modules_changed = modules.len();
    summary.limitations = limitations.into_iter().collect();
    summary
}

/// Options for controlling policy diff behavior.
#[derive(Debug, Clone, Default)]
pub struct PolicyDiffOptions {
    /// Enable cross-file compensation analysis (scoped to directly-connected modules).
    pub cross_file_compensation: bool,
}

/// Build a policy diff report with configurable options.
pub fn build_policy_diff_report_with_options(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    options: &PolicyDiffOptions,
) -> Result<PolicyDiffReport, PolicyGitError> {
    let mut report = build_policy_diff_report(project_root, base_ref, head_ref)?;

    if options.cross_file_compensation {
        apply_compensation(&mut report, project_root, head_ref)?;
    }

    Ok(report)
}

/// Apply cross-file compensation analysis to a policy diff report.
fn apply_compensation(
    report: &mut PolicyDiffReport,
    project_root: &Path,
    head_ref: &str,
) -> Result<(), PolicyGitError> {
    // Load HEAD specs to extract dependency edges
    let head_specs = git::load_spec_snapshots_for_ref(project_root, head_ref)?;
    let edges = compensate::dependency_edges_from_specs(&head_specs);

    // Separate widenings and narrowings from all diffs
    let mut widenings: Vec<FieldChange> = Vec::new();
    let mut narrowings: Vec<FieldChange> = Vec::new();

    for diff in &report.diffs {
        for change in &diff.changes {
            match change.classification {
                ChangeClassification::Widening => widenings.push(change.clone()),
                ChangeClassification::Narrowing => narrowings.push(change.clone()),
                _ => {}
            }
        }
    }

    // Find compensation candidates
    let candidates = compensate::find_compensation_candidates(&widenings, &narrowings, &edges);
    report.compensations = candidates;

    // Recompute net classification
    report.net_classification = derive_net_classification_with_compensation(report);

    Ok(())
}

/// Derive net classification accounting for compensation.
fn derive_net_classification_with_compensation(report: &PolicyDiffReport) -> ChangeClassification {
    let offset_widenings: std::collections::BTreeSet<(String, String)> = report
        .compensations
        .iter()
        .filter(|c| c.result == CompensationResult::Offset)
        .map(|c| (c.widening.module.clone(), c.widening.field.clone()))
        .collect();

    let has_uncompensated_spec_widening = report.diffs.iter().flat_map(|diff| diff.changes.iter()).any(
        |change| {
            change.classification == ChangeClassification::Widening
                && !offset_widenings.contains(&(change.module.clone(), change.field.clone()))
        },
    );
    let has_config_widening = report
        .config_changes
        .iter()
        .any(|change| change.classification == ChangeClassification::Widening);

    if has_uncompensated_spec_widening || has_config_widening {
        ChangeClassification::Widening
    } else if report.summary.narrowing_changes > 0 {
        ChangeClassification::Narrowing
    } else {
        ChangeClassification::Structural
    }
}

#[cfg(test)]
mod tests;
