pub mod git;
pub mod types;

pub use git::{
    DiscoveredSpecFileChanges, FailClosedSpecOperation, PolicyGitError, discover_spec_file_changes,
    parse_name_status_z,
};
pub use types::{
    ChangeClassification, ChangeScope, FieldChange, ModulePolicyDiff, POLICY_DIFF_SCHEMA_VERSION,
    PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport, PolicyDiffSummary,
    sort_field_changes_deterministic, sort_module_policy_diffs_deterministic,
    sort_policy_diff_errors_deterministic,
};

#[cfg(test)]
mod tests;
