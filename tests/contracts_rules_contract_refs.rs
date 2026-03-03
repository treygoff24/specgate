//! Contract Rules Contract Refs Regression Tests (W5-T1b)
//!
//! Focused regression tests for:
//! 1. Cross-module imports_contract validation behavior
//! 2. Affected_modules scoping behavior for --since path
//! 3. Contract_id and remediation_hint emission in violations

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use specgate::graph::DependencyGraph;
use specgate::resolver::ModuleResolver;
use specgate::rules::contracts::{
    BOUNDARY_CONTRACT_EMPTY_RULE_ID, BOUNDARY_CONTRACT_MISSING_RULE_ID,
    BOUNDARY_CONTRACT_REF_INVALID_RULE_ID, BOUNDARY_MATCH_UNRESOLVED_RULE_ID,
    evaluate_contract_rules,
};
use specgate::spec::SpecConfig;
use specgate::spec::types::{
    Boundaries, BoundaryContract, ContractDirection, ContractMatch, EnvelopeRequirement,
};

fn spec_with_contracts(
    module: &str,
    contracts: Vec<BoundaryContract>,
) -> specgate::spec::types::SpecFile {
    specgate::spec::types::SpecFile {
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
        spec_path: Some(Path::new(module).join(format!("{module}.spec.yml"))),
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

fn build_graph(temp: &TempDir, specs: &[specgate::spec::types::SpecFile]) -> DependencyGraph {
    let mut resolver = ModuleResolver::new(temp.path(), specs).expect("resolver");
    let config = SpecConfig::default();
    DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build graph")
}

fn create_test_context<'a>(
    project_root: &'a Path,
    config: &'a SpecConfig,
    specs: &'a [specgate::spec::types::SpecFile],
    graph: &'a DependencyGraph,
) -> specgate::rules::RuleContext<'a> {
    specgate::rules::RuleContext {
        project_root,
        config,
        specs,
        graph,
    }
}

// =============================================================================
// Test Group 1: Cross-Module imports_contract Validation Behavior
// =============================================================================

/// Test that valid cross-module imports_contract references pass validation.
#[test]
fn cross_module_imports_contract_valid_reference_passes() {
    let temp = TempDir::new().expect("tempdir");

    // Create contract files for both modules
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write api contract");
    fs::write(contracts_dir.join("shared.yaml"), "type: object").expect("write shared contract");

    // Create source files
    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::create_dir_all(temp.path().join("src/shared")).expect("create src/shared");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");
    fs::write(temp.path().join("src/shared/index.ts"), "export {}").expect("write source");

    let specs = vec![
        spec_with_contracts(
            "api",
            vec![create_test_contract(
                "user_contract",
                "contracts/api.json",
                vec![],
                vec!["shared:user_contract"],
            )],
        ),
        spec_with_contracts(
            "shared",
            vec![create_test_contract(
                "user_contract",
                "contracts/shared.yaml",
                vec![],
                vec![],
            )],
        ),
    ];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert!(
        violations.is_empty(),
        "valid cross-module imports_contract should pass: {violations:?}"
    );
}

/// Test that invalid imports_contract reference (non-existent contract) is detected.
#[test]
fn cross_module_imports_contract_nonexistent_contract_detected() {
    let temp = TempDir::new().expect("tempdir");

    // Create contract file for api module
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    // Create source files
    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::create_dir_all(temp.path().join("src/other")).expect("create src/other");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");
    fs::write(temp.path().join("src/other/index.ts"), "export {}").expect("write source");

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
            .contains("nonexistent_contract")
    );
}

/// Test that invalid imports_contract format (missing colon) is detected.
#[test]
fn cross_module_imports_contract_missing_colon_detected() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "contract1",
            "contracts/api.json",
            vec![],
            vec!["invalid-format"],
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

/// Test that empty module segment in imports_contract is detected.
/// Note: The rules engine reports this as "invalid format" (consolidated message).
#[test]
fn cross_module_imports_contract_empty_module_segment_detected() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "contract1",
            "contracts/api.json",
            vec![],
            vec![":contract_id"],
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert_eq!(violations.len(), 1);
    // Rules engine uses consolidated "invalid format" message
    assert!(violations[0].violation.message.contains("invalid format"));
}

