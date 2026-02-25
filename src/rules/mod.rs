use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::graph::DependencyGraph;
use crate::spec::{SpecConfig, SpecFile};

pub mod boundary;
pub mod dependencies;

pub use dependencies::{
    DependencyRuleError, DependencyViolation, DependencyViolationKind, evaluate_dependency_rules,
    is_test_file,
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

impl RuleViolation {
    pub fn sort_stable(&mut self) {
        // no-op helper to keep callsites expressive in engines.
    }
}

/// Trait implemented by rule engines.
pub trait Rule {
    fn evaluate(&self, ctx: &RuleContext<'_>) -> Vec<RuleViolation>;
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
