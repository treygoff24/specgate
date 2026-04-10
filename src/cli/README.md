# cli/

This module owns command wiring, command-level contracts, and CLI-facing behavior.

Implemented commands:
- `check`
- `validate`
- `init`
- `doctor`
- `doctor compare`
- `baseline`

`check` output contract:
- `--output-mode deterministic|metrics`
- `--metrics` remains as a backwards-compatible alias for `--output-mode metrics`

`doctor compare` notes:
- mismatch is diagnostic (not policy) and returns dedicated exit code `3`
- supports focused compare for a single import via `--from <file> --import <specifier>`
- focused output includes `specgate_resolution`, `tsc_trace_resolution`, `parity_verdict` (`MATCH`/`DIFF`), and `actionable_mismatch_hint`
- `--tsc-trace` accepts structured JSON trace payloads
- `--tsc-command` executes via `sh -lc` and requires explicit `--allow-shell`

Exit code contract:
- `0` pass
- `1` policy violations
- `2` config/runtime errors
- `3` doctor diagnostic mismatch
