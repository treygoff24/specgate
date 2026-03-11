# Policy diff

`specgate policy-diff` compares policy snapshots between two git refs and classifies each detected change as `widening`, `narrowing`, or `structural`.

This command is for policy governance across git history. It diffs both module specs (`.spec.yml`) and the repo-root `specgate.config.yml`, then produces a deterministic report and exit code for CI gating.

## Usage

```bash
specgate policy-diff --base <base-ref> [--head <head-ref>] [--project-root <path>] [--format human|json|ndjson] [--cross-file-compensation]
```

`--base` is required. `--head` defaults to `HEAD`. `--format` defaults to `human`. `--cross-file-compensation` is opt-in and adds scoped offset analysis between directly connected modules.

## Examples

Examples below use `origin/main` as shorthand for the consumer repo's default
branch ref. Substitute your actual default branch ref when it differs (for
example `origin/master`).

Compare the current branch against `origin/main`.

```bash
specgate policy-diff --base origin/main
```

Compare two explicit commits and emit JSON for CI or post processing.

```bash
specgate policy-diff --base 2f4c1ad --head 9b13d0e --format json
```

Emit NDJSON when you want one JSON object per change plus a final summary record.

```bash
specgate policy-diff --base origin/main --format ndjson
```

Run against a different repository root.

```bash
specgate policy-diff --project-root ../repo --base origin/main
```

Include cross-file compensation analysis when you want to see whether a narrowing in one directly connected module offsets a widening in another.

```bash
specgate policy-diff --base origin/main --cross-file-compensation --format json
```

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | No widening changes were detected. Narrowing only, structural only, or no changes. |
| `1` | One or more widening changes were detected. |
| `2` | Runtime failure. Examples include missing git refs, shallow clone history gaps, or `.spec.yml` parse errors. |

### Exit-2 payload contract

When the command exits `2`, it reports runtime/parse failures in the `errors` list, and suppresses authoritative policy classification output. In this mode:

- `diffs` is empty.
- `summary` counters are zeroed (`modules_changed`, `widening_changes`, `narrowing_changes`, `structural_changes`) and `has_widening` is `false`.
- `net_classification` falls back to `structural`.
- `errors` carries structured failure details.
- NDJSON output emits structured error events (see below).

This lets CI and tooling treat any non-zero summary counters as a trustworthy gate signal only when exit code is `0` or `1`.

## Output formats

| Format | Behavior |
|--------|----------|
| `human` | Grouped text output with `WIDENING`, `NARROWING`, and `STRUCTURAL` sections, optional `Config changes` and `Compensations` sections, then a summary. Errors and limitations are appended when present. |
| `json` | One deterministic `PolicyDiffReport` object with `schema_version`, `base_ref`, `head_ref`, `diffs`, `summary`, `errors`, `net_classification`, plus `config_changes` and optional `compensations`. |
| `ndjson` | One JSON object per event: `type: "error"` records first, then `type: "change"`, `type: "config_change"`, optional `type: "compensation"`, and a final `type: "summary"` record. |

Example human output:

```text
Policy diff: base=origin/main head=HEAD

WIDENING (1)
  - module=orders field=boundaries.allow_imports_from detail=allow_imports_from restricted -> unrestricted

NARROWING (0)

STRUCTURAL (1)
  - module=payments field=spec_file detail=new policy file

Summary: modules_changed=2 widening=1 narrowing=0 structural=1
```

## File operation semantics in the MVP

The command first looks at git file status for changed `.spec.yml` files, then does field level classification for in place edits.

| Git status | MVP result | Notes |
|-----------|------------|-------|
| `A` | `structural` | A new policy file is reported as a new governed module, not as a widening. |
| `M` / `T` | field level classification | Parsed `.spec.yml` content is compared field by field. |
| `D` | `widening` | Deleting a policy file removes governance for that module, so this is fail closed. |
| `R*` / `C*` | semantic pairing | Rename/copy is `structural` when old/new `.spec.yml` snapshots are semantically equivalent after normalization; otherwise it stays fail-closed `widening`. |

