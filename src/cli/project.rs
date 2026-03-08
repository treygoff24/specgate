use super::*;

pub(crate) fn load_project(project_root: &Path) -> std::result::Result<LoadedProject, String> {
    let project_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    let config = spec::load_config(&project_root)
        .map_err(|error| format!("failed to load config: {error}"))?;

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
