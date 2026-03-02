//! Contract validation fixture tests for W4-T2
//!
//! These tests verify contract validation by loading individual spec files
//! and checking validation outcomes.

use std::path::PathBuf;

use specgate::spec::types::SUPPORTED_SPEC_VERSIONS;
use specgate::spec::{load_spec, validate_specs};

fn fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/contract-validation")
        .join(filename)
}

/// Test that a valid 2.3 spec with contracts passes validation
#[test]
fn valid_2_3_with_contracts_passes() {
    let path = fixture_path("valid-2.3-with-contracts.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    // Verify it's version 2.3
    assert_eq!(spec.version, "2.3");

    // Verify contracts exist (clone to avoid partial move)
    let boundaries = spec.boundaries.clone().expect("should have boundaries");
    assert_eq!(boundaries.contracts.len(), 3);

    // Validate the spec - should pass
    let report = validate_specs(&[spec]);
    assert!(
        !report.has_errors(),
        "valid 2.3 spec with contracts should have no errors"
    );
}

/// Test that 2.2 spec with contracts fails with version mismatch error
#[test]
fn invalid_2_2_with_contracts_fails() {
    let path = fixture_path("invalid-2.2-with-contracts.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    // Verify it's version 2.2
    assert_eq!(spec.version, "2.2");

    // Verify contracts exist (clone to avoid partial move)
    let boundaries = spec.boundaries.clone().expect("should have boundaries");
    assert_eq!(boundaries.contracts.len(), 1);

    // Validate the spec - should fail with version mismatch
    let report = validate_specs(&[spec]);
    assert!(report.has_errors(), "2.2 spec with contracts should fail");

    let errors = report.errors();
    assert!(
        errors
            .iter()
            .any(|e| { e.message.contains("boundary.contract_version_mismatch") }),
        "should report contract version mismatch error"
    );
}

/// Test that duplicate contract IDs are detected
#[test]
fn duplicate_contract_ids_detected() {
    let path = fixture_path("duplicate-contract-ids.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    // Validate the spec - should detect duplicate contract IDs
    let report = validate_specs(&[spec]);
    assert!(
        report.has_errors(),
        "duplicate contract IDs should be detected"
    );

    let errors = report.errors();
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.contract_ref_invalid")
                && e.message.contains("duplicate contract id")
        }),
        "should report duplicate contract id error"
    );
}

/// Test that invalid glob patterns in match.files are detected
#[test]
fn invalid_glob_pattern_detected() {
    let path = fixture_path("invalid-glob-pattern.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    // Validate the spec - should detect invalid glob pattern
    let report = validate_specs(&[spec]);
    assert!(
        report.has_errors(),
        "invalid glob pattern should be detected"
    );

    let errors = report.errors();
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.match_unresolved")
                && e.message.contains("invalid glob pattern")
        }),
        "should report invalid glob pattern error"
    );
}

/// Test that invalid imports_contract references are detected
#[test]
fn invalid_imports_contract_ref_detected() {
    let path = fixture_path("invalid-imports-contract-ref.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    // Validate the spec - should detect invalid imports_contract refs
    let report = validate_specs(&[spec]);
    assert!(
        report.has_errors(),
        "invalid imports_contract refs should be detected"
    );

    let errors = report.errors();

    // Check for missing colon error
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.contract_ref_invalid")
                && e.message.contains("exactly one colon")
        }),
        "should report missing colon error"
    );

    // Check for multiple colons error
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.contract_ref_invalid")
                && e.message.contains("exactly one colon")
        }),
        "should report multiple colons error"
    );

    // Check for empty module segment
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.contract_ref_invalid")
                && e.message.contains("empty module segment")
        }),
        "should report empty module segment error"
    );

    // Check for empty contract id segment
    assert!(
        errors.iter().any(|e| {
            e.message.contains("boundary.contract_ref_invalid")
                && e.message.contains("empty contract id segment")
        }),
        "should report empty contract id segment error"
    );
}

/// Test that valid 2.3 spec fixture contains expected contract fields
#[test]
fn valid_2_3_contract_structure() {
    let path = fixture_path("valid-2.3-with-contracts.spec.yml");
    let spec = load_spec(&path).expect("should load spec file");

    assert_eq!(spec.module, "api");

    let boundaries = spec.boundaries.expect("should have boundaries");
    assert_eq!(boundaries.contracts.len(), 3);

    // Check first contract
    let first = &boundaries.contracts[0];
    assert_eq!(first.id, "create_user");
    assert_eq!(first.contract, "contracts/create_user.json");
    assert_eq!(first.r#match.files.len(), 1);
    assert_eq!(first.r#match.files[0], "src/api/handlers.ts");

    // Check contract with imports_contract
    let third = &boundaries.contracts[2];
    assert_eq!(third.id, "delete_user");
    assert_eq!(third.imports_contract.len(), 1);
    assert_eq!(third.imports_contract[0], "db:user_deletion");
}

/// Test that SUPPORTED_SPEC_VERSIONS includes 2.3 for contract support
#[test]
fn supported_versions_include_2_3() {
    assert!(
        SUPPORTED_SPEC_VERSIONS.contains(&"2.3"),
        "2.3 should be a supported spec version for contracts"
    );
}
