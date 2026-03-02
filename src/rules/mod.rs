use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::deterministic::normalize_repo_relative;
use crate::graph::DependencyGraph;
use crate::spec::{SpecConfig, SpecFile};

pub mod boundary;
pub mod circular;
pub mod dependencies;
pub mod layers;

pub use boundary::{
    is_canonical_import_rule_id, BOUNDARY_CANONICAL_IMPORTS_RULE_ID_ALIAS,
    BOUNDARY_CANONICAL_IMPORT_RULE_ID, BOUNDARY_CONTRACT_VERSION_MISMATCH_RULE_ID,
};
pub use circular::{
    evaluate_no_circular_deps, CircularDependencyViolation, CircularScopeParam,
    NO_CIRCULAR_DEPS_RULE_ID,
};
pub use dependencies::{
    evaluate_dependency_rules, DependencyRule, DependencyRuleError, DependencyViolation,
    DependencyViolationKind, DEPENDENCY_FORBIDDEN_RULE_ID, DEPENDENCY_NOT_ALLOWED_RULE_ID,
};
pub use layers::{
    evaluate_enforce_layer, layer_for_module, parse_enforce_layer_config, EnforceLayerConfig,
    EnforceLayerReport, LayerConfigIssue, LayerConfigParseError, LayerViolation,
    ENFORCE_LAYER_RULE_ID,
};

/// Shared evaluation context passed into rules.
pub struct RuleContext<'a> {
    pub project_root: &'a Path,
    pub config: &'a SpecConfig,
    pub specs: &'a [SpecFile],
    pub graph: &'a DependencyGraph,
}

/// Rule violation emitted by the rule engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleViolation {
    pub rule: String,
    pub message: String,
    pub from_file: PathBuf,
    pub to_file: Option<PathBuf>,
    pub from_module: Option<String>,
    pub to_module: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Trait implemented by stateless graph-only rule engines.
pub trait Rule {
    fn evaluate(&self, ctx: &RuleContext<'_>) -> Vec<RuleViolation>;
}

/// Bridge trait for rules that require mutable infrastructure (e.g. resolver caches).
pub trait RuleWithResolver {
    type Error;

    fn evaluate_with_resolver(
        &self,
        ctx: &RuleContext<'_>,
        resolver: &mut crate::resolver::ModuleResolver,
    ) -> std::result::Result<Vec<RuleViolation>, Self::Error>;
}

pub(crate) fn sort_violations_stable(violations: &mut [RuleViolation]) {
    violations.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.from_module.cmp(&b.from_module))
            .then_with(|| a.to_module.cmp(&b.to_module))
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.message.cmp(&b.message))
    });
}

pub(crate) fn normalized_string_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[derive(Debug)]
pub(crate) enum GlobCompileError {
    InvalidPattern {
        pattern: String,
        source: globset::Error,
    },
    Build {
        source: globset::Error,
    },
}

pub(crate) fn compile_optional_globset_strict(
    patterns: &[String],
) -> std::result::Result<Option<GlobSet>, GlobCompileError> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|source| GlobCompileError::InvalidPattern {
            pattern: pattern.clone(),
            source,
        })?;
        builder.add(glob);
    }

    let matcher = builder
        .build()
        .map_err(|source| GlobCompileError::Build { source })?;

    Ok(Some(matcher))
}

pub(crate) fn matches_test_file(
    project_root: &Path,
    file: &Path,
    test_matcher: Option<&GlobSet>,
) -> bool {
    let Some(test_matcher) = test_matcher else {
        return false;
    };

    let relative = normalize_repo_relative(project_root, file);
    test_matcher.is_match(&relative)
}

#[cfg(test)]
pub(crate) fn write_test_file(project_root: &Path, relative_path: &str, contents: &str) {
    let full_path = project_root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir test fixture parent");
    }
    std::fs::write(full_path, contents).expect("write test fixture file");
}
