use serde::Serialize;

/// JSON schema version for `PolicyDiffReport` output.
pub const POLICY_DIFF_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeClassification {
    Widening,
    Narrowing,
    Structural,
}

impl ChangeClassification {
    pub const fn deterministic_rank(self) -> u8 {
        match self {
            Self::Widening => 0,
            Self::Narrowing => 1,
            Self::Structural => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeScope {
    SpecFile,
    Boundaries,
    Constraint,
    Contract,
    ContractMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    pub module: String,
    pub spec_path: String,
    pub scope: ChangeScope,
    pub field: String,
    pub classification: ChangeClassification,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModulePolicyDiff {
    pub module: String,
    pub spec_path: String,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct PolicyDiffSummary {
    pub modules_changed: usize,
    pub widening_changes: usize,
    pub narrowing_changes: usize,
    pub structural_changes: usize,
    pub has_widening: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDiffReport {
    pub schema_version: String,
    pub base_ref: String,
    pub head_ref: String,
    pub diffs: Vec<ModulePolicyDiff>,
    pub summary: PolicyDiffSummary,
    pub errors: Vec<PolicyDiffErrorEntry>,
}

impl PolicyDiffReport {
    pub fn new(
        base_ref: String,
        head_ref: String,
        diffs: Vec<ModulePolicyDiff>,
        summary: PolicyDiffSummary,
        errors: Vec<PolicyDiffErrorEntry>,
    ) -> Self {
        Self {
            schema_version: POLICY_DIFF_SCHEMA_VERSION.to_string(),
            base_ref,
            head_ref,
            diffs,
            summary,
            errors,
        }
    }

    pub fn sort_deterministic(&mut self) {
        sort_module_policy_diffs_deterministic(&mut self.diffs);
        sort_policy_diff_errors_deterministic(&mut self.errors);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDiffErrorEntry {
    pub code: String,
    pub message: String,
    pub spec_path: Option<String>,
}

pub fn sort_field_changes_deterministic(changes: &mut [FieldChange]) {
    changes.sort_by(|a, b| {
        a.classification
            .deterministic_rank()
            .cmp(&b.classification.deterministic_rank())
            .then_with(|| a.field.cmp(&b.field))
            .then_with(|| a.detail.cmp(&b.detail))
            .then_with(|| a.module.cmp(&b.module))
            .then_with(|| a.spec_path.cmp(&b.spec_path))
    });
}

pub fn sort_module_policy_diffs_deterministic(diffs: &mut [ModulePolicyDiff]) {
    for diff in diffs.iter_mut() {
        sort_field_changes_deterministic(&mut diff.changes);
    }

    diffs.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.spec_path.cmp(&b.spec_path))
    });
}

pub fn sort_policy_diff_errors_deterministic(errors: &mut [PolicyDiffErrorEntry]) {
    errors.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.spec_path.cmp(&b.spec_path))
            .then_with(|| a.message.cmp(&b.message))
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDiffExit {
    Clean,
    Widening,
    RuntimeError,
}

impl PolicyDiffExit {
    pub const fn code(self) -> i32 {
        match self {
            Self::Clean => 0,
            Self::Widening => 1,
            Self::RuntimeError => 2,
        }
    }
}
