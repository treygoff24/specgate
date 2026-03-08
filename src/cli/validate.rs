//! Validate command integration surface.
//!
//! This module provides types for the validate command.

use std::path::PathBuf;

use super::*;
use clap::Args;

/// Validate command arguments.
#[derive(Debug, Clone, Args)]
pub struct ValidateArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,
}

pub(super) fn handle_validate(args: CommonProjectArgs) -> CliRunResult {
    let loaded = match load_project(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    let mut issues = loaded
        .validation
        .issues
        .iter()
        .map(|issue| ValidateIssueOutput {
            level: match issue.level {
                ValidationLevel::Error => "error".to_string(),
                ValidationLevel::Warning => "warning".to_string(),
            },
            module: issue.module.clone(),
            message: issue.message.clone(),
            spec_path: issue
                .spec_path
                .as_ref()
                .map(|path| normalize_repo_relative(&loaded.project_root, path)),
        })
        .collect::<Vec<_>>();

    issues.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.level.cmp(&b.level))
            .then_with(|| a.spec_path.cmp(&b.spec_path))
            .then_with(|| a.message.cmp(&b.message))
    });

    let error_count = loaded.validation.errors().len();
    let warning_count = loaded.validation.warnings().len();

    let output = ValidateOutput {
        schema_version: "2.2".to_string(),
        status: if error_count == 0 {
            "ok".to_string()
        } else {
            "error".to_string()
        },
        spec_count: loaded.specs.len(),
        error_count,
        warning_count,
        issues,
    };

    let exit_code = if error_count == 0 {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_RUNTIME_ERROR
    };

    CliRunResult::json(exit_code, &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_args_has_project_root() {
        let args = ValidateArgs {
            project_root: PathBuf::from("."),
        };

        assert_eq!(args.project_root, PathBuf::from("."));
    }
}
