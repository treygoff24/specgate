use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use serde_json::{json, Value};

use super::git::{
    list_tracked_files_scoped, FailClosedSpecOperation, RenameCopySemanticPairing, SpecSnapshotPair,
};
use super::types::{
    sort_field_changes_deterministic, sort_module_policy_diffs_deterministic, ChangeClassification,
    ChangeScope, FieldChange, ModulePolicyDiff,
};
use crate::spec::types::{
    Boundaries, BoundaryContract, Constraint, EnvelopeRequirement, Severity, SpecFile, Visibility,
};

#[derive(Debug, Clone, Copy)]
struct PathCoverageContext<'a> {
    project_root: &'a Path,
    base_ref: &'a str,
    head_ref: &'a str,
}

pub fn classify_spec_snapshot_pairs(pairs: &[SpecSnapshotPair]) -> Vec<ModulePolicyDiff> {
    classify_spec_snapshot_pairs_inner(pairs, None)
}

pub fn classify_spec_snapshot_pairs_with_path_coverage(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    pairs: &[SpecSnapshotPair],
) -> Vec<ModulePolicyDiff> {
    classify_spec_snapshot_pairs_inner(
        pairs,
        Some(PathCoverageContext {
            project_root,
            base_ref,
            head_ref,
        }),
    )
}

pub fn specs_semantically_equivalent_for_rename(base: &SpecFile, head: &SpecFile) -> bool {
    canonical_spec_for_rename_pairing(base) == canonical_spec_for_rename_pairing(head)
}

fn classify_spec_snapshot_pairs_inner(
    pairs: &[SpecSnapshotPair],
    path_coverage_context: Option<PathCoverageContext<'_>>,
) -> Vec<ModulePolicyDiff> {
    let mut diffs: Vec<ModulePolicyDiff> = pairs
        .iter()
        .filter_map(|pair| {
            classify_spec_snapshot_pair_inner(
                &pair.spec_path,
                pair.base_spec.as_ref(),
                pair.head_spec.as_ref(),
                path_coverage_context,
            )
        })
        .collect();

    sort_module_policy_diffs_deterministic(&mut diffs);
    diffs
}

pub fn classify_fail_closed_operations(ops: &[FailClosedSpecOperation]) -> Vec<ModulePolicyDiff> {
    let mut diffs = Vec::new();

    for operation in ops {
        match operation {
            FailClosedSpecOperation::Deletion { path } => {
                diffs.push(ModulePolicyDiff {
                    module: module_hint_from_spec_path(path),
                    spec_path: path.clone(),
                    changes: vec![FieldChange {
                        module: module_hint_from_spec_path(path),
                        spec_path: path.clone(),
                        scope: ChangeScope::SpecFile,
                        field: "spec_file".to_string(),
                        classification: ChangeClassification::Widening,
                        before: Some(json!(path)),
                        after: None,
                        detail: format!("deletion of policy file {path}"),
                    }],
                });
            }
            FailClosedSpecOperation::RenameOrCopy {
                status,
                from_path,
                to_path,
                semantic_pairing,
            } => {
                let (classification, detail) = match semantic_pairing {
                    RenameCopySemanticPairing::Equivalent => (
                        ChangeClassification::Structural,
                        format!(
                            "rename/copy of policy file {from_path} -> {to_path} ({status}) is semantically equivalent after normalization"
                        ),
                    ),
                    RenameCopySemanticPairing::Different => (
                        ChangeClassification::Widening,
                        format!(
                            "rename/copy of policy file {from_path} -> {to_path} ({status}) changed policy semantics and is treated as widening-risk"
                        ),
                    ),
                    RenameCopySemanticPairing::Inconclusive
                    | RenameCopySemanticPairing::Unassessed => (
                        ChangeClassification::Widening,
                        format!(
                            "rename/copy of policy file {from_path} -> {to_path} ({status}) could not be semantically paired and is treated as widening-risk"
                        ),
                    ),
                };

                let module = module_hint_from_spec_path(to_path);
                diffs.push(ModulePolicyDiff {
                    module: module.clone(),
                    spec_path: to_path.clone(),
                    changes: vec![FieldChange {
                        module,
                        spec_path: to_path.clone(),
                        scope: ChangeScope::SpecFile,
                        field: "spec_file".to_string(),
                        classification,
                        before: Some(json!(from_path)),
                        after: Some(json!(to_path)),
                        detail,
                    }],
                });
            }
        }
    }

    sort_module_policy_diffs_deterministic(&mut diffs);
    diffs
}

