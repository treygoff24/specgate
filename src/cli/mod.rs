//! CLI module for Specgate.
//!
//! This module provides the command-line interface with subcommands for
//! check, validate, init, baseline, and doctor operations.

mod analysis;
mod baseline_cmd;
mod blast;
pub mod check;
mod doctor;
pub mod init;
mod project;
mod severity;
pub mod types;
mod util;
pub mod validate;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::baseline::{
    BaselineGeneratedFrom, DEFAULT_BASELINE_PATH, build_baseline_with_metadata,
    classify_violations_with_stale, load_optional_baseline, refresh_baseline_with_metadata,
    write_baseline,
};
use crate::build_info;
use crate::deterministic::{normalize_path, normalize_repo_relative, stable_hash_hex};
use crate::graph::DependencyGraph;
use crate::resolver::classify::extract_package_name;
use crate::resolver::nearest_tsconfig_for_dir_uncached;
use crate::resolver::{ModuleResolver, ModuleResolverOptions, ResolvedImport};
use crate::rules::boundary::evaluate_boundary_rules;
use crate::rules::{
    DEPENDENCY_FORBIDDEN_RULE_ID, DEPENDENCY_NOT_ALLOWED_RULE_ID, DependencyRule, RuleContext,
    RuleWithResolver, evaluate_enforce_layer, evaluate_no_circular_deps,
    is_canonical_import_rule_id,
};
use crate::spec::config::{ReleaseChannel, StaleBaselinePolicy};
use crate::spec::{
    self, Severity, SpecConfig, ValidationLevel,
    workspace_discovery::discover_workspace_packages_with_config,
};
use crate::verdict::{
    self, AnonymizedTelemetryEvent, AnonymizedTelemetrySummary, GovernanceContext, PolicyViolation,
    TelemetryEventName, VerdictBuildOptions, VerdictIdentity, VerdictMetrics, VerdictStatus,
    WorkspacePackageInfo, build_verdict_with_options,
};

// Re-export from submodules for convenience
pub(crate) use analysis::*;
pub(crate) use baseline_cmd::*;
pub(crate) use blast::*;
pub use check::{CheckArgs, CheckOutputMode, DiffMode, OutputFormat};
pub(crate) use doctor::*;
pub use init::InitArgs as InitArgsEnhanced;
pub(crate) use project::*;
pub(crate) use severity::*;
pub use types::*;
pub(crate) use util::*;
pub use validate::ValidateArgs;

#[derive(Debug, Parser)]
#[command(name = "specgate")]
#[command(version)]
#[command(about = "Machine-checkable architectural intent for TypeScript projects")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the full policy pipeline and emit verdict JSON.
    Check(CheckArgs),
    /// Validate spec/config files only.
    Validate(CommonProjectArgs),
    /// Initialize starter config/spec scaffolding.
    Init(init::InitArgs),
    /// Diagnostics and parity checks.
    Doctor(DoctorArgs),
    /// Generate a baseline file for current violations.
    Baseline(BaselineArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct CommonProjectArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
}

pub fn run<I, T>(args: I) -> CliRunResult
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return CliRunResult::clap_error(error),
    };

    match cli.command {
        Command::Validate(args) => validate::handle_validate(args),
        Command::Check(args) => check::handle_check(args),
        Command::Init(args) => init::handle_init(args),
        Command::Baseline(args) => baseline_cmd::handle_baseline(args),
        Command::Doctor(args) => doctor::handle_doctor(args),
    }
}

#[cfg(test)]
mod tests;
