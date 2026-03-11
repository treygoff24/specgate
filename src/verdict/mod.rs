//! Verdict module for policy check results.

pub mod format;

/// Schema version for the verdict format itself.
pub const VERDICT_SCHEMA_VERSION: &str = "1.0";

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::deterministic::normalize_repo_relative;
use crate::graph::EdgeType;
use crate::spec::Severity;
use crate::spec::config::StaleBaselinePolicy;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspacePackageInfo {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tsconfig: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyViolation {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub from_file: PathBuf,
    pub to_file: Option<PathBuf>,
    pub from_module: Option<String>,
    pub to_module: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Expected value for structured diagnostics (e.g., allowed import patterns).
    pub expected: Option<String>,
    /// Actual value that violated the policy (e.g., the actual import found).
    pub actual: Option<String>,
    /// Human-readable hint on how to fix the violation.
    pub remediation_hint: Option<String>,
    /// Contract ID for contract-related violations.
    pub contract_id: Option<String>,
    /// Edge classification for unresolved import findings.
    pub edge_type: Option<EdgeType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationDisposition {
    New,
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintedViolation {
    pub violation: PolicyViolation,
    pub fingerprint: String,
    pub disposition: ViolationDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeClassification {
    pub resolved: usize,
    pub unresolved_literal: usize,
    pub unresolved_dynamic: usize,
    pub external: usize,
    pub type_only: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedEdge {
    pub from: String,
    pub specifier: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictSummary {
    #[serde(flatten)]
    pub core: AnonymizedTelemetrySummary,
    pub suppressed_violations: usize,
    pub error_violations: usize,
    pub warning_violations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictViolation {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub fingerprint: String,
    pub disposition: VerdictDisposition,
    pub from_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_module: Option<String>,
    pub to_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Expected value for structured diagnostics (e.g., allowed import patterns).
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Actual value that violated the policy (e.g., the actual import found).
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Human-readable hint on how to fix the violation.
    pub remediation_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Contract ID for contract-related violations.
    pub contract_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<EdgeType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictEdge {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_module: Option<String>,
    pub edge_type: EdgeType,
    pub import_path: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictDisposition {
    New,
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictMetrics {
    pub timings_ms: BTreeMap<String, u128>,
    pub total_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictGovernance {
    pub stale_baseline_policy: StaleBaselinePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryEventName {
    CheckCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnonymizedTelemetrySummary {
    pub total_violations: usize,
    pub new_violations: usize,
    pub baseline_violations: usize,
    pub new_error_violations: usize,
    pub new_warning_violations: usize,
    pub stale_baseline_entries: usize,
    pub expired_baseline_entries: usize,
}

impl From<VerdictSummary> for AnonymizedTelemetrySummary {
    fn from(summary: VerdictSummary) -> Self {
        summary.core
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnonymizedTelemetryEvent {
    pub schema_version: String,
    pub event: TelemetryEventName,
    /// Stable project-level hash supplied by the caller; never raw repo paths/names.
    pub project_fingerprint: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub status: VerdictStatus,
    pub summary: AnonymizedTelemetrySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VerdictBuildOptions {
    pub stale_baseline_policy: StaleBaselinePolicy,
    pub telemetry: Option<AnonymizedTelemetryEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictBuildRequest<'a> {
    pub project_root: &'a Path,
    pub violations: &'a [FingerprintedViolation],
    pub suppressed_violations: usize,
    pub metrics: Option<VerdictMetrics>,
    pub identity: VerdictIdentity,
    pub governance: GovernanceContext,
    pub options: VerdictBuildOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictIdentity {
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub output_mode: String,
    pub spec_files_changed: Vec<String>,
    pub rule_deltas: Vec<String>,
    pub policy_change_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub verdict_schema: String,
    pub schema_version: String,
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub output_mode: String,
    pub spec_files_changed: Vec<String>,
    pub rule_deltas: Vec<String>,
    pub policy_change_detected: bool,
    pub status: VerdictStatus,
    pub summary: VerdictSummary,
    pub violations: Vec<VerdictViolation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<VerdictMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governance: Option<VerdictGovernance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<AnonymizedTelemetryEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_packages: Option<Vec<WorkspacePackageInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_classification: Option<EdgeClassification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<VerdictEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_edges: Vec<UnresolvedEdge>,
}

/// Governance context for verdict building.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GovernanceContext {
    /// Number of baseline entries that no longer match any current violations.
    pub stale_baseline_entries: usize,
    /// Number of baseline entries that have passed their expiry date.
    pub expired_baseline_entries: usize,
    /// Rule deltas computed from spec changes in diff-aware mode.
    pub rule_deltas: Vec<String>,
    /// Whether policy changes were detected in diff-aware mode.
    pub policy_change_detected: bool,
}

pub fn build_verdict(
    project_root: &Path,
    violations: &[FingerprintedViolation],
    suppressed_violations: usize,
    metrics: Option<VerdictMetrics>,
    identity: VerdictIdentity,
) -> Verdict {
    build_verdict_from_request(VerdictBuildRequest {
        project_root,
        violations,
        suppressed_violations,
        metrics,
        identity,
        governance: GovernanceContext::default(),
        options: VerdictBuildOptions::default(),
    })
}

pub fn build_verdict_with_governance(
    project_root: &Path,
    violations: &[FingerprintedViolation],
    suppressed_violations: usize,
    metrics: Option<VerdictMetrics>,
    identity: VerdictIdentity,
    governance: GovernanceContext,
) -> Verdict {
    build_verdict_from_request(VerdictBuildRequest {
        project_root,
        violations,
        suppressed_violations,
        metrics,
        identity,
        governance,
        options: VerdictBuildOptions::default(),
    })
}

pub fn build_verdict_with_options(
    project_root: &Path,
    violations: &[FingerprintedViolation],
    suppressed_violations: usize,
    metrics: Option<VerdictMetrics>,
    identity: VerdictIdentity,
    governance: GovernanceContext,
    options: VerdictBuildOptions,
) -> Verdict {
    build_verdict_from_request(VerdictBuildRequest {
        project_root,
        violations,
        suppressed_violations,
        metrics,
        identity,
        governance,
        options,
    })
}

pub fn build_verdict_from_request(request: VerdictBuildRequest<'_>) -> Verdict {
    let VerdictBuildRequest {
        project_root,
        violations,
        suppressed_violations,
        metrics,
        identity,
        governance,
        options,
    } = request;

    let mut rendered = violations
        .iter()
        .map(|entry| render_violation(project_root, entry))
        .collect::<Vec<_>>();

    rendered.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| severity_rank(a.severity).cmp(&severity_rank(b.severity)))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
            .then_with(|| a.message.cmp(&b.message))
    });

    let summary = summarize(
        &rendered,
        suppressed_violations,
        governance.stale_baseline_entries,
        governance.expired_baseline_entries,
    );
    let fail_on_stale = options.stale_baseline_policy == StaleBaselinePolicy::Fail
        && summary.core.stale_baseline_entries > 0;
    let status = if summary.core.new_error_violations > 0 || fail_on_stale {
        VerdictStatus::Fail
    } else {
        VerdictStatus::Pass
    };

    // Always emit governance when stale entries exist so consumers can distinguish
    // "warn policy with stale entries" from "not evaluated / no stale entries".
    let governance_payload = if summary.core.stale_baseline_entries > 0 {
        Some(VerdictGovernance {
            stale_baseline_policy: options.stale_baseline_policy,
        })
    } else {
        None
    };

    Verdict {
        schema_version: "2.2".to_string(),
        verdict_schema: VERDICT_SCHEMA_VERSION.to_string(),
        tool_version: identity.tool_version,
        git_sha: identity.git_sha,
        config_hash: identity.config_hash,
        spec_hash: identity.spec_hash,
        output_mode: identity.output_mode,
        spec_files_changed: identity.spec_files_changed,
        rule_deltas: if governance.rule_deltas.is_empty() {
            identity.rule_deltas
        } else {
            governance.rule_deltas
        },
        policy_change_detected: governance.policy_change_detected
            || identity.policy_change_detected,
        status,
        summary,
        violations: rendered,
        metrics,
        governance: governance_payload,
        telemetry: options.telemetry,
        workspace_packages: None,
        edge_classification: None,
        edges: Vec::new(),
        unresolved_edges: Vec::new(),
    }
}

pub fn sort_policy_violations(violations: &mut [PolicyViolation]) {
    violations.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.from_module.cmp(&b.from_module))
            .then_with(|| a.to_module.cmp(&b.to_module))
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| severity_rank(a.severity).cmp(&severity_rank(b.severity)))
            .then_with(|| a.message.cmp(&b.message))
    });
}

fn render_violation(project_root: &Path, entry: &FingerprintedViolation) -> VerdictViolation {
    VerdictViolation {
        rule: entry.violation.rule.clone(),
        severity: entry.violation.severity,
        message: entry.violation.message.clone(),
        fingerprint: entry.fingerprint.clone(),
        disposition: match entry.disposition {
            ViolationDisposition::New => VerdictDisposition::New,
            ViolationDisposition::Baseline => VerdictDisposition::Baseline,
        },
        from_file: normalize_repo_relative(project_root, &entry.violation.from_file),
        to_file: entry
            .violation
            .to_file
            .as_ref()
            .map(|path| normalize_repo_relative(project_root, path)),
        from_module: entry.violation.from_module.clone(),
        to_module: entry.violation.to_module.clone(),
        line: entry.violation.line,
        column: entry.violation.column,
        expected: entry.violation.expected.clone(),
        actual: entry.violation.actual.clone(),
        remediation_hint: entry.violation.remediation_hint.clone(),
        contract_id: entry.violation.contract_id.clone(),
        edge_type: entry.violation.edge_type,
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn summarize(
    violations: &[VerdictViolation],
    suppressed_violations: usize,
    stale_baseline_entries: usize,
    expired_baseline_entries: usize,
) -> VerdictSummary {
    let total_violations = violations.len();
    let new_violations = violations
        .iter()
        .filter(|violation| matches!(violation.disposition, VerdictDisposition::New))
        .count();
    let baseline_violations = total_violations.saturating_sub(new_violations);

    let error_violations = violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .count();
    let warning_violations = total_violations.saturating_sub(error_violations);

    let new_error_violations = violations
        .iter()
        .filter(|violation| {
            matches!(violation.disposition, VerdictDisposition::New)
                && violation.severity == Severity::Error
        })
        .count();
    let new_warning_violations = new_violations.saturating_sub(new_error_violations);

    VerdictSummary {
        core: AnonymizedTelemetrySummary {
            total_violations,
            new_violations,
            baseline_violations,
            new_error_violations,
            new_warning_violations,
            stale_baseline_entries,
            expired_baseline_entries,
        },
        suppressed_violations,
        error_violations,
        warning_violations,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;

    // ---- EdgeClassification tests ----

    #[test]
    fn test_edge_classification_serialization() {
        let classification = EdgeClassification {
            resolved: 10,
            unresolved_literal: 2,
            unresolved_dynamic: 1,
            external: 5,
            type_only: 3,
        };
        let json = serde_json::to_string(&classification).expect("serialize");
        let restored: EdgeClassification = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, classification);
        assert!(json.contains("\"resolved\":10"));
        assert!(json.contains("\"unresolved_literal\":2"));
        assert!(json.contains("\"unresolved_dynamic\":1"));
        assert!(json.contains("\"external\":5"));
        assert!(json.contains("\"type_only\":3"));
    }

    #[test]
    fn test_unresolved_edge_serialization_with_line() {
        let edge = UnresolvedEdge {
            from: "src/a.ts".to_string(),
            specifier: "./missing".to_string(),
            kind: "unresolved_literal".to_string(),
            line: Some(42),
        };
        let json = serde_json::to_string(&edge).expect("serialize");
        let restored: UnresolvedEdge = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored, edge);
        assert!(json.contains("\"line\":42"));
    }

    #[test]
    fn test_unresolved_edge_serialization_omits_line_when_none() {
        let edge = UnresolvedEdge {
            from: "src/a.ts".to_string(),
            specifier: "./missing".to_string(),
            kind: "unresolved_dynamic".to_string(),
            line: None,
        };
        let json = serde_json::to_string(&edge).expect("serialize");
        assert!(!json.contains("\"line\""), "line should be omitted: {json}");
    }

    #[test]
    fn test_verdict_with_edge_classification() {
        let entries: Vec<FingerprintedViolation> = vec![];
        let mut verdict =
            build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        verdict.edge_classification = Some(EdgeClassification {
            resolved: 5,
            unresolved_literal: 1,
            unresolved_dynamic: 0,
            external: 2,
            type_only: 1,
        });
        let json = serde_json::to_string(&verdict).expect("serialize");
        assert!(
            json.contains("\"edge_classification\""),
            "should include edge_classification: {json}"
        );
        assert!(json.contains("\"resolved\":5"));
    }

    #[test]
    fn test_verdict_without_edge_classification() {
        let entries: Vec<FingerprintedViolation> = vec![];
        let verdict = build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        let json = serde_json::to_string(&verdict).expect("serialize");
        assert!(
            !json.contains("\"edge_classification\""),
            "should omit edge_classification when None: {json}"
        );
    }

    #[test]
    fn test_verdict_with_unresolved_edges() {
        let entries: Vec<FingerprintedViolation> = vec![];
        let mut verdict =
            build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        verdict.unresolved_edges = vec![UnresolvedEdge {
            from: "src/a.ts".to_string(),
            specifier: "./missing".to_string(),
            kind: "unresolved_literal".to_string(),
            line: Some(5),
        }];
        let json = serde_json::to_string(&verdict).expect("serialize");
        assert!(
            json.contains("\"unresolved_edges\""),
            "should include unresolved_edges: {json}"
        );
        assert!(json.contains("\"src/a.ts\""));
    }

    #[test]
    fn test_verdict_without_unresolved_edges_omits_field() {
        let entries: Vec<FingerprintedViolation> = vec![];
        let verdict = build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        let json = serde_json::to_string(&verdict).expect("serialize");
        assert!(
            !json.contains("\"unresolved_edges\""),
            "should omit unresolved_edges when empty: {json}"
        );
    }

    #[test]
    fn test_edge_classification_counts_are_correct() {
        // Verify zero-value classification serializes correctly
        let classification = EdgeClassification {
            resolved: 0,
            unresolved_literal: 0,
            unresolved_dynamic: 0,
            external: 0,
            type_only: 0,
        };
        let json = serde_json::to_string(&classification).expect("serialize");
        let restored: EdgeClassification = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.resolved, 0);
        assert_eq!(restored.unresolved_literal, 0);
        assert_eq!(restored.unresolved_dynamic, 0);
        assert_eq!(restored.external, 0);
        assert_eq!(restored.type_only, 0);
    }

    fn violation(
        rule: &str,
        severity: Severity,
        from_file: &str,
        message: &str,
    ) -> PolicyViolation {
        PolicyViolation {
            rule: rule.to_string(),
            severity,
            message: message.to_string(),
            from_file: PathBuf::from(from_file),
            to_file: None,
            from_module: Some("app".to_string()),
            to_module: Some("core".to_string()),
            line: Some(1),
            column: Some(0),
            expected: None,
            actual: None,
            remediation_hint: None,
            contract_id: None,
            edge_type: None,
        }
    }

    fn identity(output_mode: &str) -> VerdictIdentity {
        VerdictIdentity {
            tool_version: "0.1.0".to_string(),
            git_sha: "abc123".to_string(),
            config_hash: "sha256:config".to_string(),
            spec_hash: "sha256:spec".to_string(),
            output_mode: output_mode.to_string(),
            spec_files_changed: Vec::new(),
            rule_deltas: Vec::new(),
            policy_change_detected: false,
        }
    }

    #[test]
    fn verdict_status_fails_on_new_error_only() {
        let entries = vec![
            FingerprintedViolation {
                violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
                fingerprint: "sha256:a".to_string(),
                disposition: ViolationDisposition::Baseline,
            },
            FingerprintedViolation {
                violation: violation("boundary.public_api", Severity::Warning, "src/b.ts", "warn"),
                fingerprint: "sha256:b".to_string(),
                disposition: ViolationDisposition::New,
            },
        ];

        let verdict = build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        assert_eq!(verdict.status, VerdictStatus::Pass);
        assert_eq!(verdict.summary.core.new_warning_violations, 1);

        let mut entries_with_error = entries;
        entries_with_error.push(FingerprintedViolation {
            violation: violation("dependency.not_allowed", Severity::Error, "src/c.ts", "bad"),
            fingerprint: "sha256:c".to_string(),
            disposition: ViolationDisposition::New,
        });

        let failing = build_verdict(
            Path::new("."),
            &entries_with_error,
            0,
            None,
            identity("deterministic"),
        );
        assert_eq!(failing.status, VerdictStatus::Fail);
        assert_eq!(failing.summary.core.new_error_violations, 1);
    }

    #[test]
    fn deterministic_json_omits_metrics_by_default() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let verdict = build_verdict(Path::new("."), &entries, 2, None, identity("deterministic"));
        let rendered = serde_json::to_string(&verdict).expect("serialize");

        assert_eq!(verdict.verdict_schema, VERDICT_SCHEMA_VERSION);
        assert!(!rendered.contains("metrics"));
        assert!(rendered.contains("suppressed_violations"));
        assert!(rendered.contains("\"verdict_schema\":\"1.0\""));
        assert!(rendered.contains("\"tool_version\""));
        assert!(rendered.contains("\"config_hash\""));
        assert!(rendered.contains("\"spec_hash\""));
        assert!(rendered.contains("\"output_mode\":\"deterministic\""));
        assert!(rendered.contains("\"spec_files_changed\":[]"));
        assert!(rendered.contains("\"rule_deltas\":[]"));
        assert!(rendered.contains("\"policy_change_detected\":false"));
        assert!(!rendered.contains("\"governance\""));
        assert!(!rendered.contains("\"telemetry\""));
    }

    #[test]
    fn canonical_request_builder_preserves_wrapper_json_shape() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let wrapper_verdict = build_verdict_with_options(
            Path::new("."),
            &entries,
            2,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 1,
                expired_baseline_entries: 0,
                rule_deltas: vec!["boundary.never_imports:added".to_string()],
                policy_change_detected: true,
            },
            VerdictBuildOptions {
                stale_baseline_policy: StaleBaselinePolicy::Warn,
                telemetry: None,
            },
        );

        let request_verdict = build_verdict_from_request(VerdictBuildRequest {
            project_root: Path::new("."),
            violations: &entries,
            suppressed_violations: 2,
            metrics: None,
            identity: identity("deterministic"),
            governance: GovernanceContext {
                stale_baseline_entries: 1,
                expired_baseline_entries: 0,
                rule_deltas: vec!["boundary.never_imports:added".to_string()],
                policy_change_detected: true,
            },
            options: VerdictBuildOptions {
                stale_baseline_policy: StaleBaselinePolicy::Warn,
                telemetry: None,
            },
        });

        let wrapper_json = serde_json::to_value(&wrapper_verdict).expect("serialize wrapper");
        let request_json = serde_json::to_value(&request_verdict).expect("serialize request");

        assert_eq!(request_verdict, wrapper_verdict);
        assert_eq!(request_json, wrapper_json);
    }

    #[test]
    fn metrics_mode_is_serialized_when_present() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let mut timings = BTreeMap::new();
        timings.insert("build_graph".to_string(), 12);

        let verdict = build_verdict(
            Path::new("."),
            &entries,
            0,
            Some(VerdictMetrics {
                timings_ms: timings,
                total_ms: 20,
            }),
            identity("metrics"),
        );

        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("\"verdict_schema\":\"1.0\""));
        assert!(rendered.contains("metrics"));
        assert!(rendered.contains("build_graph"));
        assert!(rendered.contains("\"output_mode\":\"metrics\""));
    }

    #[test]
    fn stale_baseline_entries_included_in_summary() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let verdict = build_verdict_with_governance(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 5,
                expired_baseline_entries: 0,
                rule_deltas: Vec::new(),
                policy_change_detected: false,
            },
        );

        assert_eq!(verdict.summary.core.stale_baseline_entries, 5);
        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("\"stale_baseline_entries\":5"));
    }

    #[test]
    fn governance_context_propagates_rule_deltas() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let rule_deltas = vec![
            "boundary.never_imports:added".to_string(),
            "dependency.forbidden:removed".to_string(),
        ];

        let verdict = build_verdict_with_governance(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 0,
                expired_baseline_entries: 0,
                rule_deltas: rule_deltas.clone(),
                policy_change_detected: true,
            },
        );

        assert_eq!(verdict.rule_deltas, rule_deltas);
        assert!(verdict.policy_change_detected);
        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("\"boundary.never_imports:added\""));
        assert!(rendered.contains("\"policy_change_detected\":true"));
    }

    #[test]
    fn stale_baseline_policy_fail_flips_status_without_new_errors() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.public_api", Severity::Warning, "src/a.ts", "warn"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let warn_verdict = build_verdict_with_governance(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 2,
                expired_baseline_entries: 0,
                rule_deltas: Vec::new(),
                policy_change_detected: false,
            },
        );
        assert_eq!(warn_verdict.status, VerdictStatus::Pass);

        let fail_verdict = build_verdict_with_options(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 2,
                expired_baseline_entries: 0,
                rule_deltas: Vec::new(),
                policy_change_detected: false,
            },
            VerdictBuildOptions {
                stale_baseline_policy: StaleBaselinePolicy::Fail,
                telemetry: None,
            },
        );

        assert_eq!(fail_verdict.status, VerdictStatus::Fail);
        assert_eq!(
            fail_verdict.governance,
            Some(VerdictGovernance {
                stale_baseline_policy: StaleBaselinePolicy::Fail,
            })
        );
        let rendered = serde_json::to_string(&fail_verdict).expect("serialize");
        assert!(rendered.contains("\"stale_baseline_policy\":\"fail\""));
    }

    #[test]
    fn test_expired_baseline_count_in_governance() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let verdict = build_verdict_with_governance(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext {
                stale_baseline_entries: 0,
                expired_baseline_entries: 3,
                rule_deltas: Vec::new(),
                policy_change_detected: false,
            },
        );

        assert_eq!(verdict.summary.core.expired_baseline_entries, 3);
        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("\"expired_baseline_entries\":3"));
    }

    #[test]
    fn verdict_optionally_serializes_anonymized_telemetry_payload() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let telemetry = AnonymizedTelemetryEvent {
            schema_version: "1".to_string(),
            event: TelemetryEventName::CheckCompleted,
            project_fingerprint: "sha256:project".to_string(),
            config_hash: "sha256:config".to_string(),
            spec_hash: "sha256:spec".to_string(),
            status: VerdictStatus::Fail,
            summary: AnonymizedTelemetrySummary {
                total_violations: 1,
                new_violations: 1,
                baseline_violations: 0,
                new_error_violations: 1,
                new_warning_violations: 0,
                stale_baseline_entries: 0,
                expired_baseline_entries: 0,
            },
        };

        let verdict = build_verdict_with_options(
            Path::new("."),
            &entries,
            0,
            None,
            identity("deterministic"),
            GovernanceContext::default(),
            VerdictBuildOptions {
                stale_baseline_policy: StaleBaselinePolicy::Warn,
                telemetry: Some(telemetry.clone()),
            },
        );

        assert_eq!(verdict.telemetry, Some(telemetry));
        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("\"telemetry\""));
        assert!(rendered.contains("\"event\":\"check_completed\""));
        assert!(rendered.contains("\"project_fingerprint\":\"sha256:project\""));
    }
}