pub fn classify_spec_snapshot_pair(
    spec_path: &str,
    base: Option<&SpecFile>,
    head: Option<&SpecFile>,
) -> Option<ModulePolicyDiff> {
    classify_spec_snapshot_pair_inner(spec_path, base, head, None)
}

pub fn classify_spec_snapshot_pair_with_path_coverage(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    spec_path: &str,
    base: Option<&SpecFile>,
    head: Option<&SpecFile>,
) -> Option<ModulePolicyDiff> {
    classify_spec_snapshot_pair_inner(
        spec_path,
        base,
        head,
        Some(PathCoverageContext {
            project_root,
            base_ref,
            head_ref,
        }),
    )
}

fn classify_spec_snapshot_pair_inner(
    spec_path: &str,
    base: Option<&SpecFile>,
    head: Option<&SpecFile>,
    path_coverage_context: Option<PathCoverageContext<'_>>,
) -> Option<ModulePolicyDiff> {
    let module = module_name(base, head, spec_path);
    let mut changes = Vec::new();

    match (base, head) {
        (None, None) => return None,
        (None, Some(head)) => {
            changes.push(field_change(
                change_context(&module, spec_path, ChangeScope::SpecFile, "spec_file"),
                ChangeClassification::Structural,
                None,
                Some(json!(head.module)),
                "new policy file",
            ));
        }
        (Some(base), None) => {
            changes.push(field_change(
                change_context(&module, spec_path, ChangeScope::SpecFile, "spec_file"),
                ChangeClassification::Widening,
                Some(json!(base.module)),
                None,
                "policy file removed",
            ));
        }
        (Some(base), Some(head)) => {
            classify_spec_fields(
                spec_path,
                &module,
                base,
                head,
                path_coverage_context,
                &mut changes,
            );
        }
    }

    if changes.is_empty() {
        return None;
    }

    sort_field_changes_deterministic(&mut changes);
    Some(ModulePolicyDiff {
        module,
        spec_path: spec_path.to_string(),
        changes,
    })
}

fn classify_spec_fields(
    spec_path: &str,
    module: &str,
    base: &SpecFile,
    head: &SpecFile,
    path_coverage_context: Option<PathCoverageContext<'_>>,
    changes: &mut Vec<FieldChange>,
) {
    push_structural_if_changed(
        changes,
        module,
        spec_path,
        "version",
        json!(base.version),
        json!(head.version),
    );
    push_structural_if_changed(
        changes,
        module,
        spec_path,
        "module",
        json!(base.module),
        json!(head.module),
    );
    push_structural_if_changed(
        changes,
        module,
        spec_path,
        "package",
        json!(base.package),
        json!(head.package),
    );
    push_structural_if_changed(
        changes,
        module,
        spec_path,
        "import_id",
        json!(base.import_id),
        json!(head.import_id),
    );

    let base_import_ids = normalized_set(&base.import_ids);
    let head_import_ids = normalized_set(&head.import_ids);
    if base_import_ids != head_import_ids {
        changes.push(field_change(
            change_context(module, spec_path, ChangeScope::SpecFile, "import_ids"),
            ChangeClassification::Structural,
            Some(json!(base_import_ids)),
            Some(json!(head_import_ids)),
            "import_ids changed",
        ));
    }

    push_structural_if_changed(
        changes,
        module,
        spec_path,
        "description",
        json!(trimmed_opt(&base.description)),
        json!(trimmed_opt(&head.description)),
    );

    classify_boundaries(
        module,
        spec_path,
        base.boundaries.as_ref(),
        head.boundaries.as_ref(),
        path_coverage_context,
        changes,
    );
    classify_constraints(
        module,
        spec_path,
        &base.constraints,
        &head.constraints,
        changes,
    );
}

