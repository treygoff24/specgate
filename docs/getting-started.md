# Getting Started

```bash
cargo test
```

Phase 1 ships foundational modules only:
- `spec`
- `resolver`
- `parser`
- `deterministic`

## MVP Merge Gate

Run the MVP merge gate locally:

```bash
./scripts/ci/mvp_gate.sh
```

See `docs/mvp-merge-gate.md` for the exact command sequence, pass criteria,
and failure categorization (`runtime/setup`, `contract drift`, `policy`).
