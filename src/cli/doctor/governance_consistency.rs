use std::collections::BTreeMap;

use clap::Args;
use serde::Serialize;

use crate::cli::{
    CliRunResult, CommonProjectArgs, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, load_project,
    runtime_error_json,
};
use crate::spec::SpecFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum GovernanceConsistencyFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorGovernanceConsistencyArgs {
    #[command(flatten)]
    pub(super) common: CommonProjectArgs,
    /// Output format: human or json
    #[arg(long, default_value = "human")]
    pub(super) format: GovernanceConsistencyFormat,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GovernanceConflict {
    /// The module where the conflict was detected.
    pub(crate) module: String,
    /// The type of conflict detected.
    pub(crate) conflict_type: String,
    /// Human-readable description of the conflict.
    pub(crate) description: String,
    /// The spec file path where the conflict originates (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) spec_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GovernanceConsistencyOutput {
    schema_version: String,
    status: String,
    conflict_count: usize,
    conflicts: Vec<GovernanceConflict>,
}

pub(super) fn handle_doctor_governance_consistency(
    args: DoctorGovernanceConsistencyArgs,
) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    if loaded.validation.has_errors() {
        let details = loaded
            .validation
            .errors()
            .into_iter()
            .map(|issue| format!("{}: {}", issue.module, issue.message))
            .collect();
        return runtime_error_json(
            "validation",
            "spec validation failed; run `specgate validate` for details",
            details,
        );
    }

    let conflicts = detect_governance_conflicts(&loaded.specs);

    let status = if conflicts.is_empty() {
        "ok"
    } else {
        "conflicts"
    }
    .to_string();

    let exit_code = if conflicts.is_empty() {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_POLICY_VIOLATIONS
    };

    match args.format {
        GovernanceConsistencyFormat::Json => {
            let output = GovernanceConsistencyOutput {
                schema_version: "1.0".to_string(),
                status,
                conflict_count: conflicts.len(),
                conflicts,
            };
            CliRunResult::json(exit_code, &output)
        }
        GovernanceConsistencyFormat::Human => {
            let text = render_human(&conflicts);
            CliRunResult {
                exit_code,
                stdout: text,
                stderr: String::new(),
            }
        }
    }
}