fn classify_boundaries(
    module: &str,
    spec_path: &str,
    base: Option<&Boundaries>,
    head: Option<&Boundaries>,
    path_coverage_context: Option<PathCoverageContext<'_>>,
    changes: &mut Vec<FieldChange>,
) {
    let base = base.cloned().unwrap_or_default();
    let head = head.cloned().unwrap_or_default();

    let base_path = trimmed_opt(&base.path);
    let head_path = trimmed_opt(&head.path);
    if base_path != head_path {
        let (classification, detail) = classify_boundaries_path_change(
            spec_path,
            base_path.as_deref(),
            head_path.as_deref(),
            path_coverage_context,
        );

        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Boundaries,
                "boundaries.path",
            ),
            classification,
            Some(json!(base_path)),
            Some(json!(head_path)),
            &detail,
        ));
    }

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.public_api",
        ),
        &base.public_api,
        &head.public_api,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        true,
        changes,
    );

    classify_allow_imports_from(
        module,
        spec_path,
        &base.allow_imports_from,
        &head.allow_imports_from,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.never_imports",
        ),
        &base.never_imports,
        &head.never_imports,
        ChangeClassification::Narrowing,
        ChangeClassification::Widening,
        false,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.allow_type_imports_from",
        ),
        &base.allow_type_imports_from,
        &head.allow_type_imports_from,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        false,
        changes,
    );

    classify_visibility(module, spec_path, base.visibility, head.visibility, changes);
    classify_allow_imported_by(
        module,
        spec_path,
        &base.allow_imported_by,
        &head.allow_imported_by,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.deny_imported_by",
        ),
        &base.deny_imported_by,
        &head.deny_imported_by,
        ChangeClassification::Narrowing,
        ChangeClassification::Widening,
        false,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.friend_modules",
        ),
        &base.friend_modules,
        &head.friend_modules,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        false,
        changes,
    );

    classify_bool_polarity(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.enforce_canonical_imports",
        ),
        base.enforce_canonical_imports,
        head.enforce_canonical_imports,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.allowed_dependencies",
        ),
        &base.allowed_dependencies,
        &head.allowed_dependencies,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        false,
        changes,
    );

    classify_set_field(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.forbidden_dependencies",
        ),
        &base.forbidden_dependencies,
        &head.forbidden_dependencies,
        ChangeClassification::Narrowing,
        ChangeClassification::Widening,
        false,
        changes,
    );

    classify_bool_polarity(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.enforce_in_tests",
        ),
        base.enforce_in_tests,
        head.enforce_in_tests,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
        changes,
    );

    classify_contracts(module, spec_path, &base.contracts, &head.contracts, changes);
}

fn classify_boundaries_path_change(
    spec_path: &str,
    base_path: Option<&str>,
    head_path: Option<&str>,
    path_coverage_context: Option<PathCoverageContext<'_>>,
) -> (ChangeClassification, String) {
    let Some(context) = path_coverage_context else {
        return (
            ChangeClassification::Structural,
            "boundaries.path changed (coverage context unavailable in this call path)".to_string(),
        );
    };

    let prefixes = candidate_path_prefixes(spec_path, base_path, head_path);
    if prefixes.is_empty() {
        return (
            ChangeClassification::Structural,
            "boundaries.path changed: path_coverage_unbounded_mvp (empty scoped prefix set)"
                .to_string(),
        );
    }

    let base_files = match list_tracked_files_scoped(
        context.project_root,
        context.base_ref,
        &prefixes,
    ) {
        Ok(files) => files,
        Err(error) => {
            return (
                ChangeClassification::Structural,
                format!(
                    "boundaries.path changed: path_coverage_unbounded_mvp (base ref scoped ls-tree failed: {})",
                    error.message()
                ),
            );
        }
    };

    let head_files = match list_tracked_files_scoped(
        context.project_root,
        context.head_ref,
        &prefixes,
    ) {
        Ok(files) => files,
        Err(error) => {
            return (
                ChangeClassification::Structural,
                format!(
                    "boundaries.path changed: path_coverage_unbounded_mvp (head ref scoped ls-tree failed: {})",
                    error.message()
                ),
            );
        }
    };

    let file_universe: BTreeSet<String> = base_files
        .into_iter()
        .chain(head_files)
        .filter(|path| is_source_like_or_spec(path))
        .collect();

    let base_matcher = match compile_optional_path_glob(base_path) {
        Ok(matcher) => matcher,
        Err(message) => {
            return (
                ChangeClassification::Structural,
                format!("boundaries.path changed: path_coverage_unbounded_mvp ({message})"),
            );
        }
    };

    let head_matcher = match compile_optional_path_glob(head_path) {
        Ok(matcher) => matcher,
        Err(message) => {
            return (
                ChangeClassification::Structural,
                format!("boundaries.path changed: path_coverage_unbounded_mvp ({message})"),
            );
        }
    };

    let base_matches = matched_paths_for_glob(base_matcher.as_ref(), &file_universe);
    let head_matches = matched_paths_for_glob(head_matcher.as_ref(), &file_universe);

    if head_matches == base_matches {
        return (
            ChangeClassification::Structural,
            "boundaries.path changed with equal scoped coverage".to_string(),
        );
    }

    if head_matches.is_superset(&base_matches) {
        return (
            ChangeClassification::Widening,
            format!(
                "boundaries.path broadened scoped coverage ({} -> {} matched files)",
                base_matches.len(),
                head_matches.len()
            ),
        );
    }

    if head_matches.is_subset(&base_matches) {
        return (
            ChangeClassification::Narrowing,
            format!(
                "boundaries.path narrowed scoped coverage ({} -> {} matched files)",
                base_matches.len(),
                head_matches.len()
            ),
        );
    }

    (
        ChangeClassification::Structural,
        "boundaries.path changed with ambiguous scoped coverage overlap".to_string(),
    )
}