Two consequences are intentional in the current implementation. First, deleting a `.spec.yml` file is always reported as a widening. Second, rename/copy remains fail-closed when semantic pairing cannot prove equivalence (for example parse failures or ambiguous pairings). A run exits `1` whenever any widening remains.

## CI guidance

`policy-diff` needs both refs to exist locally. In GitHub Actions, use a full fetch when comparing against `origin/main` or any other remote ref.

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0

- name: Detect policy widenings
  run: specgate policy-diff --base origin/main
```

If the base ref is missing in a shallow clone, the command exits `2` with `git.shallow_clone_missing_ref` guidance. The message tells you to use `fetch-depth: 0` or to deepen history manually, for example:

```bash
git fetch --deepen=200 origin <base-ref>
specgate policy-diff --base <base-ref>
```

## `check --deny-widenings` integration

`specgate check` can enforce the same widening gate directly:

```bash
specgate check --since origin/main --deny-widenings
```

Behavior with the flag enabled:

- It reuses the same policy-diff classification pipeline that powers `specgate policy-diff`.
- If widening changes are detected, `check` exits `1` and includes widening details in output metadata.
- If governance evaluation fails (for example missing git refs or parse/runtime issues), `check` exits `2`.
- If no widening is detected, `check` follows normal `check` pass/fail behavior.

## What the command classifies today

For modified `.spec.yml` files, `policy-diff` classifies changes over parsed policy fields rather than raw text. That includes boundaries, constraints, and contracts. Examples include import allowlists, import denylists, visibility, contract envelope requirements, and other policy fields.

For `specgate.config.yml`, `policy-diff` emits `config_changes` records for governance-relevant config fields and folds them into the same summary counters and exit semantics. A config widening still makes the run fail with exit `1`. Shipped examples include `strict_ownership`, `strict_ownership_level`, `unresolved_edge_policy`, `baseline.require_metadata`, and `import_hygiene.deny_deep_import_entries`.

Some changes remain intentionally conservative in the MVP. Constraint additions and removals are currently reported as `structural` unless a rule specific severity change is recognized. When `boundaries.path` coverage cannot be bounded safely, the command reports the change as `structural` and adds the `path_coverage_unbounded_mvp` limitation in the summary.

Rename/copy semantic pairing uses normalized `SpecFile` snapshots (trimmed scalar strings, set-like list normalization, and canonicalized constraint/contract structures). If normalization cannot produce a trustworthy semantic comparison, classification stays fail-closed as widening-risk.

`net_classification` is the authoritative top-level verdict for machine consumers:

- `widening` if any uncompensated spec widening or any config widening remains.
- `narrowing` if no widening remains and at least one narrowing is present.
- `structural` if the report is otherwise clean or if exit `2` suppressed authoritative classification output.

## Shipped follow-up items

| Item | Current behavior |
|------|------------------|
| Semantic rename pairing | Implemented for `R*`/`C*` when both sides can be normalized and compared; inconclusive pairings remain fail-closed widenings. |
| Cross file compensation | Implemented as an opt-in analysis behind `--cross-file-compensation`. It is scoped to directly connected modules and reports candidate pairings in `compensations`; ambiguous matches fail closed. |
| Config level governance | Implemented for `specgate.config.yml`; config changes are reported in `config_changes` and folded into summary/net classification. |
| Future gate integration | Implemented for `check` via `--deny-widenings` (requires `--since <base-ref>`). |

Ownership note:

- `strict_ownership_level` participates in config governance diffing today.
- In `doctor ownership`, `strict_ownership_level: errors` gates duplicate module ids and invalid ownership globs, while `strict_ownership_level: warnings` gates all ownership findings.

## Related docs

See [CI gate understanding](../design/ci-gate-understanding.md) for broader CI patterns and [Spec language reference](spec-language.md) for `.spec.yml` field definitions.
