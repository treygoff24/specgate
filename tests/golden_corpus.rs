//! Golden Corpus Integration Tests
//!
//! These tests execute the golden corpus fixtures deterministically,
//! verifying that Specgate catches the intended bug patterns.
//!
//! ## Status Key
//!
//! - ✅ **Direct Detection**: Pattern catchable with current Specgate rules
//! - ⚠️ **Future Enhancement**: Requires rule not yet implemented; fixture demonstrates intended behavior
//! - ⚠️ **Semantic Proxy**: Requires semantic analysis beyond current capabilities; serves as proxy for future enhancement

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
}

/// Install npm dependencies if package.json exists
fn install_npm_deps(project_root: &std::path::Path) {
    if project_root.join("package.json").exists() {
        let result = Command::new("npm")
            .args(["install"])
            .current_dir(project_root)
            .output();

        if let Ok(output) = result {
            if !output.status.success() {
                eprintln!(
                    "npm install warning: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

fn copy_files(src_dir: &std::path::Path, dest_dir: &std::path::Path) {
    fs::create_dir_all(dest_dir).expect("create dest dir");
    for entry in fs::read_dir(src_dir).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let src_path = entry.path();
        let dest_path = dest_dir.join(entry.file_name());

        if src_path.is_dir() {
            copy_files(&src_path, &dest_path);
        } else {
            fs::copy(&src_path, &dest_path).expect("copy file");
        }
    }
}

// =============================================================================
// C02: Mass-Assignment Vulnerability
// Status: ⚠️ Future Enhancement - requires 'no-pattern' rule
// =============================================================================

/// Test that C02 intro validates without errors (rule not yet implemented)
#[test]
fn c02_mass_assignment_intro_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c02-mass-assignment").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source (vulnerable)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c02-mass-assignment")
            .join("src")
            .join("handlers-intro.ts"),
        temp.path().join("src").join("handlers.ts"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c02-mass-assignment")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // NOTE: This will PASS until 'no-pattern' rule is implemented
    // The fixture is a "future enhancement" demonstrating intended behavior
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C02 intro should pass (future enhancement - rule not yet implemented): stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that C02 fix validates
#[test]
fn c02_mass_assignment_fix_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c02-mass-assignment").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source (hardened)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c02-mass-assignment")
            .join("src")
            .join("handlers-fix.ts"),
        temp.path().join("src").join("handlers.ts"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c02-mass-assignment")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C02 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// C06: Duplicate Object Key Shadowing
// Status: ⚠️ Future Enhancement - requires 'no-pattern' rule
// =============================================================================

/// Test that C06 intro validates (rule not yet implemented)
#[test]
fn c06_duplicate_key_intro_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c06-duplicate-key").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c06-duplicate-key")
            .join("src")
            .join("utils-intro.js"),
        temp.path().join("src").join("utils.js"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c06-duplicate-key")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // NOTE: This will PASS until 'no-pattern' rule is implemented
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C06 intro should pass (future enhancement - rule not yet implemented): stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that C06 fix validates
#[test]
fn c06_duplicate_key_fix_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c06-duplicate-key").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c06-duplicate-key")
            .join("src")
            .join("utils-fix.js"),
        temp.path().join("src").join("utils.js"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c06-duplicate-key")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C06 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// C07: Registry Collision (Duplicate Tool Definitions)
// Status: ⚠️ Future Enhancement - requires 'boundary.unique_export' rule
// =============================================================================

/// Test that C07 intro validates (rule not yet implemented)
#[test]
fn c07_registry_collision_intro_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("c07-registry-collision")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro sources (all intro files)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("attachments-intro.ts"),
        temp.path().join("src").join("attachments.ts"),
    )
    .expect("copy attachments");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("notes-intro.ts"),
        temp.path().join("src").join("notes.ts"),
    )
    .expect("copy notes");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("registry-intro.ts"),
        temp.path().join("src").join("registry.ts"),
    )
    .expect("copy registry");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // NOTE: This will PASS until 'boundary.unique_export' rule is implemented
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C07 intro should pass (future enhancement - rule not yet implemented): stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that C07 fix validates
#[test]
fn c07_registry_collision_fix_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("c07-registry-collision")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix sources
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("attachments-fix.ts"),
        temp.path().join("src").join("attachments.ts"),
    )
    .expect("copy attachments");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("notes-fix.ts"),
        temp.path().join("src").join("notes.ts"),
    )
    .expect("copy notes");
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("src")
            .join("registry-fix.ts"),
        temp.path().join("src").join("registry.ts"),
    )
    .expect("copy registry");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c07-registry-collision")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C07 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// C08: Layer Inversion (Protocol Policy Conflation)
