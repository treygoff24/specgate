use std::fs;
use std::path::PathBuf;

use specgate::spec::{discover_specs, load_config};

fn fixture_root(relative_path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative_path)
}

#[test]
fn monorepo_fixture_has_nested_configs_and_discoverable_specs() {
    let root = fixture_root("doctor-parity/monorepo-root");

    let root_config = root.join("specgate.config.yml");
    let web_config = root.join("packages/web/specgate.config.yml");
    let shared_config = root.join("packages/shared/specgate.config.yml");

    assert!(root_config.exists(), "missing root specgate.config.yml");
    assert!(
        web_config.exists(),
        "missing packages/web/specgate.config.yml"
    );
    assert!(
        shared_config.exists(),
        "missing packages/shared/specgate.config.yml"
    );

    let root_specs = discover_specs(&root, &load_config(&root).expect("load root config"))
        .expect("discover root specs");
    assert_eq!(root_specs.len(), 1, "expected one root-level spec");
    assert_eq!(root_specs[0].module, "root");

    let web_root = root.join("packages/web");
    let web_specs = discover_specs(
        &web_root,
        &load_config(&web_root).expect("load web package config"),
    )
    .expect("discover web specs");
    assert_eq!(web_specs.len(), 1, "expected one web package spec");
    assert_eq!(web_specs[0].module, "web");

    let shared_root = root.join("packages/shared");
    let shared_specs = discover_specs(
        &shared_root,
        &load_config(&shared_root).expect("load shared package config"),
    )
    .expect("discover shared specs");
    assert_eq!(shared_specs.len(), 1, "expected one shared package spec");
    assert_eq!(shared_specs[0].module, "shared");
}

#[test]
fn project_ref_fixture_references_module_by_id() {
    let root = fixture_root("doctor-parity/project-ref");
    let specs = discover_specs(&root, &load_config(&root).expect("load fixture config"))
        .expect("discover project-ref specs");

    assert_eq!(specs.len(), 2, "expected app and shared specs");

    let app = specs
        .iter()
        .find(|spec| spec.module == "app")
        .expect("app spec present");
    let shared = specs
        .iter()
        .find(|spec| spec.module == "shared")
        .expect("shared spec present");

    let app_allow = app
        .boundaries
        .as_ref()
        .and_then(|b| b.allow_imports_from.as_ref())
        .expect("app allow_imports_from configured");
    assert!(
        app_allow.iter().any(|module| module == "shared"),
        "app fixture should reference shared module by ID"
    );

    let shared_ids = shared.canonical_import_ids();
    assert!(
        shared_ids.contains(&"@shared".to_string()),
        "shared fixture should expose canonical import id"
    );
    assert!(
        shared_ids.contains(&"@repo/shared".to_string()),
        "shared fixture should expose alias import id"
    );

    let app_spec_text =
        fs::read_to_string(root.join("modules/app.spec.yml")).expect("read app spec");
    assert!(
        app_spec_text.contains("allow_imports_from:"),
        "app fixture should explicitly model module-id references"
    );
}