fn compile_optional_path_glob(path_glob: Option<&str>) -> Result<Option<GlobMatcher>, String> {
    let Some(path_glob) = path_glob.map(str::trim).filter(|glob| !glob.is_empty()) else {
        return Ok(None);
    };

    let glob = GlobBuilder::new(path_glob)
        .literal_separator(true)
        .build()
        .map_err(|error| format!("invalid boundaries.path glob '{path_glob}': {error}"))?;

    Ok(Some(glob.compile_matcher()))
}

fn matched_paths_for_glob(
    matcher: Option<&GlobMatcher>,
    file_universe: &BTreeSet<String>,
) -> BTreeSet<String> {
    let Some(matcher) = matcher else {
        return BTreeSet::new();
    };

    file_universe
        .iter()
        .filter(|path| matcher.is_match(path.as_str()))
        .cloned()
        .collect()
}

fn candidate_path_prefixes(
    spec_path: &str,
    base_path: Option<&str>,
    head_path: Option<&str>,
) -> BTreeSet<String> {
    let mut prefixes = BTreeSet::new();

    for path_glob in [base_path, head_path].into_iter().flatten() {
        if let Some(prefix) = static_prefix_from_glob(path_glob) {
            prefixes.insert(prefix);
        }
    }

    if prefixes.is_empty()
        && (base_path.is_some() || head_path.is_some())
        && let Some(module_prefix) = module_prefix_from_spec_path(spec_path)
    {
        prefixes.insert(module_prefix);
    }

    prefixes
}

fn static_prefix_from_glob(path_glob: &str) -> Option<String> {
    let trimmed = path_glob.trim();
    if trimmed.is_empty() {
        return None;
    }

    let wildcard_index = trimmed
        .char_indices()
        .find_map(|(index, character)| match character {
            '*' | '?' | '[' => Some(index),
            _ => None,
        })
        .unwrap_or(trimmed.len());

    let prefix = trimmed[..wildcard_index]
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/');

    if prefix.is_empty() {
        return None;
    }

    Some(prefix.to_string())
}

fn module_prefix_from_spec_path(spec_path: &str) -> Option<String> {
    let stripped = spec_path
        .trim()
        .strip_suffix(".spec.yml")
        .unwrap_or(spec_path.trim())
        .trim_start_matches("./")
        .trim_start_matches('/');

    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn is_source_like_or_spec(path: &str) -> bool {
    path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".js")
        || path.ends_with(".jsx")
        || path.ends_with(".mts")
        || path.ends_with(".cts")
        || path.ends_with(".spec.yml")
}

fn classify_allow_imports_from(
    module: &str,
    spec_path: &str,
    base: &Option<Vec<String>>,
    head: &Option<Vec<String>>,
    changes: &mut Vec<FieldChange>,
) {
    let base_set = base.as_ref().map(|values| normalized_set(values));
    let head_set = head.as_ref().map(|values| normalized_set(values));

    match (&base_set, &head_set) {
        (None, None) => {}
        (Some(before), Some(after)) => {
            classify_set_delta(
                change_context(
                    module,
                    spec_path,
                    ChangeScope::Boundaries,
                    "boundaries.allow_imports_from",
                ),
                before,
                after,
                ChangeClassification::Widening,
                ChangeClassification::Narrowing,
                changes,
            );
        }
        (Some(before), None) => {
            changes.push(field_change(
                change_context(
                    module,
                    spec_path,
                    ChangeScope::Boundaries,
                    "boundaries.allow_imports_from",
                ),
                ChangeClassification::Widening,
                Some(json!(before)),
                None,
                "allow_imports_from restricted -> unrestricted",
            ));
        }
        (None, Some(after)) => {
            changes.push(field_change(
                change_context(
                    module,
                    spec_path,
                    ChangeScope::Boundaries,
                    "boundaries.allow_imports_from",
                ),
                ChangeClassification::Narrowing,
                None,
                Some(json!(after)),
                "allow_imports_from unrestricted -> restricted",
            ));
        }
    }
}