/// Test that empty contract id segment in imports_contract is detected.
/// Note: The rules engine reports this as "invalid format" (consolidated message).
#[test]
fn cross_module_imports_contract_empty_contract_id_detected() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "contract1",
            "contracts/api.json",
            vec![],
            vec!["module:"],
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert_eq!(violations.len(), 1);
    // Rules engine uses consolidated "invalid format" message
    assert!(violations[0].violation.message.contains("invalid format"));
}

// =============================================================================
// Test Group 2: Affected_Modules Scoping Behavior for --since Path
// =============================================================================

/// Test that affected_modules filter limits evaluation to specified modules.
#[test]
fn affected_modules_filter_limits_evaluation() {
    let temp = TempDir::new().expect("tempdir");

    // Create valid contract file for api module
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write api contract");

    // Create source files
    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::create_dir_all(temp.path().join("src/other")).expect("create src/other");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");
    fs::write(temp.path().join("src/other/index.ts"), "export {}").expect("write source");

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
                "contracts/missing.yaml", // Missing contract file
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

    assert!(
        violations.is_empty(),
        "affected_modules filter should limit evaluation: {violations:?}"
    );

    // Evaluate all modules - should report missing contract for "other"
    let all_violations = evaluate_contract_rules(&ctx, None);
    assert_eq!(all_violations.len(), 1);
    assert!(
        all_violations[0]
            .violation
            .message
            .contains("other_contract")
    );
}

/// Test that empty affected_modules set returns no violations.
#[test]
fn empty_affected_modules_returns_no_violations() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

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

    // Empty affected_modules set means no modules should be evaluated
    let affected = BTreeSet::new();
    let violations = evaluate_contract_rules(&ctx, Some(&affected));

    assert!(violations.is_empty());
}

/// Test that multiple affected modules are all evaluated.
#[test]
fn multiple_affected_modules_all_evaluated() {
    let temp = TempDir::new().expect("tempdir");

    // Create contracts directory but no contract files (will trigger violations)
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");

    // Create source files
    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::create_dir_all(temp.path().join("src/other")).expect("create src/other");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");
    fs::write(temp.path().join("src/other/index.ts"), "export {}").expect("write source");

    let specs = vec![
        spec_with_contracts(
            "api",
            vec![create_test_contract(
                "contract1",
                "contracts/api.json",
                vec![],
                vec![],
            )],
        ),
        spec_with_contracts(
            "other",
            vec![create_test_contract(
                "contract2",
                "contracts/other.json",
                vec![],
                vec![],
            )],
        ),
    ];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    // Evaluate both modules
    let mut affected = BTreeSet::new();
    affected.insert("api".to_string());
    affected.insert("other".to_string());
    let violations = evaluate_contract_rules(&ctx, Some(&affected));

    assert_eq!(violations.len(), 2);
}

// =============================================================================
// Test Group 3: Contract_ID and Remediation_Hint Emission in Violations
// =============================================================================

/// Test that contract_id is correctly emitted in contract_missing violations.
#[test]
fn contract_id_emitted_in_missing_violation() {
    let temp = TempDir::new().expect("tempdir");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "my_contract",
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
    assert_eq!(violations[0].contract_id, "my_contract");
    assert_eq!(
        violations[0].violation.rule,
        BOUNDARY_CONTRACT_MISSING_RULE_ID
    );
}

/// Test that contract_id is correctly emitted in contract_empty violations.
#[test]
fn contract_id_emitted_in_empty_violation() {
    let temp = TempDir::new().expect("tempdir");

    // Create an empty contract file
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    let contract_file = contracts_dir.join("api.json");
    fs::File::create(&contract_file).expect("create empty file");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "empty_contract",
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
    assert_eq!(violations[0].contract_id, "empty_contract");
    assert_eq!(
        violations[0].violation.rule,
        BOUNDARY_CONTRACT_EMPTY_RULE_ID
    );
}

