//! Contract rules engine for validating boundary contracts.
//!
//! Validates contracts defined in module boundaries, including:
//! - Contract file existence and non-emptiness
//! - Match pattern resolution (glob matching)
//! - Cross-module contract reference validity

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use globset::GlobBuilder;

use crate::rules::{RuleContext, RuleViolation};
use crate::spec::types::{BoundaryContract, SpecFile};

// Module placeholder for testing exports
// This module is re-exported in mod.rs

/// Rule ID for missing contract file.
pub const BOUNDARY_CONTRACT_MISSING_RULE_ID: &str = "boundary.contract_missing";
/// Rule ID for empty contract file.
pub const BOUNDARY_CONTRACT_EMPTY_RULE_ID: &str = "boundary.contract_empty";
/// Rule ID for unresolved match patterns.
pub const BOUNDARY_MATCH_UNRESOLVED_RULE_ID: &str = "boundary.match_unresolved";
/// Rule ID for invalid cross-module contract reference.
pub const BOUNDARY_CONTRACT_REF_INVALID_RULE_ID: &str = "boundary.contract_ref_invalid";

/// A contract rule violation with remediation hint and contract context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractRuleViolation {
    /// The underlying rule violation.
    pub violation: RuleViolation,
    /// Human-readable remediation hint.
    pub remediation_hint: String,
    /// The contract ID that triggered this violation.
    pub contract_id: String,
}

impl ContractRuleViolation {
    /// Create a new contract rule violation.
    fn new(
        violation: RuleViolation,
        remediation_hint: impl Into<String>,
        contract_id: impl Into<String>,
    ) -> Self {
        Self {
            violation,
            remediation_hint: remediation_hint.into(),
            contract_id: contract_id.into(),
        }
    }
}

/// Evaluate all contract rules for the given context.
///
/// When `affected_modules` is `Some`, only contracts in those modules are evaluated.
/// This enables evaluation-time scoping for incremental checks.
pub fn evaluate_contract_rules(
    ctx: &RuleContext<'_>,
    affected_modules: Option<&BTreeSet<String>>,
) -> Vec<ContractRuleViolation> {
    let mut violations = Vec::new();

    // Build a registry of all contracts across all modules for cross-module validation
    let contract_registry = build_contract_registry(ctx.specs);

    // Collect specs to evaluate based on affected_modules filter
    let specs_to_evaluate: Vec<&SpecFile> = ctx
        .specs
        .iter()
        .filter(|spec| {
            if let Some(modules) = affected_modules {
                modules.contains(&spec.module)
            } else {
                true
            }
        })
        .collect();

    for spec in specs_to_evaluate {
        let Some(boundaries) = &spec.boundaries else {
            continue;
        };

        for contract in &boundaries.contracts {
            // Check contract file exists and is non-empty
            if let Some(v) = check_contract_file(ctx, spec, contract) {
                violations.push(v);
            }

            // Check match.files patterns resolve to actual files
            if let Some(v) = check_match_patterns(ctx, spec, contract) {
                violations.push(v);
            }

            // Check cross-module imports_contract references
            if let Some(v) = check_contract_refs(ctx, spec, contract, &contract_registry) {
                violations.push(v);
            }
        }
    }

    // Sort violations for deterministic output
    violations.sort_by(|a, b| {
        a.violation
            .from_file
            .cmp(&b.violation.from_file)
            .then_with(|| a.contract_id.cmp(&b.contract_id))
            .then_with(|| a.violation.rule.cmp(&b.violation.rule))
    });

    violations
}

/// Build a registry of all contracts for cross-module lookup.
/// Maps "module:contract_id" to the contract reference.
fn build_contract_registry(specs: &[SpecFile]) -> BTreeSet<String> {
    let mut registry = BTreeSet::new();

    for spec in specs {
        if let Some(boundaries) = &spec.boundaries {
            for contract in &boundaries.contracts {
                let key = format!("{}:{}", spec.module, contract.id);
                registry.insert(key);
            }
        }
    }

    registry
}