fn classify_allow_imported_by(
    module: &str,
    spec_path: &str,
    base: &[String],
    head: &[String],
    changes: &mut Vec<FieldChange>,
) {
    let base_set = normalized_set(base);
    let head_set = normalized_set(head);

    let base_restricted = !base_set.is_empty();
    let head_restricted = !head_set.is_empty();

    match (base_restricted, head_restricted) {
        (false, false) => {}
        (true, false) => changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Boundaries,
                "boundaries.allow_imported_by",
            ),
            ChangeClassification::Widening,
            Some(json!(base_set)),
            Some(json!([])),
            "allow_imported_by restricted -> unrestricted",
        )),
        (false, true) => changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Boundaries,
                "boundaries.allow_imported_by",
            ),
            ChangeClassification::Narrowing,
            Some(json!([])),
            Some(json!(head_set)),
            "allow_imported_by unrestricted -> restricted",
        )),
        (true, true) => classify_set_delta(
            change_context(
                module,
                spec_path,
                ChangeScope::Boundaries,
                "boundaries.allow_imported_by",
            ),
            &base_set,
            &head_set,
            ChangeClassification::Widening,
            ChangeClassification::Narrowing,
            changes,
        ),
    }
}

fn classify_visibility(
    module: &str,
    spec_path: &str,
    base: Option<Visibility>,
    head: Option<Visibility>,
    changes: &mut Vec<FieldChange>,
) {
    let before = base.unwrap_or(Visibility::Public);
    let after = head.unwrap_or(Visibility::Public);

    if before == after {
        return;
    }

    let before_rank = visibility_rank(before);
    let after_rank = visibility_rank(after);
    let classification = if after_rank < before_rank {
        ChangeClassification::Widening
    } else {
        ChangeClassification::Narrowing
    };

    changes.push(field_change(
        change_context(
            module,
            spec_path,
            ChangeScope::Boundaries,
            "boundaries.visibility",
        ),
        classification,
        Some(json!(before)),
        Some(json!(after)),
        format!("visibility changed: {before:?} -> {after:?}"),
    ));
}

fn visibility_rank(value: Visibility) -> u8 {
    match value {
        Visibility::Public => 0,
        Visibility::Internal => 1,
        Visibility::Private => 2,
    }
}

fn classify_constraints(
    module: &str,
    spec_path: &str,
    base: &[Constraint],
    head: &[Constraint],
    changes: &mut Vec<FieldChange>,
) {
    let mut base_by_key: BTreeMap<String, &Constraint> = BTreeMap::new();
    for constraint in base {
        base_by_key.insert(constraint_key(constraint), constraint);
    }

    let mut head_by_key: BTreeMap<String, &Constraint> = BTreeMap::new();
    for constraint in head {
        head_by_key.insert(constraint_key(constraint), constraint);
    }

    for (key, base_constraint) in &base_by_key {
        if let Some(head_constraint) = head_by_key.get(key) {
            if base_constraint.severity != head_constraint.severity {
                let classification = match (base_constraint.severity, head_constraint.severity) {
                    (Severity::Error, Severity::Warning) => ChangeClassification::Widening,
                    (Severity::Warning, Severity::Error) => ChangeClassification::Narrowing,
                    _ => ChangeClassification::Structural,
                };

                changes.push(field_change(
                    change_context(
                        module,
                        spec_path,
                        ChangeScope::Constraint,
                        "constraints.severity",
                    ),
                    classification,
                    Some(json!(base_constraint.severity)),
                    Some(json!(head_constraint.severity)),
                    format!("constraint '{key}' severity changed"),
                ));
            }

            let base_message = trimmed_opt(&base_constraint.message);
            let head_message = trimmed_opt(&head_constraint.message);
            if base_message != head_message {
                changes.push(field_change(
                    change_context(
                        module,
                        spec_path,
                        ChangeScope::Constraint,
                        "constraints.message",
                    ),
                    ChangeClassification::Structural,
                    Some(json!(base_message)),
                    Some(json!(head_message)),
                    format!("constraint '{key}' message changed"),
                ));
            }
        }
    }

    for key in base_by_key.keys() {
        if !head_by_key.contains_key(key) {
            changes.push(field_change(
                change_context(module, spec_path, ChangeScope::Constraint, "constraints"),
                ChangeClassification::Structural,
                Some(json!(key)),
                None,
                format!("constraint '{key}' removed (MVP structural)"),
            ));
        }
    }

    for key in head_by_key.keys() {
        if !base_by_key.contains_key(key) {
            changes.push(field_change(
                change_context(module, spec_path, ChangeScope::Constraint, "constraints"),
                ChangeClassification::Structural,
                None,
                Some(json!(key)),
                format!("constraint '{key}' added (MVP structural)"),
            ));
        }
    }
}

