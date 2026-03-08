use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::resolver::ModuleMapOverlap;
use crate::spec::{SpecConfig, SpecFile, ValidationReport};
use crate::verdict::PolicyViolation;

pub const EXIT_CODE_PASS: i32 = 0;
pub const EXIT_CODE_POLICY_VIOLATIONS: i32 = 1;
pub const EXIT_CODE_RUNTIME_ERROR: i32 = 2;
pub const EXIT_CODE_DOCTOR_MISMATCH: i32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliRunResult {
    pub(crate) fn json<T: Serialize>(exit_code: i32, payload: &T) -> Self {
        match serde_json::to_string_pretty(payload) {
            Ok(json) => Self {
                exit_code,
                stdout: format!("{json}\n"),
                stderr: String::new(),
            },
            Err(error) => Self {
                exit_code: EXIT_CODE_RUNTIME_ERROR,
                stdout: String::new(),
                stderr: format!("failed to serialize CLI JSON output: {error}\n"),
            },
        }
    }

    pub(crate) fn clap_error(error: clap::Error) -> Self {
        Self {
            exit_code: error.exit_code(),
            stdout: String::new(),
            stderr: format!("{error}"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedProject {
    pub(crate) project_root: PathBuf,
    pub(crate) config: SpecConfig,
    pub(crate) specs: Vec<SpecFile>,
    pub(crate) validation: ValidationReport,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalysisArtifacts {
    pub(crate) policy_violations: Vec<PolicyViolation>,
    pub(crate) layer_config_issues: Vec<String>,
    pub(crate) module_map_overlaps: Vec<ModuleMapOverlap>,
    pub(crate) parse_warning_count: usize,
    pub(crate) graph_nodes: usize,
    pub(crate) graph_edges: usize,
    pub(crate) suppressed_violations: usize,
    pub(crate) edge_pairs: BTreeSet<(String, String)>,
}

#[derive(Debug, Clone)]
pub(crate) struct GovernanceHashes {
    pub(crate) config_hash: String,
    pub(crate) spec_hash: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorOutput {
    pub(crate) schema_version: String,
    pub(crate) status: String,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ValidateOutput {
    pub(crate) schema_version: String,
    pub(crate) status: String,
    pub(crate) spec_count: usize,
    pub(crate) error_count: usize,
    pub(crate) warning_count: usize,
    pub(crate) issues: Vec<ValidateIssueOutput>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ValidateIssueOutput {
    pub(crate) level: String,
    pub(crate) module: String,
    pub(crate) message: String,
    pub(crate) spec_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BaselineOutput {
    pub(crate) schema_version: String,
    pub(crate) status: String,
    pub(crate) baseline_path: String,
    pub(crate) entry_count: usize,
    pub(crate) source_violation_count: usize,
    pub(crate) refreshed: bool,
    pub(crate) stale_entries_pruned: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitOutput {
    pub(crate) schema_version: String,
    pub(crate) status: String,
    pub(crate) project_root: String,
    pub(crate) created: Vec<String>,
    pub(crate) skipped_existing: Vec<String>,
}