/// Detect governance conflicts across specs.
///
/// This checks for:
/// 1. Within a single spec: `allow_imports_from` and `never_imports` overlap
/// 2. Within a single spec: `allow_imported_by` and `deny_imported_by` overlap
/// 3. Private visibility with non-empty `allow_imported_by` (contradictory intent)
/// 4. Cross-spec `imports_contract` conflicts (same contract referenced with
///    conflicting directions or envelope requirements)
pub(crate) fn detect_governance_conflicts(specs: &[SpecFile]) -> Vec<GovernanceConflict> {
    let mut conflicts = Vec::new();

    for spec in specs {
        let spec_path = spec.spec_path.as_ref().map(|p| p.display().to_string());

        if let Some(boundaries) = &spec.boundaries {
            // 1. allow_imports_from ∩ never_imports
            if let Some(allow_list) = &boundaries.allow_imports_from {
                for module in &boundaries.never_imports {
                    if allow_list.contains(module) {
                        conflicts.push(GovernanceConflict {
                            module: spec.module.clone(),
                            conflict_type: "allow_never_overlap".to_string(),
                            description: format!(
                                "Module '{}' appears in both allow_imports_from and never_imports for '{}'.",
                                module, spec.module
                            ),
                            spec_path: spec_path.clone(),
                        });
                    }
                }
            }

            // 2. allow_imported_by ∩ deny_imported_by
            for importer in &boundaries.deny_imported_by {
                if boundaries.allow_imported_by.contains(importer) {
                    conflicts.push(GovernanceConflict {
                        module: spec.module.clone(),
                        conflict_type: "allow_deny_imported_by_overlap".to_string(),
                        description: format!(
                            "Module '{}' appears in both allow_imported_by and deny_imported_by for '{}'.",
                            importer, spec.module
                        ),
                        spec_path: spec_path.clone(),
                    });
                }
            }

            // 3. Private visibility with non-empty allow_imported_by
            if boundaries.visibility == Some(crate::spec::Visibility::Private)
                && !boundaries.allow_imported_by.is_empty()
            {
                conflicts.push(GovernanceConflict {
                    module: spec.module.clone(),
                    conflict_type: "private_with_allow_imported_by".to_string(),
                    description: format!(
                        "Module '{}' has visibility 'private' but also declares allow_imported_by [{}]. Private modules cannot be imported by anyone.",
                        spec.module,
                        boundaries.allow_imported_by.join(", ")
                    ),
                    spec_path: spec_path.clone(),
                });
            }

            // 4. Cross-spec imports_contract conflicts:
            //    Collect contract references for cross-module consistency check later
        }
    }

    // Cross-spec: detect conflicting imports_contract on same namespace
    //
    // An imports_contract reference is "target_module:contract_id". If two different
    // provider specs publish the same contract_id with conflicting directions or
    // envelope requirements, that is a governance conflict.
    let mut contract_sources: BTreeMap<String, Vec<ContractSource>> = BTreeMap::new();

    for spec in specs {
        if let Some(boundaries) = &spec.boundaries {
            for contract in &boundaries.contracts {
                if contract.id.is_empty() {
                    continue;
                }
                // Group by contract ID only (not namespaced by module) to detect cross-module collisions.
                // Same-module duplicates are already caught by validation before this function runs.
                contract_sources
                    .entry(contract.id.clone())
                    .or_default()
                    .push(ContractSource {
                        provider_module: spec.module.clone(),
                        spec_path: spec.spec_path.as_ref().map(|p| p.display().to_string()),
                    });
            }
        }
    }

    // Detect cross-module duplicate contract IDs: two or more distinct modules publishing the same
    // contract ID string is a governance conflict. Same-module duplicates are handled by validation.
    for (contract_id, sources) in &contract_sources {
        // Collect the distinct modules that define this contract ID
        let mut distinct_modules: Vec<&ContractSource> = Vec::new();
        for source in sources {
            if !distinct_modules
                .iter()
                .any(|s| s.provider_module == source.provider_module)
            {
                distinct_modules.push(source);
            }
        }
        if distinct_modules.len() > 1 {
            let module_names: Vec<&str> = distinct_modules
                .iter()
                .map(|s| s.provider_module.as_str())
                .collect();
            conflicts.push(GovernanceConflict {
                module: distinct_modules[0].provider_module.clone(),
                conflict_type: "duplicate_contract_id".to_string(),
                description: format!(
                    "Contract ID '{}' is published by multiple modules: [{}]. Each contract ID must be unique across modules.",
                    contract_id,
                    module_names.join(", "),
                ),
                spec_path: distinct_modules[0].spec_path.clone(),
            });
        }
    }

    // Cross-spec: detect contradictory imports_contract references
    // If module A's contract imports_contract references "B:foo", and module C's
    // contract also imports_contract references "B:foo" but with different envelope
    // or direction expectations, that's a conflict.
    let mut imports_contract_refs: BTreeMap<String, Vec<ImportsContractRef>> = BTreeMap::new();

    for spec in specs {
        if let Some(boundaries) = &spec.boundaries {
            for contract in &boundaries.contracts {
                for import_ref in &contract.imports_contract {
                    imports_contract_refs
                        .entry(import_ref.clone())
                        .or_default()
                        .push(ImportsContractRef {
                            consumer_module: spec.module.clone(),
                            consumer_contract_id: contract.id.clone(),
                            direction: serialize_to_lowercase(&contract.direction),
                            envelope: serialize_to_lowercase(&contract.envelope),
                            spec_path: spec.spec_path.as_ref().map(|p| p.display().to_string()),
                        });
                }
            }
        }
    }

    for (target_ref, refs) in &imports_contract_refs {
        if refs.len() > 1 {
            // Proper pairwise comparison: check every i < j pair so no conflict is silently missed.
            // For example, given consumers [A, B, C] where A+B agree but C conflicts with B,
            // comparing only A-vs-rest would miss the B-vs-C conflict.
            for i in 0..refs.len() {
                for j in (i + 1)..refs.len() {
                    let a = &refs[i];
                    let b = &refs[j];
                    if a.direction != b.direction || a.envelope != b.envelope {
                        conflicts.push(GovernanceConflict {
                            module: target_ref.clone(),
                            conflict_type: "imports_contract_conflict".to_string(),
                            description: format!(
                                "Conflicting imports_contract references to '{}': consumer '{}' (contract '{}', direction={}, envelope={}) vs consumer '{}' (contract '{}', direction={}, envelope={}).",
                                target_ref,
                                a.consumer_module,
                                a.consumer_contract_id,
                                a.direction,
                                a.envelope,
                                b.consumer_module,
                                b.consumer_contract_id,
                                b.direction,
                                b.envelope,
                            ),
                            spec_path: a.spec_path.clone(),
                        });
                    }
                }
            }
        }
    }

    // Sort for deterministic output
    conflicts.sort();
    conflicts.dedup();
    conflicts
}

