use std::path::{Path, PathBuf};

use specgate::parser::parse_file;

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tsjs-barrel")
        .join(relative)
}

#[test]
fn export_star_generates_single_reexport_edge() {
    let analysis = parse_file(&fixture("star/index.ts")).expect("parse star barrel");

    assert_eq!(analysis.re_exports.len(), 1);
    let edge = &analysis.re_exports[0];
    assert!(edge.is_star, "export * should be marked as star re-export");
    assert_eq!(edge.specifier, "./leaf");
    assert!(edge.names.is_empty());

    assert_eq!(analysis.dependency_specifiers(), vec!["./leaf".to_string()]);
}

#[test]
fn named_reexport_captures_exported_names() {
    let analysis = parse_file(&fixture("named/index.ts")).expect("parse named barrel");

    assert_eq!(analysis.re_exports.len(), 1);
    let edge = &analysis.re_exports[0];
    assert!(!edge.is_star);
    assert_eq!(edge.specifier, "./leaf");
    assert_eq!(
        edge.names,
        vec!["renamedFoo".to_string(), "bar".to_string()]
    );

    assert_eq!(analysis.dependency_specifiers(), vec!["./leaf".to_string()]);
}

#[test]
fn type_reexport_is_tracked_as_dependency_edge() {
    let analysis = parse_file(&fixture("type/index.ts")).expect("parse type barrel");

    assert_eq!(analysis.re_exports.len(), 1);
    let edge = &analysis.re_exports[0];
    assert!(!edge.is_star);
    assert_eq!(edge.specifier, "./types");
    assert_eq!(edge.names, vec!["SharedType".to_string()]);

    assert_eq!(
        analysis.dependency_specifiers(),
        vec!["./types".to_string()]
    );
}

#[test]
fn layered_barrels_have_bounded_file_edge_counts() {
    let layer1 = parse_file(&fixture("layered/src/layer1/index.ts")).expect("parse layer1");
    let layer2 = parse_file(&fixture("layered/src/layer2/index.ts")).expect("parse layer2");
    let entry = parse_file(&fixture("layered/src/entry.ts")).expect("parse entry");

    assert_eq!(
        layer1.re_exports.len(),
        2,
        "layer1 should emit one edge per declaration"
    );
    assert_eq!(
        layer2.re_exports.len(),
        2,
        "layer2 should emit one edge per declaration"
    );

    let type_only_imports = entry
        .imports
        .iter()
        .filter(|import| import.is_type_only)
        .count();
    let runtime_imports = entry
        .imports
        .iter()
        .filter(|import| !import.is_type_only)
        .count();
    assert_eq!(
        type_only_imports, 1,
        "entry should preserve one type-only import"
    );
    assert_eq!(
        runtime_imports, 1,
        "entry should preserve one runtime import"
    );

    let total_dependency_edges = layer1.dependency_specifiers().len()
        + layer2.dependency_specifiers().len()
        + entry.dependency_specifiers().len();

    assert_eq!(
        total_dependency_edges, 5,
        "layered barrels should stay bounded to declaration-level/file-level edges"
    );
}
