mod canonical;
mod compare;
mod focus;
mod governance_consistency;
mod overview;
mod ownership;
mod parity;
mod trace_io;
mod trace_parser;
mod trace_types;
mod types;

// Re-export: used by cli/tests.rs for snapshot schema assertions
#[cfg(test)]
pub(crate) use trace_types::STRUCTURED_TRACE_SCHEMA_VERSION;

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::{CliRunResult, CommonProjectArgs};

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    #[command(subcommand)]
    command: Option<DoctorCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum DoctorCommand {
    /// Compare Specgate dependency edges with a configured trace source.
    Compare(DoctorCompareArgs),
    /// Validate module ownership: detect overlaps, unclaimed files, orphaned specs.
    Ownership(ownership::DoctorOwnershipArgs),
    /// Detect contradictory namespace-intent in spec governance configuration.
    GovernanceConsistency(governance_consistency::DoctorGovernanceConsistencyArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorCompareArgs {
    #[command(flatten)]
    pub(super) common: CommonProjectArgs,
    /// Parser mode for tsc parity payloads.
    #[arg(long, value_enum, default_value_t = DoctorCompareParserMode::Auto)]
    pub(super) parser_mode: DoctorCompareParserMode,
    /// Read structured trace snapshot JSON from this path.
    #[arg(long, conflicts_with_all = ["tsc_trace", "tsc_command"])]
    pub(super) structured_snapshot_in: Option<PathBuf>,
    /// Write normalized structured trace snapshot JSON to this path.
    #[arg(long)]
    pub(super) structured_snapshot_out: Option<PathBuf>,
    /// Trace payload file: JSON edge data or raw `tsc --traceResolution` output text.
    #[arg(long)]
    pub(super) tsc_trace: Option<PathBuf>,
    /// Command that emits compatible JSON to stdout.
    ///
    /// SECURITY: this command is executed through `sh -lc` and can run arbitrary shell code.
    /// You must also pass `--allow-shell` to opt into execution.
    #[arg(long)]
    pub(super) tsc_command: Option<String>,
    /// Explicit opt-in for running `--tsc-command` through the system shell.
    #[arg(long)]
    pub(super) allow_shell: bool,
    /// Resolve and compare a single import from a specific file.
    #[arg(long)]
    pub(super) from: Option<PathBuf>,
    /// Import specifier paired with `--from` for single-edge diagnostics.
    #[arg(long = "import")]
    pub(super) import_specifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum DoctorCompareParserMode {
    Auto,
    Structured,
    Legacy,
}

impl DoctorCompareParserMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Structured => "structured",
            Self::Legacy => "legacy",
        }
    }
}

pub(super) fn handle_doctor(args: DoctorArgs) -> CliRunResult {
    match args.command {
        Some(DoctorCommand::Compare(compare_args)) => compare::handle_doctor_compare(compare_args),
        Some(DoctorCommand::Ownership(ownership_args)) => {
            ownership::handle_doctor_ownership(ownership_args)
        }
        Some(DoctorCommand::GovernanceConsistency(gc_args)) => {
            governance_consistency::handle_doctor_governance_consistency(gc_args)
        }

        None => overview::handle_doctor_overview(args.common),
    }
}

#[cfg(test)]
mod tests;