fn constraint_key(constraint: &Constraint) -> String {
    format!(
        "{}::{}",
        constraint.rule.trim(),
        canonical_json_string(&constraint.params)
    )
}

fn canonical_json_string(value: &Value) -> String {
    serde_json::to_string(&canonicalize_json(value)).unwrap_or_else(|_| "null".to_string())
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = serde_json::Map::new();
            for key in map.keys().cloned().collect::<BTreeSet<_>>() {
                if let Some(v) = map.get(&key) {
                    ordered.insert(key, canonicalize_json(v));
                }
            }
            Value::Object(ordered)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

fn canonical_spec_for_rename_pairing(spec: &SpecFile) -> Value {
    let boundaries = spec.boundaries.clone().unwrap_or_default();

    json!({
        "version": spec.version.trim(),
        "module": spec.module.trim(),
        "package": trimmed_opt(&spec.package),
        "import_id": trimmed_opt(&spec.import_id),
        "import_ids": normalized_vec(&spec.import_ids),
        "description": trimmed_opt(&spec.description),
        "boundaries": {
            "path": trimmed_opt(&boundaries.path),
            "public_api": normalized_vec(&boundaries.public_api),
            "allow_imports_from": normalized_opt_vec(&boundaries.allow_imports_from),
            "never_imports": normalized_vec(&boundaries.never_imports),
            "allow_type_imports_from": normalized_vec(&boundaries.allow_type_imports_from),
            "visibility": boundaries.visibility.unwrap_or(Visibility::Public),
            "allow_imported_by": normalized_vec(&boundaries.allow_imported_by),
            "deny_imported_by": normalized_vec(&boundaries.deny_imported_by),
            "friend_modules": normalized_vec(&boundaries.friend_modules),
            "enforce_canonical_imports": boundaries.enforce_canonical_imports,
            "allowed_dependencies": normalized_vec(&boundaries.allowed_dependencies),
            "forbidden_dependencies": normalized_vec(&boundaries.forbidden_dependencies),
            "enforce_in_tests": boundaries.enforce_in_tests,
            "contracts": canonical_contracts_for_rename_pairing(&boundaries.contracts),
        },
        "constraints": canonical_constraints_for_rename_pairing(&spec.constraints),
    })
}

fn canonical_constraints_for_rename_pairing(constraints: &[Constraint]) -> Vec<Value> {
    let mut by_key: BTreeMap<String, Value> = BTreeMap::new();

    for constraint in constraints {
        by_key.insert(
            constraint_key(constraint),
            json!({
                "rule": constraint.rule.trim(),
                "params": canonicalize_json(&constraint.params),
                "severity": constraint.severity,
                "message": trimmed_opt(&constraint.message),
            }),
        );
    }

    by_key.into_values().collect()
}

fn canonical_contracts_for_rename_pairing(contracts: &[BoundaryContract]) -> Vec<Value> {
    let mut by_id: BTreeMap<String, Value> = BTreeMap::new();

    for contract in contracts {
        let contract_id = contract.id.trim().to_string();
        by_id.insert(
            contract_id,
            json!({
                "id": contract.id.trim(),
                "contract": contract.contract.trim(),
                "files": normalized_vec(&contract.r#match.files),
                "pattern": trimmed_opt(&contract.r#match.pattern),
                "direction": contract.direction,
                "envelope": contract.envelope,
                "imports_contract": normalized_vec(&contract.imports_contract),
            }),
        );
    }

    by_id.into_values().collect()
}

fn normalized_opt_vec(values: &Option<Vec<String>>) -> Option<Vec<String>> {
    values.as_ref().map(|values| normalized_vec(values))
}

fn normalized_vec(values: &[String]) -> Vec<String> {
    normalized_set(values).into_iter().collect()
}

fn classify_contracts(
    module: &str,
    spec_path: &str,
    base: &[BoundaryContract],
    head: &[BoundaryContract],
    changes: &mut Vec<FieldChange>,
) {
    let mut base_by_id: BTreeMap<String, &BoundaryContract> = BTreeMap::new();
    for contract in base {
        base_by_id.insert(contract.id.trim().to_string(), contract);
    }

    let mut head_by_id: BTreeMap<String, &BoundaryContract> = BTreeMap::new();
    for contract in head {
        head_by_id.insert(contract.id.trim().to_string(), contract);
    }

    for (id, base_contract) in &base_by_id {
        if let Some(head_contract) = head_by_id.get(id) {
            classify_contract_fields(module, spec_path, id, base_contract, head_contract, changes);
        }
    }

    for id in base_by_id.keys() {
        if !head_by_id.contains_key(id) {
            changes.push(field_change(
                change_context(
                    module,
                    spec_path,
                    ChangeScope::Contract,
                    "boundaries.contracts",
                ),
                ChangeClassification::Widening,
                Some(json!(id)),
                None,
                format!("contract '{id}' removed"),
            ));
        }
    }

    for id in head_by_id.keys() {
        if !base_by_id.contains_key(id) {
            changes.push(field_change(
                change_context(
                    module,
                    spec_path,
                    ChangeScope::Contract,
                    "boundaries.contracts",
                ),
                ChangeClassification::Narrowing,
                None,
                Some(json!(id)),
                format!("contract '{id}' added"),
            ));
        }
    }
}

fn classify_contract_fields(
    module: &str,
    spec_path: &str,
    id: &str,
    base: &BoundaryContract,
    head: &BoundaryContract,
    changes: &mut Vec<FieldChange>,
) {
    if base.envelope != head.envelope {
        let classification = match (base.envelope, head.envelope) {
            (EnvelopeRequirement::Required, EnvelopeRequirement::Optional) => {
                ChangeClassification::Widening
            }
            (EnvelopeRequirement::Optional, EnvelopeRequirement::Required) => {
                ChangeClassification::Narrowing
            }
            _ => ChangeClassification::Structural,
        };

        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Contract,
                "boundaries.contracts.envelope",
            ),
            classification,
            Some(json!(base.envelope)),
            Some(json!(head.envelope)),
            format!("contract '{id}' envelope changed"),
        ));
    }

    let base_files = normalized_set(&base.r#match.files);
    let head_files = normalized_set(&head.r#match.files);
    classify_set_delta(
        change_context(
            module,
            spec_path,
            ChangeScope::ContractMatch,
            "boundaries.contracts.match.files",
        ),
        &base_files,
        &head_files,
        ChangeClassification::Narrowing,
        ChangeClassification::Widening,
        changes,
    );

    if base.r#match.pattern != head.r#match.pattern {
        let classification = match (
            trimmed_opt(&base.r#match.pattern),
            trimmed_opt(&head.r#match.pattern),
        ) {
            (None, Some(_)) => ChangeClassification::Widening,
            (Some(_), None) => ChangeClassification::Narrowing,
            (Some(_), Some(_)) => ChangeClassification::Structural,
            (None, None) => ChangeClassification::Structural,
        };

        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::ContractMatch,
                "boundaries.contracts.match.pattern",
            ),
            classification,
            Some(json!(trimmed_opt(&base.r#match.pattern))),
            Some(json!(trimmed_opt(&head.r#match.pattern))),
            format!("contract '{id}' match.pattern changed"),
        ));
    }

    let base_contract_path = base.contract.trim();
    let head_contract_path = head.contract.trim();
    if base_contract_path != head_contract_path {
        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Contract,
                "boundaries.contracts.contract",
            ),
            ChangeClassification::Structural,
            Some(json!(base_contract_path)),
            Some(json!(head_contract_path)),
            format!("contract '{id}' path changed"),
        ));
    }

    if base.direction != head.direction {
        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Contract,
                "boundaries.contracts.direction",
            ),
            ChangeClassification::Structural,
            Some(json!(base.direction)),
            Some(json!(head.direction)),
            format!("contract '{id}' direction changed (MVP structural)"),
        ));
    }

    let base_imports = normalized_set(&base.imports_contract);
    let head_imports = normalized_set(&head.imports_contract);
    if base_imports != head_imports {
        changes.push(field_change(
            change_context(
                module,
                spec_path,
                ChangeScope::Contract,
                "boundaries.contracts.imports_contract",
            ),
            ChangeClassification::Structural,
            Some(json!(base_imports)),
            Some(json!(head_imports)),
            format!("contract '{id}' imports_contract changed (MVP structural)"),
        ));
    }
}

