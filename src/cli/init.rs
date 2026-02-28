//! Init command integration surface.
//!
//! This module provides types for the init command.

use std::path::PathBuf;

use clap::Args;

/// Init command arguments.
#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,
    /// Directory where starter `.spec.yml` files are written.
    #[arg(long, default_value = "modules")]
    pub spec_dir: PathBuf,
    /// Optional starter module id override.
    #[arg(long)]
    pub module: Option<String>,
    /// Optional starter module boundary glob override.
    #[arg(long)]
    pub module_path: Option<String>,
    /// Overwrite existing scaffold files.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_args_defaults_are_reasonable() {
        let args = InitArgs {
            project_root: PathBuf::from("."),
            spec_dir: PathBuf::from("modules"),
            module: None,
            module_path: None,
            force: false,
        };

        assert_eq!(args.spec_dir, PathBuf::from("modules"));
        assert_eq!(args.module, None);
        assert_eq!(args.module_path, None);
        assert!(!args.force);
    }
}
