//! Cross-file compensation logic for policy-diff.
//!
//! Scoped compensation: a narrowing in module A can offset a widening in module B
//! only if A and B share a direct dependency relationship, and the changes are in
//! the same field family. Ambiguous cases fail closed.

use std::collections::BTreeSet;

use super::types::{
    ChangeClassification, CompensationCandidate, CompensationResult, DependencyEdge, FieldChange,
};

/// Extract the "field family" from a field name for compensation matching.
/// Same field name = same family.
fn field_family(field: &str) -> &str {
    field
}

/// Check if two modules share a direct dependency edge (either direction).
fn find_edge<'a>(
    module_a: &str,
    module_b: &str,
    edges: &'a [DependencyEdge],
) -> Option<&'a DependencyEdge> {
    edges.iter().find(|e| {
        (e.importer == module_a && e.provider == module_b)
            || (e.importer == module_b && e.provider == module_a)
    })
}

/// Find compensation candidates between widenings and narrowings.
///
/// Rules:
/// - Same field family only
/// - Direct dependency relationship required (typed `DependencyEdge`)
/// - If a narrowing could offset multiple widenings (or vice versa), mark as `Ambiguous`
pub fn find_compensation_candidates(
    widenings: &[FieldChange],
    narrowings: &[FieldChange],
    edges: &[DependencyEdge],
) -> Vec<CompensationCandidate> {
    let mut candidates = Vec::new();

    for narrowing in narrowings {
        debug_assert_eq!(narrowing.classification, ChangeClassification::Narrowing);

        let compatible: Vec<(&FieldChange, &DependencyEdge)> = widenings
            .iter()
            .filter_map(|w| {
                if w.classification != ChangeClassification::Widening {
                    return None;
                }
                if field_family(&w.field) != field_family(&narrowing.field) {
                    return None;
                }
                if w.module == narrowing.module {
                    return None;
                }
                find_edge(&w.module, &narrowing.module, edges).map(|e| (w, e))
            })
            .collect();

        if compatible.len() == 1 {
            let (widening, edge) = compatible[0];
            candidates.push(CompensationCandidate {
                widening: widening.clone(),
                narrowing: narrowing.clone(),
                relationship: edge.clone(),
                result: CompensationResult::Offset,
            });
        } else if compatible.len() > 1 {
            // Ambiguous: one narrowing matches multiple widenings — emit all as Ambiguous
            for (widening, edge) in &compatible {
                candidates.push(CompensationCandidate {
                    widening: (*widening).clone(),
                    narrowing: narrowing.clone(),
                    relationship: (*edge).clone(),
                    result: CompensationResult::Ambiguous,
                });
            }
        }
    }

    // Dedup: if multiple narrowings matched the same widening, mark those as Ambiguous
    let mut widening_keys: BTreeSet<(String, String)> = BTreeSet::new();
    let mut duplicate_keys: BTreeSet<(String, String)> = BTreeSet::new();
    for c in &candidates {
        if c.result == CompensationResult::Offset {
            let key = (c.widening.module.clone(), c.widening.field.clone());
            if !widening_keys.insert(key.clone()) {
                duplicate_keys.insert(key);
            }
        }
    }
    for c in &mut candidates {
        let key = (c.widening.module.clone(), c.widening.field.clone());
        if duplicate_keys.contains(&key) {
            c.result = CompensationResult::Ambiguous;
        }
    }

    candidates
}

/// Extract dependency edges from HEAD specs' `allow_imports_from` fields.
pub fn dependency_edges_from_specs(specs: &[crate::spec::SpecFile]) -> Vec<DependencyEdge> {
    let mut edges = Vec::new();
    for spec in specs {
        if let Some(boundaries) = &spec.boundaries {
            if let Some(allowed) = &boundaries.allow_imports_from {
                for provider in allowed {
                    edges.push(DependencyEdge {
                        importer: spec.module.clone(),
                        provider: provider.clone(),
                    });
                }
            }
        }
    }
    edges
}
