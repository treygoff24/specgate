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
    /// Starter module id used in the initial example spec.
    #[arg(long, default_value = "app")]
    pub module: String,
    /// Starter module boundary glob.
    #[arg(long, default_value = "src/app/**/*")]
    pub module_path: String,
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
            module: "app".to_string(),
            module_path: "src/app/**/*".to_string(),
            force: false,
        };

        assert_eq!(args.spec_dir, PathBuf::from("modules"));
        assert_eq!(args.module, "app");
        assert_eq!(args.module_path, "src/app/**/*");
        assert!(!args.force);
    }
}
