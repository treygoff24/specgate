use clap::Args;

use super::*;

#[derive(Debug, Clone, Args)]
pub(crate) struct BaselineArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Output baseline path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    output: PathBuf,
    /// Rebuild baseline from current violations (prunes stale entries, re-sorts, dedupes, and resets generated_from metadata to current tool version and git SHA).
    #[arg(long)]
    refresh: bool,
}

pub(crate) fn handle_baseline(args: BaselineArgs) -> CliRunResult {
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

    let artifacts = match analyze_project(&loaded, None) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return runtime_error_json("runtime", "failed to analyze project", vec![error]);
        }
    };

    if !artifacts.layer_config_issues.is_empty() {
        return runtime_error_json(
            "config",
            "invalid enforce-layer rule configuration",
            artifacts.layer_config_issues,
        );
    }

    let governance = match compute_governance_hashes(&loaded) {
        Ok(governance) => governance,
        Err(error) => {
            return runtime_error_json(
                "governance",
                "failed to compute deterministic governance hashes",
                vec![error],
            );
        }
    };

    let baseline_path = resolve_against_root(&loaded.project_root, &args.output);
    let generated_from = BaselineGeneratedFrom {
        tool_version: build_info::tool_version().to_string(),
        git_sha: build_info::git_sha().to_string(),
        config_hash: governance.config_hash,
        spec_hash: governance.spec_hash,
    };

    let (baseline, stale_entries_pruned) = if args.refresh {
        let existing = match load_optional_baseline(&baseline_path) {
            Ok(existing) => existing,
            Err(error) => {
                return runtime_error_json(
                    "baseline",
                    "failed to load baseline file for refresh",
                    vec![error.to_string()],
                );
            }
        };

        if let Some(existing) = existing.as_ref() {
            let refreshed = refresh_baseline_with_metadata(
                &loaded.project_root,
                &artifacts.policy_violations,
                Some(existing),
                generated_from.clone(),
            );
            (refreshed.baseline, refreshed.stale_entries_pruned)
        } else {
            (
                build_baseline_with_metadata(
                    &loaded.project_root,
                    &artifacts.policy_violations,
                    generated_from.clone(),
                ),
                0usize,
            )
        }
    } else {
        (
            build_baseline_with_metadata(
                &loaded.project_root,
                &artifacts.policy_violations,
                generated_from,
            ),
            0usize,
        )
    };

    if let Err(error) = write_baseline(&baseline_path, &baseline) {
        return runtime_error_json(
            "baseline",
            "failed to write baseline file",
            vec![error.to_string()],
        );
    }

    let output = BaselineOutput {
        schema_version: "2.2".to_string(),
        status: "ok".to_string(),
        baseline_path: normalize_repo_relative(&loaded.project_root, &baseline_path),
        entry_count: baseline.entries.len(),
        source_violation_count: artifacts.policy_violations.len(),
        refreshed: args.refresh,
        stale_entries_pruned,
    };

    CliRunResult::json(EXIT_CODE_PASS, &output)
}
