use std::path::Path;

use crate::cli::{CliRunResult, LoadedProject, runtime_error_json};
use crate::spec;

pub(crate) fn load_project(project_root: &Path) -> std::result::Result<LoadedProject, String> {
    let project_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    let config = spec::load_config(&project_root)
        .map_err(|error| format!("failed to load config: {error}"))?;

    spec::workspace_discovery::discover_workspace_packages_strict(&project_root, &config)
        .map_err(|error| format!("failed to discover workspace packages: {error}"))?;

    let (specs, validation) =
        spec::discover_and_validate(&project_root, &config).map_err(|error| {
            use std::error::Error;
            let mut msg = format!("failed to discover specs: {error}");
            let mut source = error.source();
            while let Some(cause) = source {
                msg.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            msg
        })?;

    Ok(LoadedProject {
        project_root,
        config,
        specs,
        validation,
    })
}

pub(crate) fn load_project_for_analysis(
    project_root: &Path,
) -> std::result::Result<LoadedProject, CliRunResult> {
    let loaded = load_project(project_root)
        .map_err(|error| runtime_error_json("config", "failed to load project", vec![error]))?;

    if loaded.validation.has_errors() {
        let details = loaded
            .validation
            .errors()
            .into_iter()
            .map(|issue| format!("{}: {}", issue.module, issue.message))
            .collect();
        return Err(runtime_error_json(
            "validation",
            "spec validation failed; run `specgate validate` for details",
            details,
        ));
    }

    Ok(loaded)
}