fn classify_set_field(
    context: ChangeContext<'_>,
    base: &[String],
    head: &[String],
    addition_classification: ChangeClassification,
    removal_classification: ChangeClassification,
    reorder_structural: bool,
    changes: &mut Vec<FieldChange>,
) {
    let base_set = normalized_set(base);
    let head_set = normalized_set(head);

    if base_set == head_set {
        if reorder_structural && base != head {
            changes.push(field_change(
                context,
                ChangeClassification::Structural,
                Some(json!(base)),
                Some(json!(head)),
                "order changed",
            ));
        }
        return;
    }

    classify_set_delta(
        context,
        &base_set,
        &head_set,
        addition_classification,
        removal_classification,
        changes,
    );
}

fn classify_set_delta(
    context: ChangeContext<'_>,
    base_set: &BTreeSet<String>,
    head_set: &BTreeSet<String>,
    addition_classification: ChangeClassification,
    removal_classification: ChangeClassification,
    changes: &mut Vec<FieldChange>,
) {
    let additions: Vec<String> = head_set.difference(base_set).cloned().collect();
    let removals: Vec<String> = base_set.difference(head_set).cloned().collect();

    if !additions.is_empty() {
        changes.push(field_change(
            context,
            addition_classification,
            Some(json!(base_set)),
            Some(json!(head_set)),
            format!("added {}", additions.join(", ")),
        ));
    }

    if !removals.is_empty() {
        changes.push(field_change(
            context,
            removal_classification,
            Some(json!(base_set)),
            Some(json!(head_set)),
            format!("removed {}", removals.join(", ")),
        ));
    }
}