#[derive(Debug, Clone)]
struct ContractSource {
    provider_module: String,
    spec_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportsContractRef {
    consumer_module: String,
    consumer_contract_id: String,
    direction: String,
    envelope: String,
    spec_path: Option<String>,
}

fn serialize_to_lowercase<T: serde::Serialize + std::fmt::Debug>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{value:?}"))
}

fn render_human(conflicts: &[GovernanceConflict]) -> String {
    let mut out = String::new();

    out.push_str("Governance Consistency Report:\n");

    if conflicts.is_empty() {
        out.push_str("  No conflicts detected.\n");
        return out;
    }

    out.push_str(&format!("  Conflicts: {}\n\n", conflicts.len()));

    for (i, conflict) in conflicts.iter().enumerate() {
        out.push_str(&format!(
            "  {}. [{}] {}\n",
            i + 1,
            conflict.conflict_type,
            conflict.module
        ));
        out.push_str(&format!("     {}\n", conflict.description));
        if let Some(path) = &conflict.spec_path {
            out.push_str(&format!("     spec: {path}\n"));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use crate::spec::SpecFile;
    use crate::spec::types::{
        Boundaries, BoundaryContract, ContractDirection, ContractMatch, EnvelopeRequirement,
        Visibility,
    };

    use super::*;

    fn make_spec(module: &str, boundaries: Option<Boundaries>) -> SpecFile {
        SpecFile {
            version: "2.2".to_string(),
            module: module.to_string(),
            package: None,
            import_id: None,
            import_ids: vec![],
            description: None,
            boundaries,
            constraints: vec![],
            spec_path: None,
        }
    }

    #[test]
    fn no_conflicts_for_clean_specs() {
        let specs = vec![
            make_spec(
                "app",
                Some(Boundaries {
                    allow_imports_from: Some(vec!["core".to_string()]),
                    ..Default::default()
                }),
            ),
            make_spec(
                "core",
                Some(Boundaries {
                    visibility: Some(Visibility::Public),
                    ..Default::default()
                }),
            ),
        ];
        let conflicts = detect_governance_conflicts(&specs);
        assert!(conflicts.is_empty(), "expected no conflicts: {conflicts:?}");
    }

    #[test]
    fn detects_allow_never_overlap() {
        let specs = vec![make_spec(
            "app",
            Some(Boundaries {
                allow_imports_from: Some(vec!["core".to_string(), "utils".to_string()]),
                never_imports: vec!["core".to_string()],
                ..Default::default()
            }),
        )];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "allow_never_overlap");
        assert!(conflicts[0].description.contains("core"));
        assert!(conflicts[0].description.contains("allow_imports_from"));
        assert!(conflicts[0].description.contains("never_imports"));
    }

    #[test]
    fn detects_allow_deny_imported_by_overlap() {
        let specs = vec![make_spec(
            "core",
            Some(Boundaries {
                allow_imported_by: vec!["app".to_string(), "ui".to_string()],
                deny_imported_by: vec!["app".to_string()],
                ..Default::default()
            }),
        )];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "allow_deny_imported_by_overlap");
        assert!(conflicts[0].description.contains("app"));
    }

    #[test]
    fn detects_private_with_allow_imported_by() {
        let specs = vec![make_spec(
            "internal",
            Some(Boundaries {
                visibility: Some(Visibility::Private),
                allow_imported_by: vec!["friend".to_string()],
                ..Default::default()
            }),
        )];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "private_with_allow_imported_by");
        assert!(conflicts[0].description.contains("private"));
        assert!(conflicts[0].description.contains("allow_imported_by"));
    }

    #[test]
    fn detects_conflicting_imports_contract_references() {
        let specs = vec![
            make_spec(
                "consumer_a",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "my_contract".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "consumer_b",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "other_contract".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Optional,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "imports_contract_conflict");
        assert!(
            conflicts[0]
                .description
                .contains("provider:shared_contract")
        );
    }

    #[test]
    fn no_conflict_when_imports_contract_references_agree() {
        let specs = vec![
            make_spec(
                "consumer_a",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "my_contract".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "consumer_b",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "other_contract".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        assert!(
            conflicts.is_empty(),
            "expected no conflicts when references agree: {conflicts:?}"
        );
    }

    #[test]
    fn detects_duplicate_contract_id_across_modules() {
        // Two distinct modules publishing the same contract ID — cross-module collision.
        // Same-module duplicates are caught by validation (not this detector).
        let specs = vec![
            make_spec(
                "provider_x",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "dup_contract".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        r#match: ContractMatch::default(),
                        envelope: EnvelopeRequirement::Optional,
                        imports_contract: vec![],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "provider_y",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "dup_contract".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Outbound,
                        r#match: ContractMatch::default(),
                        envelope: EnvelopeRequirement::Required,
                        imports_contract: vec![],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "duplicate_contract_id");
        assert!(conflicts[0].description.contains("dup_contract"));
    }

    #[test]
    fn multiple_conflicts_are_sorted_deterministically() {
        let specs = vec![
            make_spec(
                "z_module",
                Some(Boundaries {
                    allow_imports_from: Some(vec!["banned".to_string()]),
                    never_imports: vec!["banned".to_string()],
                    ..Default::default()
                }),
            ),
            make_spec(
                "a_module",
                Some(Boundaries {
                    allow_imported_by: vec!["x".to_string()],
                    deny_imported_by: vec!["x".to_string()],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 2);
        // Conflicts should be sorted by module name
        assert_eq!(conflicts[0].module, "a_module");
        assert_eq!(conflicts[1].module, "z_module");
    }

    #[test]
    fn specs_without_boundaries_have_no_conflicts() {
        let specs = vec![make_spec("app", None), make_spec("core", None)];
        let conflicts = detect_governance_conflicts(&specs);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn human_output_format_is_readable() {
        let conflicts = vec![GovernanceConflict {
            module: "app".to_string(),
            conflict_type: "allow_never_overlap".to_string(),
            description:
                "Module 'core' appears in both allow_imports_from and never_imports for 'app'."
                    .to_string(),
            spec_path: Some("/path/to/app.spec.yml".to_string()),
        }];

        let output = render_human(&conflicts);
        assert!(output.contains("Governance Consistency Report:"));
        assert!(output.contains("Conflicts: 1"));
        assert!(output.contains("[allow_never_overlap]"));
        assert!(output.contains("spec: /path/to/app.spec.yml"));
    }

    #[test]
    fn human_output_no_conflicts() {
        let output = render_human(&[]);
        assert!(output.contains("No conflicts detected."));
    }

    // B1 tests: cross-module duplicate contract ID detection

    #[test]
    fn detects_cross_module_duplicate_contract_id() {
        // module_a and module_b both publish contract ID "shared_api" — governance conflict
        let specs = vec![
            make_spec(
                "module_a",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "shared_api".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec![],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "module_b",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "shared_api".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Optional,
                        r#match: ContractMatch::default(),
                        imports_contract: vec![],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "module_c",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "unique_api".to_string(),
                        contract: "contracts/c.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec![],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        let dup_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| c.conflict_type == "duplicate_contract_id")
            .collect();
        assert_eq!(
            dup_conflicts.len(),
            1,
            "expected 1 cross-module duplicate_contract_id conflict, got: {conflicts:?}"
        );
        assert!(
            dup_conflicts[0].description.contains("shared_api"),
            "description should mention the contract id"
        );
        assert!(
            dup_conflicts[0].description.contains("module_a")
                || dup_conflicts[0].description.contains("module_b"),
            "description should mention the conflicting modules"
        );
    }

    #[test]
    fn same_module_duplicate_contract_id_does_not_trigger_cross_module_detector() {
        // Same module with duplicate contract IDs — handled by validation, NOT by this detector.
        // After the fix, the key is grouped by contract.id only, but both entries are from the
        // SAME module, so the cross-module check (2+ distinct modules) must NOT fire.
        let specs = vec![make_spec(
            "provider",
            Some(Boundaries {
                contracts: vec![
                    BoundaryContract {
                        id: "dup_contract".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Optional,
                        r#match: ContractMatch::default(),
                        imports_contract: vec![],
                    },
                    BoundaryContract {
                        id: "dup_contract".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec![],
                    },
                ],
                ..Default::default()
            }),
        )];

        let conflicts = detect_governance_conflicts(&specs);
        let dup_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| c.conflict_type == "duplicate_contract_id")
            .collect();
        assert!(
            dup_conflicts.is_empty(),
            "same-module duplicates should NOT be flagged by cross-module detector: {conflicts:?}"
        );
    }

    // D5 test: direction and envelope in conflict descriptions use lowercase (serde format)
    #[test]
    fn conflict_description_uses_lowercase_direction_and_envelope() {
        let specs = vec![
            make_spec(
                "consumer_a",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "my_contract".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "consumer_b",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "other_contract".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Optional,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_contract".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        assert_eq!(conflicts.len(), 1);
        let desc = &conflicts[0].description;
        // Must use lowercase serde form, not PascalCase Debug form
        assert!(
            desc.contains("inbound"),
            "expected lowercase 'inbound' in description, got: {desc}"
        );
        assert!(
            desc.contains("outbound"),
            "expected lowercase 'outbound' in description, got: {desc}"
        );
        assert!(
            desc.contains("required"),
            "expected lowercase 'required' in description, got: {desc}"
        );
        assert!(
            desc.contains("optional"),
            "expected lowercase 'optional' in description, got: {desc}"
        );
        assert!(
            !desc.contains("Inbound"),
            "must not use PascalCase 'Inbound' in description, got: {desc}"
        );
        assert!(
            !desc.contains("Outbound"),
            "must not use PascalCase 'Outbound' in description, got: {desc}"
        );
    }

    // B2 test: pairwise comparison for imports_contract conflicts

    #[test]
    fn detects_all_pairwise_imports_contract_conflicts() {
        // Consumer A: Inbound + Required
        // Consumer B: Inbound + Required  (agrees with A)
        // Consumer C: Outbound + Optional (conflicts with both A and B)
        // Current buggy code: finds A-vs-C (1 conflict)
        // Fixed code: finds A-vs-C AND B-vs-C (at least 2 conflicts)
        let specs = vec![
            make_spec(
                "consumer_a",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "contract_a".to_string(),
                        contract: "contracts/a.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_api".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "consumer_b",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "contract_b".to_string(),
                        contract: "contracts/b.json".to_string(),
                        direction: ContractDirection::Inbound,
                        envelope: EnvelopeRequirement::Required,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_api".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
            make_spec(
                "consumer_c",
                Some(Boundaries {
                    contracts: vec![BoundaryContract {
                        id: "contract_c".to_string(),
                        contract: "contracts/c.json".to_string(),
                        direction: ContractDirection::Outbound,
                        envelope: EnvelopeRequirement::Optional,
                        r#match: ContractMatch::default(),
                        imports_contract: vec!["provider:shared_api".to_string()],
                    }],
                    ..Default::default()
                }),
            ),
        ];

        let conflicts = detect_governance_conflicts(&specs);
        let ic_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| c.conflict_type == "imports_contract_conflict")
            .collect();
        assert!(
            ic_conflicts.len() >= 2,
            "expected at least 2 pairwise conflicts (A-vs-C and B-vs-C), got {}: {conflicts:?}",
            ic_conflicts.len()
        );

        // Verify B-vs-C conflict is present
        let has_b_c_conflict = ic_conflicts.iter().any(|c| {
            (c.description.contains("consumer_b") && c.description.contains("consumer_c"))
                || (c.description.contains("consumer_c") && c.description.contains("consumer_b"))
        });
        assert!(
            has_b_c_conflict,
            "expected a B-vs-C conflict to be reported, got: {conflicts:?}"
        );

        // Also verify both consumers' spec paths are included (not just first)
        // For each pairwise conflict the description should mention both consumers
        for conflict in &ic_conflicts {
            let has_two_consumers = ["consumer_a", "consumer_b", "consumer_c"]
                .iter()
                .filter(|&&name| conflict.description.contains(name))
                .count()
                >= 2;
            assert!(
                has_two_consumers,
                "each conflict description should mention both consumers: {}",
                conflict.description
            );
        }
    }
}