/// Test that contract_id is correctly emitted in match_unresolved violations.
#[test]
fn contract_id_emitted_in_unresolved_match_violation() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/existing.ts"), "export {}").expect("write existing");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "unresolved_pattern",
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
    assert_eq!(violations[0].contract_id, "unresolved_pattern");
    assert_eq!(
        violations[0].violation.rule,
        BOUNDARY_MATCH_UNRESOLVED_RULE_ID
    );
}

/// Test that contract_id is correctly emitted in contract_ref_invalid violations.
#[test]
fn contract_id_emitted_in_invalid_ref_violation() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "bad_ref_contract",
            "contracts/api.json",
            vec![],
            vec!["other:missing"],
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].contract_id, "bad_ref_contract");
    assert_eq!(
        violations[0].violation.rule,
        BOUNDARY_CONTRACT_REF_INVALID_RULE_ID
    );
}

/// Test that remediation_hint is present and meaningful in missing contract violations.
#[test]
fn remediation_hint_emitted_in_missing_violation() {
    let temp = TempDir::new().expect("tempdir");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "contract1",
            "contracts/my_contract.json",
            vec![],
            vec![],
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0]
            .remediation_hint
            .contains("Create the contract file"),
        "remediation_hint should suggest creating the contract file: {}",
        violations[0].remediation_hint
    );
    assert!(
        violations[0]
            .remediation_hint
            .contains("contracts/my_contract.json"),
        "remediation_hint should mention the contract path: {}",
        violations[0].remediation_hint
    );
}

/// Test that remediation_hint is present and meaningful in empty contract violations.
#[test]
fn remediation_hint_emitted_in_empty_violation() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    let contract_file = contracts_dir.join("api.json");
    fs::File::create(&contract_file).expect("create empty file");

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
    assert!(
        violations[0]
            .remediation_hint
            .contains("Add schema/content"),
        "remediation_hint should suggest adding content: {}",
        violations[0].remediation_hint
    );
}

/// Test that remediation_hint is present and meaningful in unresolved match violations.
#[test]
fn remediation_hint_emitted_in_unresolved_match_violation() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/existing.ts"), "export {}").expect("write source");

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
    assert!(
        violations[0]
            .remediation_hint
            .contains("`match.files` globs"),
        "remediation_hint should mention match patterns: {}",
        violations[0].remediation_hint
    );
}

/// Test that remediation_hint is present and meaningful in invalid contract ref violations.
#[test]
fn remediation_hint_emitted_in_invalid_ref_violation() {
    let temp = TempDir::new().expect("tempdir");

    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");
    fs::write(temp.path().join("src/api/index.ts"), "export {}").expect("write source");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "contract1",
            "contracts/api.json",
            vec![],
            vec!["other:bad_ref"],
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].remediation_hint.contains("imports_contract"),
        "remediation_hint should mention imports_contract: {}",
        violations[0].remediation_hint
    );
    assert!(
        violations[0]
            .remediation_hint
            .contains("module:contract_id"),
        "remediation_hint should explain the expected format: {}",
        violations[0].remediation_hint
    );
}

/// Test that multiple violations on the same contract each have correct contract_id.
#[test]
fn multiple_violations_same_contract_have_correct_ids() {
    let temp = TempDir::new().expect("tempdir");

    // Create an empty contract file
    let contracts_dir = temp.path().join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts dir");
    let contract_file = contracts_dir.join("api.json");
    fs::File::create(&contract_file).expect("create empty file");

    fs::create_dir_all(temp.path().join("src/api")).expect("create src/api");

    let specs = vec![spec_with_contracts(
        "api",
        vec![create_test_contract(
            "multi_violation_contract",
            "contracts/api.json",
            vec!["src/nonexistent/**/*.ts"], // Unresolved pattern
            vec!["invalid-ref"],             // Invalid ref
        )],
    )];

    let graph = build_graph(&temp, &specs);
    let config = SpecConfig::default();
    let ctx = create_test_context(temp.path(), &config, &specs, &graph);

    let violations = evaluate_contract_rules(&ctx, None);

    // Should have 3 violations: empty file, unresolved pattern, invalid ref
    assert_eq!(violations.len(), 3);

    // All violations should have the same contract_id
    for v in &violations {
        assert_eq!(v.contract_id, "multi_violation_contract");
    }
}