fn classify_bool_polarity(
    context: ChangeContext<'_>,
    before: bool,
    after: bool,
    true_to_false_classification: ChangeClassification,
    false_to_true_classification: ChangeClassification,
    changes: &mut Vec<FieldChange>,
) {
    if before == after {
        return;
    }

    let classification = if before && !after {
        true_to_false_classification
    } else {
        false_to_true_classification
    };

    changes.push(field_change(
        context,
        classification,
        Some(json!(before)),
        Some(json!(after)),
        format!("{before} -> {after}"),
    ));
}

fn normalized_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn trimmed_opt(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|value| value.trim().to_string())
        .and_then(|value| if value.is_empty() { None } else { Some(value) })
}

fn push_structural_if_changed(
    changes: &mut Vec<FieldChange>,
    module: &str,
    spec_path: &str,
    field: &str,
    before: Value,
    after: Value,
) {
    if before != after {
        changes.push(field_change(
            change_context(module, spec_path, ChangeScope::SpecFile, field),
            ChangeClassification::Structural,
            Some(before),
            Some(after),
            format!("{field} changed"),
        ));
    }
}

#[derive(Debug, Clone, Copy)]
struct ChangeContext<'a> {
    module: &'a str,
    spec_path: &'a str,
    scope: ChangeScope,
    field: &'a str,
}

fn change_context<'a>(
    module: &'a str,
    spec_path: &'a str,
    scope: ChangeScope,
    field: &'a str,
) -> ChangeContext<'a> {
    ChangeContext {
        module,
        spec_path,
        scope,
        field,
    }
}

fn field_change(
    context: ChangeContext<'_>,
    classification: ChangeClassification,
    before: Option<Value>,
    after: Option<Value>,
    detail: impl Into<String>,
) -> FieldChange {
    FieldChange {
        module: context.module.to_string(),
        spec_path: context.spec_path.to_string(),
        scope: context.scope,
        field: context.field.to_string(),
        classification,
        before,
        after,
        detail: detail.into(),
    }
}

fn module_name(base: Option<&SpecFile>, head: Option<&SpecFile>, spec_path: &str) -> String {
    if let Some(head) = head {
        return head.module.clone();
    }
    if let Some(base) = base {
        return base.module.clone();
    }

    module_hint_from_spec_path(spec_path)
}

fn module_hint_from_spec_path(spec_path: &str) -> String {
    spec_path
        .strip_suffix(".spec.yml")
        .unwrap_or(spec_path)
        .trim_start_matches("modules/")
        .to_string()
}
