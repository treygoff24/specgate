use crate::spec::{Boundaries, SpecFile};

pub(crate) fn build_spec_with_boundaries(
    version: &str,
    module: &str,
    path: &str,
    boundaries: Boundaries,
) -> SpecFile {
    SpecFile {
        version: version.to_string(),
        module: module.to_string(),
        package: None,
        import_id: None,
        import_ids: Vec::new(),
        description: None,
        boundaries: Some(Boundaries {
            path: Some(path.to_string()),
            ..boundaries
        }),
        constraints: Vec::new(),
        spec_path: None,
    }
}
