# Policy diff

`specgate policy-diff` compares `.spec.yml` policy snapshots between two git refs and classifies each detected change as `widening`, `narrowing`, or `structural`.

This command is for policy governance across git history. It looks only at `.spec.yml` files. It does not diff `specgate.config.yml`.

## Config governance scope decision

`specgate.config.yml` diffing is explicitly deferred-by-decision for this release.

- `policy-diff` remains scoped to `.spec.yml` governance changes only.
- Config changes still require normal code review and release controls.
- This keeps widening classification deterministic while config-level governance semantics are designed separately.

## Usage

```bash
specgate policy-diff --base <base-ref> [--head <head-ref>] [--project-root <path>] [--format human|json|ndjson]
```

`--base` is required. `--head` defaults to `HEAD`. `--format` defaults to `human`.

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
- `errors` carries structured failure details.
- NDJSON output emits structured error events (see below).

This lets CI and tooling treat any non-zero summary counters as a trustworthy gate signal only when exit code is `0` or `1`.

## Output formats

| Format | Behavior |
|--------|----------|
| `human` | Grouped text output with `WIDENING`, `NARROWING`, and `STRUCTURAL` sections, followed by a summary. Errors and limitations are appended when present. |
| `json` | One deterministic `PolicyDiffReport` object with `schema_version`, `base_ref`, `head_ref`, `diffs`, `summary`, and `errors`. |
| `ndjson` | One JSON object per event with `type: "error"` for each structured error, then `type: "change"` entries, then a final `type: "summary"` record. |

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

Some changes remain intentionally conservative in the MVP. Constraint additions and removals are currently reported as `structural` unless a rule specific severity change is recognized. When `boundaries.path` coverage cannot be bounded safely, the command reports the change as `structural` and adds the `path_coverage_unbounded_mvp` limitation in the summary.

Rename/copy semantic pairing uses normalized `SpecFile` snapshots (trimmed scalar strings, set-like list normalization, and canonicalized constraint/contract structures). If normalization cannot produce a trustworthy semantic comparison, classification stays fail-closed as widening-risk.

## Deferred follow up

| Item | Current behavior |
|------|------------------|
| Semantic rename pairing | Implemented for `R*`/`C*` when both sides can be normalized and compared; inconclusive pairings remain fail-closed widenings. |
| Cross file compensation | Not implemented. A widening in one file is not offset by a narrowing in another file. |
| Config level governance | Deferred by decision for this release. `specgate.config.yml` diffing remains out of scope for `policy-diff`. |
| Future gate integration | Implemented for `check` via `--deny-widenings` (requires `--since <base-ref>`). |

## Related docs

See [CI gate understanding](../design/ci-gate-understanding.md) for broader CI patterns and [Spec language reference](spec-language.md) for `.spec.yml` field definitions.