/// Check that the contract file exists and is non-empty.
/// Returns Some(violation) if the check fails, None if it passes.
fn check_contract_file(
    ctx: &RuleContext<'_>,
    spec: &SpecFile,
    contract: &BoundaryContract,
) -> Option<ContractRuleViolation> {
    let contract_path = ctx.project_root.join(&contract.contract);

    // Check file exists
    if !contract_path.exists() {
        return Some(ContractRuleViolation::new(
            RuleViolation {
                rule: BOUNDARY_CONTRACT_MISSING_RULE_ID.to_string(),
                message: format!(
                    "Contract file '{}' not found for contract '{}' in module '{}'",
                    contract.contract, contract.id, spec.module
                ),
                from_file: spec
                    .spec_path
                    .clone()
                    .unwrap_or_else(|| ctx.project_root.to_path_buf()),
                to_file: None,
                from_module: Some(spec.module.clone()),
                to_module: None,
                line: None,
                column: None,
            },
            format!(
                "Create the contract file at '{}' (relative to project root), or update `boundaries.contracts[].contract` to the correct existing path",
                contract.contract
            ),
            contract.id.clone(),
        ));
    }

    // Check file is not empty
    let metadata = fs::metadata(&contract_path).ok()?;
    if metadata.len() == 0 {
        return Some(ContractRuleViolation::new(
            RuleViolation {
                rule: BOUNDARY_CONTRACT_EMPTY_RULE_ID.to_string(),
                message: format!(
                    "Contract file '{}' is empty for contract '{}' in module '{}'",
                    contract.contract, contract.id, spec.module
                ),
                from_file: spec
                    .spec_path
                    .clone()
                    .unwrap_or_else(|| ctx.project_root.to_path_buf()),
                to_file: Some(contract_path),
                from_module: Some(spec.module.clone()),
                to_module: None,
                line: None,
                column: None,
            },
            format!(
                "Add schema/content to '{}' so the contract is non-empty, or remove the contract entry if it is no longer needed",
                contract.contract
            ),
            contract.id.clone(),
        ));
    }

    None
}

/// Check that match.files patterns resolve to at least one file.
/// Returns Some(violation) if no patterns resolve, None if at least one resolves.
fn check_match_patterns(
    ctx: &RuleContext<'_>,
    spec: &SpecFile,
    contract: &BoundaryContract,
) -> Option<ContractRuleViolation> {
    if contract.r#match.files.is_empty() {
        // No patterns to check - this is valid
        return None;
    }

    // Check each pattern to see if it resolves to any files
    let mut any_resolved = false;
    let mut failed_patterns = Vec::new();

    for pattern in &contract.r#match.files {
        // Use GlobBuilder with literal_separator(true) for accurate matching
        let glob = match GlobBuilder::new(pattern).literal_separator(true).build() {
            Ok(g) => g,
            Err(_) => {
                failed_patterns.push(pattern.clone());
                continue;
            }
        };

        let matcher = glob.compile_matcher();

        // Check if any file matches this pattern
        let matches = find_matching_files(ctx.project_root, &matcher);
        if !matches.is_empty() {
            any_resolved = true;
        } else {
            failed_patterns.push(pattern.clone());
        }
    }

    if !any_resolved && !contract.r#match.files.is_empty() {
        return Some(ContractRuleViolation::new(
            RuleViolation {
                rule: BOUNDARY_MATCH_UNRESOLVED_RULE_ID.to_string(),
                message: format!(
                    "Match patterns for contract '{}' in module '{}' did not resolve to any files: {:?}",
                    contract.id, spec.module, failed_patterns
                ),
                from_file: spec
                    .spec_path
                    .clone()
                    .unwrap_or_else(|| ctx.project_root.to_path_buf()),
                to_file: None,
                from_module: Some(spec.module.clone()),
                to_module: None,
                line: None,
                column: None,
            },
            format!(
                "Update `match.files` globs for contract '{}' so at least one path resolves, or add files that satisfy the declared glob patterns",
                contract.id
            ),
            contract.id.clone(),
        ));
    }

    None
}

