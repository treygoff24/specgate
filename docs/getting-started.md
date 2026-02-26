# Getting Started

Run the baseline test suite:

```bash
cargo test
```

## Operator workflow: resolver parity diagnosis

When an import resolves differently between Specgate and TypeScript, use focused parity mode:

```bash
specgate doctor compare \
  --project-root . \
  --from src/app/main.ts \
  --import @core/utils \
  --tsc-trace trace.log
```

`--tsc-trace` accepts either:
- JSON edge payloads (legacy fixture format), or
- raw `tsc --traceResolution` text output.

### What to look for in output

`doctor compare` now reports:
- `specgate_resolution` (result kind + step trace)
- `tsc_trace_resolution` (trace-derived result)
- `parity_verdict` (`MATCH` or `DIFF`)
- `actionable_mismatch_hint` for next debugging steps

Example mismatch checklist from `actionable_mismatch_hint`:
- verify active `tsconfig` (`baseUrl`, `paths`)
- verify monorepo project references
- compare `moduleResolution` conditions / package `exports`
- check symlink behavior (`preserveSymlinks`)

## Current phase scope

Phase 1 ships foundational modules:
- `spec`
- `resolver`
- `parser`
- `deterministic`
