//! Validate command integration surface.
//!
//! This module provides types for the validate command.

use std::path::PathBuf;

use clap::Args;

/// Validate command arguments.
#[derive(Debug, Clone, Args)]
pub struct ValidateArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,
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
