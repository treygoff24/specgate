use super::*;

use std::collections::BTreeMap;
use std::path::Path;

use crate::rules::RuleViolation;
use crate::spec::{Severity, SpecConfig, SpecFile};
use crate::verdict::WorkspacePackageInfo;

pub(crate) fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

pub(crate) fn dependency_violation_severity(rule_id: &str) -> Severity {
    debug_assert!(
        matches!(
            rule_id,
            DEPENDENCY_FORBIDDEN_RULE_ID | DEPENDENCY_NOT_ALLOWED_RULE_ID
        ),
        "unexpected dependency rule id '{rule_id}'"
    );

    Severity::Error
}

pub(crate) fn rule_ids_match(constraint_rule: &str, violation_rule: &str) -> bool {
    if is_canonical_import_rule_id(constraint_rule) || is_canonical_import_rule_id(violation_rule) {
        return is_canonical_import_rule_id(constraint_rule)
            && is_canonical_import_rule_id(violation_rule);
    }

    constraint_rule == violation_rule
}

pub(crate) fn severity_for_constraint_rule(spec: &SpecFile, rule_id: &str) -> Option<Severity> {
    spec.constraints
        .iter()
        .filter(|constraint| rule_ids_match(&constraint.rule, rule_id))
        .map(|constraint| constraint.severity)
        .min_by_key(|severity| severity_rank(*severity))
}

pub(crate) fn boundary_constraint_module(violation: &RuleViolation) -> Option<&str> {
    let rule = violation.rule.as_str();

    match rule {
        "boundary.never_imports" | "boundary.allow_imports_from" => {
            violation.from_module.as_deref()
        }
        "boundary.public_api"
        | "boundary.deny_imported_by"
        | "boundary.allow_imported_by"
        | "boundary.visibility.internal"
        | "boundary.visibility.private" => violation.to_module.as_deref(),
        _ if is_canonical_import_rule_id(rule) => violation.to_module.as_deref(),
        _ => None,
    }
}

pub(crate) fn boundary_violation_severity(
    violation: &RuleViolation,
    spec_by_module: &BTreeMap<&str, &SpecFile>,
) -> Severity {
    let Some(module_id) = boundary_constraint_module(violation) else {
        return Severity::Error;
    };

    spec_by_module
        .get(module_id)
        .and_then(|spec| severity_for_constraint_rule(spec, &violation.rule))
        .unwrap_or(Severity::Error)
}

/// Build workspace package info for the verdict, returning `None` for non-monorepo projects.
pub(crate) fn build_workspace_packages_info(
    project_root: &Path,
    config: &SpecConfig,
) -> Option<Vec<WorkspacePackageInfo>> {
    let packages = discover_workspace_packages_with_config(project_root, config);
    if packages.is_empty() {
        return None;
    }

    let mut infos: Vec<WorkspacePackageInfo> = packages
        .iter()
        .map(|pkg| {
            let abs_dir = project_root.join(&pkg.relative_dir);
            let tsconfig = nearest_tsconfig_for_dir_uncached(
                project_root,
                &abs_dir,
                &config.tsconfig_filename,
            )
            .map(|p| normalize_repo_relative(project_root, &p));

            WorkspacePackageInfo {
                name: pkg.module.clone(),
                path: pkg.relative_dir.clone(),
                tsconfig,
            }
        })
        .collect();

    infos.sort_by(|a, b| a.name.cmp(&b.name));
    Some(infos)
}