/// Find all files matching a glob pattern under the project root.
fn find_matching_files(project_root: &Path, matcher: &globset::GlobMatcher) -> Vec<PathBuf> {
    let mut matches = Vec::new();

    for entry in walkdir::WalkDir::new(project_root)
        .follow_links(true)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_file() {
            let path = entry.path();
            let relative = match path.strip_prefix(project_root) {
                Ok(r) => r,
                Err(_) => path,
            };
            // Convert to string and normalize path separators
            let relative_str = relative.to_string_lossy().replace('\\', "/");
            if matcher.is_match(&relative_str) {
                matches.push(path.to_path_buf());
            }
        }
    }

    matches
}

/// Check that imports_contract references are valid (point to existing contracts in other modules).
/// Returns Some(violation) if any reference is invalid, None if all are valid.
fn check_contract_refs(
    _ctx: &RuleContext<'_>,
    spec: &SpecFile,
    contract: &BoundaryContract,
    registry: &BTreeSet<String>,
) -> Option<ContractRuleViolation> {
    if contract.imports_contract.is_empty() {
        return None;
    }

    let mut invalid_refs = Vec::new();

    for ref_str in &contract.imports_contract {
        // Validate format: "module:contract_id"
        let parts: Vec<&str> = ref_str.split(':').collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            invalid_refs.push((
                ref_str.clone(),
                "invalid format (expected 'module:contract_id')",
            ));
            continue;
        }

        // Check if the referenced contract exists in the registry
        if !registry.contains(ref_str) {
            invalid_refs.push((ref_str.clone(), "contract not found in target module"));
        }
    }

    if !invalid_refs.is_empty() {
        let ref_details: Vec<String> = invalid_refs
            .iter()
            .map(|(r, reason)| format!("'{r}' ({reason})"))
            .collect();

        return Some(ContractRuleViolation::new(
            RuleViolation {
                rule: BOUNDARY_CONTRACT_REF_INVALID_RULE_ID.to_string(),
                message: format!(
                    "Invalid imports_contract references for contract '{}' in module '{}': {}",
                    contract.id, spec.module, ref_details.join(", ")
                ),
                from_file: spec.spec_path.clone().unwrap_or_else(|| _ctx.project_root.to_path_buf()),
                to_file: None,
                from_module: Some(spec.module.clone()),
                to_module: None,
                line: None,
                column: None,
            },
            "Use valid `module:contract_id` references in `imports_contract`, or define the referenced contract ids in the target modules".to_string(),
            contract.id.clone(),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::ModuleResolver;
    use crate::spec::SpecConfig;
    use crate::spec::types::{Boundaries, ContractDirection, ContractMatch, EnvelopeRequirement};
    use tempfile::TempDir;

    fn spec_with_contracts(module: &str, contracts: Vec<BoundaryContract>) -> SpecFile {
        SpecFile {
            version: "2.3".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some(format!("src/{module}/**/*")),
                contracts,
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: Some(PathBuf::from(format!("{module}.spec.yml"))),
        }
    }

    fn create_test_contract(
        id: &str,
        contract_path: &str,
        files: Vec<&str>,
        imports_contract: Vec<&str>,
    ) -> BoundaryContract {
        BoundaryContract {
            id: id.to_string(),
            contract: contract_path.to_string(),
            r#match: ContractMatch {
                files: files.into_iter().map(String::from).collect(),
                pattern: None,
            },
            direction: ContractDirection::Bidirectional,
            envelope: EnvelopeRequirement::Optional,
            imports_contract: imports_contract.into_iter().map(String::from).collect(),
        }
    }

    fn build_graph(temp: &TempDir, specs: &[SpecFile]) -> crate::graph::DependencyGraph {
        let mut resolver = ModuleResolver::new(temp.path(), specs).expect("resolver");
        let config = SpecConfig::default();
        crate::graph::DependencyGraph::build(temp.path(), &mut resolver, &config)
            .expect("build graph")
    }

    fn create_test_context<'a>(
        project_root: &'a Path,
        config: &'a crate::spec::SpecConfig,
        specs: &'a [SpecFile],
        graph: &'a crate::graph::DependencyGraph,
    ) -> RuleContext<'a> {
        RuleContext {
            project_root,
            config,
            specs,
            graph,
        }
    }

    #[test]
    fn reports_missing_contract_file() {
        let temp = TempDir::new().expect("tempdir");

        // Create minimal source file so graph can be built
        fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
        fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation.rule,
            BOUNDARY_CONTRACT_MISSING_RULE_ID
        );
        assert_eq!(violations[0].contract_id, "contract1");
        assert!(
            violations[0]
                .remediation_hint
                .contains("Create the contract file")
        );
    }

    #[test]
    fn reports_empty_contract_file() {
        let temp = TempDir::new().expect("tempdir");

        // Create an empty contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        let contract_file = contracts_dir.join("api.json");
        fs::File::create(&contract_file).expect("create empty file");

        // Create minimal source file so graph can be built
        fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
        fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation.rule,
            BOUNDARY_CONTRACT_EMPTY_RULE_ID
        );
        assert_eq!(violations[0].contract_id, "contract1");
        assert!(
            violations[0]
                .remediation_hint
                .contains("Add schema/content")
        );
    }

    #[test]
    fn passes_valid_contract_file() {
        let temp = TempDir::new().expect("tempdir");

        // Create a non-empty contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        // Create minimal source file so graph can be built
        fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
        fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert!(violations.is_empty());
    }

    #[test]
    fn reports_unresolved_match_patterns() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        // Create some files, but not the ones that match the pattern
        fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
        fs::write(temp.path().join("src/api/existing.ts"), "export {}").expect("write existing");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec!["src/api/nonexistent/**/*.ts"],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation.rule,
            BOUNDARY_MATCH_UNRESOLVED_RULE_ID
        );
        assert_eq!(violations[0].contract_id, "contract1");
    }

    #[test]
    fn passes_resolved_match_patterns() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        // Create files that match the pattern
        fs::create_dir_all(temp.path().join("src/api/handlers")).expect("create handlers dir");
        fs::write(temp.path().join("src/api/handlers/get.ts"), "export {}").expect("write handler");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec!["src/api/handlers/*.ts"],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert!(violations.is_empty());
    }

    #[test]
    fn reports_invalid_contract_ref_format() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec!["invalid-format-without-colon"],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation.rule,
            BOUNDARY_CONTRACT_REF_INVALID_RULE_ID
        );
        assert!(violations[0].violation.message.contains("invalid format"));
    }

    #[test]
    fn reports_nonexistent_contract_ref() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file for api module
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        let specs = vec![
            spec_with_contracts(
                "api",
                vec![create_test_contract(
                    "contract1",
                    "contracts/api.json",
                    vec![],
                    vec!["other:nonexistent_contract"],
                )],
            ),
            // other module exists but doesn't have the referenced contract
            spec_with_contracts("other", vec![]),
        ];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].violation.rule,
            BOUNDARY_CONTRACT_REF_INVALID_RULE_ID
        );
        assert!(
            violations[0]
                .violation
                .message
                .contains("contract not found")
        );
    }

    #[test]
    fn passes_valid_contract_ref() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract files
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#)
            .expect("write api contract");
        fs::write(contracts_dir.join("other.yaml"), "type: object").expect("write other contract");

        let specs = vec![
            spec_with_contracts(
                "api",
                vec![create_test_contract(
                    "contract1",
                    "contracts/api.json",
                    vec![],
                    vec!["other:other_contract"],
                )],
            ),
            spec_with_contracts(
                "other",
                vec![create_test_contract(
                    "other_contract",
                    "contracts/other.yaml",
                    vec![],
                    vec![],
                )],
            ),
        ];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        // Should have no violations (valid ref + files exist)
        assert!(violations.is_empty());
    }

    #[test]
    fn affected_modules_filter_limits_evaluation() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract files
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#)
            .expect("write api contract");
        // Missing contract file for "other" module

        let specs = vec![
            spec_with_contracts(
                "api",
                vec![create_test_contract(
                    "api_contract",
                    "contracts/api.json",
                    vec![],
                    vec![],
                )],
            ),
            spec_with_contracts(
                "other",
                vec![create_test_contract(
                    "other_contract",
                    "contracts/missing.yaml",
                    vec![],
                    vec![],
                )],
            ),
        ];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        // Only evaluate "api" module - should not report missing contract for "other"
        let mut affected = BTreeSet::new();
        affected.insert("api".to_string());
        let violations = evaluate_contract_rules(&ctx, Some(&affected));

        assert!(violations.is_empty());

        // Evaluate both modules - should report missing contract for "other"
        let violations = evaluate_contract_rules(&ctx, None);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].violation.message.contains("other_contract"));
    }

    #[test]
    fn empty_affected_modules_returns_no_violations() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        // Empty affected_modules set means no modules should be evaluated
        let affected = BTreeSet::new();
        let violations = evaluate_contract_rules(&ctx, Some(&affected));

        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_violations_per_contract_are_reported() {
        let temp = TempDir::new().expect("tempdir");

        // Create a contract file that exists but is empty
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        let contract_file = contracts_dir.join("api.json");
        fs::File::create(&contract_file).expect("create empty file");

        // Create unmatched pattern and invalid ref
        fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");

        let specs = vec![spec_with_contracts(
            "api",
            vec![BoundaryContract {
                id: "multi_violation".to_string(),
                contract: "contracts/api.json".to_string(),
                r#match: ContractMatch {
                    files: vec!["src/nonexistent/**/*.ts".to_string()],
                    pattern: None,
                },
                direction: ContractDirection::Bidirectional,
                envelope: EnvelopeRequirement::Optional,
                imports_contract: vec!["invalid-ref".to_string()],
            }],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        // Should report: empty file, unresolved pattern, invalid ref
        assert_eq!(violations.len(), 3);

        let rules: Vec<_> = violations
            .iter()
            .map(|v| v.violation.rule.as_str())
            .collect();
        assert!(rules.contains(&BOUNDARY_CONTRACT_EMPTY_RULE_ID));
        assert!(rules.contains(&BOUNDARY_MATCH_UNRESOLVED_RULE_ID));
        assert!(rules.contains(&BOUNDARY_CONTRACT_REF_INVALID_RULE_ID));
    }

    #[test]
    fn glob_with_literal_separator_matches_correctly() {
        let temp = TempDir::new().expect("tempdir");

        // Create contract file
        let contracts_dir = temp.path().join("contracts");
        fs::create_dir_all(&contracts_dir).expect("create contracts dir");
        fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

        // Create nested file structure
        fs::create_dir_all(temp.path().join("src/api/handlers/nested")).expect("create nested");
        fs::write(temp.path().join("src/api/handlers/get.ts"), "export {}").expect("write get.ts");
        fs::write(
            temp.path().join("src/api/handlers/nested/post.ts"),
            "export {}",
        )
        .expect("write post.ts");

        // Pattern with literal_separator should match exact paths
        let specs = vec![spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec!["src/api/handlers/*.ts"], // Should only match get.ts, not nested/post.ts
                vec![],
            )],
        )];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        // Should pass because get.ts matches the pattern
        assert!(violations.is_empty());
    }

    #[test]
    fn missing_spec_path_uses_project_root() {
        let temp = TempDir::new().expect("tempdir");

        // Create a spec without spec_path
        let mut spec = spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/missing.json",
                vec![],
                vec![],
            )],
        );
        spec.spec_path = None;

        let specs = vec![spec];

        let graph = build_graph(&temp, &specs);
        let config = SpecConfig::default();
        let ctx = create_test_context(temp.path(), &config, &specs, &graph);

        let violations = evaluate_contract_rules(&ctx, None);

        assert_eq!(violations.len(), 1);
        // from_file should be project_root when spec_path is None
        assert_eq!(violations[0].violation.from_file, temp.path());
    }
}
