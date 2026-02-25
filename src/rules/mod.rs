use std::collections::BTreeSet;

pub mod dependencies;

pub use dependencies::{
    DependencyRuleError, DependencyViolation, DependencyViolationKind, evaluate_dependency_rules,
    is_test_file,
};

pub(crate) fn normalized_string_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}
