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
    /// Cross-file compensation candidates (populated when --cross-file-compensation is active).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compensations: Vec<CompensationCandidate>,
    /// Net classification after compensation. Always populated (defaults to summary-derived).
    pub net_classification: ChangeClassification,
    /// Config-level governance changes (populated by config diffing — see Task 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_changes: Vec<ConfigFieldChange>,
}

impl PolicyDiffReport {
    pub fn new(
        base_ref: String,
        head_ref: String,
        diffs: Vec<ModulePolicyDiff>,
        summary: PolicyDiffSummary,
        errors: Vec<PolicyDiffErrorEntry>,
    ) -> Self {
        let net_classification = if summary.has_widening {
            ChangeClassification::Widening
        } else if summary.narrowing_changes > 0 {
            ChangeClassification::Narrowing
        } else {
            ChangeClassification::Structural
        };
        Self {
            schema_version: POLICY_DIFF_SCHEMA_VERSION.to_string(),
            base_ref,
            head_ref,
            diffs,
            summary,
            errors,
            compensations: Vec::new(),
            net_classification,
            config_changes: Vec::new(),
        }
    }

    pub fn sort_deterministic(&mut self) {
        sort_module_policy_diffs_deterministic(&mut self.diffs);
        sort_policy_diff_errors_deterministic(&mut self.errors);
        sort_compensation_candidates_deterministic(&mut self.compensations);
        sort_config_field_changes_deterministic(&mut self.config_changes);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyDiffErrorEntry {
    pub code: String,
    pub message: String,
    pub spec_path: Option<String>,
}

/// A typed dependency edge between two modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyEdge {
    /// Module that imports (has `allow_imports_from` listing the provider).
    pub importer: String,
    /// Module being imported from.
    pub provider: String,
}

/// Result of attempting cross-file compensation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationResult {
    /// Narrowing fully offsets the widening.
    Offset,
    /// Narrowing partially offsets (e.g., different cardinality).
    Partial,
    /// Multiple candidates — fail closed, no compensation applied.
    Ambiguous,
}

/// A candidate pairing of a widening with a narrowing for cross-file compensation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompensationCandidate {
    pub widening: FieldChange,
    pub narrowing: FieldChange,
    pub relationship: DependencyEdge,
    pub result: CompensationResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigFieldChange {
    pub field_path: String,
    pub classification: ChangeClassification,
    pub before: String,
    pub after: String,
}

pub fn sort_compensation_candidates_deterministic(candidates: &mut [CompensationCandidate]) {
    candidates.sort_by(|a, b| {
        compensation_result_rank(a.result)
            .cmp(&compensation_result_rank(b.result))
            .then_with(|| a.widening.module.cmp(&b.widening.module))
            .then_with(|| a.widening.field.cmp(&b.widening.field))
            .then_with(|| a.narrowing.module.cmp(&b.narrowing.module))
            .then_with(|| a.narrowing.field.cmp(&b.narrowing.field))
            .then_with(|| a.relationship.importer.cmp(&b.relationship.importer))
            .then_with(|| a.relationship.provider.cmp(&b.relationship.provider))
    });
}

pub fn sort_config_field_changes_deterministic(changes: &mut [ConfigFieldChange]) {
    changes.sort_by(|a, b| {
        a.field_path
            .cmp(&b.field_path)
            .then_with(|| {
                a.classification
                    .deterministic_rank()
                    .cmp(&b.classification.deterministic_rank())
            })
            .then_with(|| a.before.cmp(&b.before))
            .then_with(|| a.after.cmp(&b.after))
    });
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

const fn compensation_result_rank(result: CompensationResult) -> u8 {
    match result {
        CompensationResult::Offset => 0,
        CompensationResult::Partial => 1,
        CompensationResult::Ambiguous => 2,
    }
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
