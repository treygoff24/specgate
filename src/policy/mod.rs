pub mod classify;
pub mod git;
pub mod render;
pub mod types;

pub use classify::{
    classify_fail_closed_operations, classify_spec_snapshot_pair,
    classify_spec_snapshot_pair_with_path_coverage, classify_spec_snapshot_pairs,
    classify_spec_snapshot_pairs_with_path_coverage,
};
pub use git::{
    DiscoveredAndLoadedSpecSnapshots, DiscoveredSpecFileChanges, FailClosedSpecOperation,
    LoadedSpecSnapshots, PolicyGitError, SpecSnapshotPair, discover_and_load_spec_snapshots,
    discover_spec_file_changes, list_tracked_files_scoped, load_spec_snapshots_for_changed_paths,
    parse_name_status_z,
};
pub use render::{render_policy_diff_human, render_policy_diff_json, render_policy_diff_ndjson};
pub use types::{
    ChangeClassification, ChangeScope, FieldChange, ModulePolicyDiff, POLICY_DIFF_SCHEMA_VERSION,
    PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport, PolicyDiffSummary,
    sort_field_changes_deterministic, sort_module_policy_diffs_deterministic,
    sort_policy_diff_errors_deterministic,
};

#[cfg(test)]
mod tests;
