pub mod circular;

pub use circular::{
    CircularDependencyViolation, CircularScopeParam, NO_CIRCULAR_DEPS_RULE_ID,
    evaluate_no_circular_deps,
};