// Status: ⚠️ Semantic Proxy - enforce-layer exists but requires layer annotations
// =============================================================================

/// Test that C08 intro validates with enforce-layer
#[test]
fn c08_layer_inversion_intro_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c08-layer-inversion").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c08-layer-inversion")
            .join("src")
            .join("originGuard-intro.ts"),
        temp.path().join("src").join("originGuard.ts"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c08-layer-inversion")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // NOTE: enforce-layer passes but doesn't detect the semantic issue
    // (shared guard without null handling) - this is a semantic proxy fixture
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C08 intro should pass (semantic proxy - full detection requires future enhancement): stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that C08 fix validates
#[test]
fn c08_layer_inversion_fix_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c08-layer-inversion").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c08-layer-inversion")
            .join("src")
            .join("originGuard-fix.ts"),
        temp.path().join("src").join("originGuard.ts"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c08-layer-inversion")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C08 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// C09: Public API Leakage (Internal Object Exposed)
// Status: ⚠️ Semantic Proxy - boundary.public_api exists but controls imports, not return types
// =============================================================================

/// Test that C09 intro validates with boundary.public_api
#[test]
fn c09_api_leakage_intro_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c09-api-leakage").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c09-api-leakage")
            .join("src")
            .join("webhookIngress-intro.ts"),
        temp.path().join("src").join("webhookIngress.ts"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c09-api-leakage")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // NOTE: boundary.public_api passes but doesn't detect type leakage
    // (raw Server in return type) - this is a semantic proxy fixture
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C09 intro should pass (semantic proxy - full detection requires future type-leakage analysis): stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that C09 fix validates
#[test]
fn c09_api_leakage_fix_validates() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir().join("c09-api-leakage").join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("c09-api-leakage")
            .join("src")
            .join("webhookIngress-fix.ts"),
        temp.path().join("src").join("webhookIngress.ts"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("c09-api-leakage")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "C09 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// D01: Forbidden Third-Party Dependency
// Status: ✅ Direct Detection - dependency.forbidden rule catches this
// =============================================================================

/// Test that D01 intro fails due to forbidden dependency
#[test]
fn d01_forbidden_third_party_intro_fails() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source (uses forbidden lodash)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("src")
            .join("utils-intro.ts"),
        temp.path().join("src").join("utils.ts"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // Copy package.json and install deps
    fs::copy(
        fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("package.json"),
        temp.path().join("package.json"),
    )
    .expect("copy package.json");
    install_npm_deps(temp.path());

    // D01 intro should fail due to forbidden dependency
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "D01 intro should fail due to forbidden dependency: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that D01 fix passes (no forbidden dependencies)
#[test]
fn d01_forbidden_third_party_fix_passes() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source (uses native JS only)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("src")
            .join("utils-fix.ts"),
        temp.path().join("src").join("utils.ts"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("d01-forbidden-third-party")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "D01 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

// =============================================================================
// D02: Dependency Not Allowed (whitelist violation)
// Status: ✅ Direct Detection - dependency.not_allowed rule catches this
// =============================================================================

/// Test that D02 intro fails due to dependency not in allowed list
#[test]
fn d02_dependency_not_allowed_intro_fails() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy intro source (uses 'got' which is not in allowed list)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("src")
            .join("client-intro.ts"),
        temp.path().join("src").join("client.ts"),
    )
    .expect("copy intro file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    // Copy package.json and install deps
    fs::copy(
        fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("package.json"),
        temp.path().join("package.json"),
    )
    .expect("copy package.json");
    install_npm_deps(temp.path());

    // D02 intro should fail due to dependency not in allowed list
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "D02 intro should fail due to dependency not allowed: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}

/// Test that D02 fix passes (uses allowed dependency)
#[test]
fn d02_dependency_not_allowed_fix_passes() {
    let temp = TempDir::new().expect("tempdir");

    // Copy spec files
    copy_files(
        &fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("modules"),
        &temp.path().join("modules"),
    );

    // Copy fix source (uses 'axios' which is in allowed list)
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("src")
            .join("client-fix.ts"),
        temp.path().join("src").join("client.ts"),
    )
    .expect("copy fix file");

    // Copy config
    fs::copy(
        fixtures_dir()
            .join("d02-dependency-not-allowed")
            .join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "D02 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
}
