# Specgate Golden Corpus v1 - Top 5

This directory contains minimal, deterministic fixtures representing real bugs found in production codebases. Each fixture demonstrates a pattern that Specgate should catch statically.

## Cases

| ID | Name | Category | Status | Intro SHA | Fix SHA | Repo |
|----|------|----------|--------|-----------|---------|------|
| C02 | Mass-Assignment | Security | ⚠️ Future | `9f7f3ba2...` | `c7fd3803...` | treys-command-logic |
| C06 | Duplicate Key Shadowing | Reliability | ⚠️ Future | `8e78a621...` | `93baa0e0...` | openclaw-hud |
| C07 | Registry Collision | Contract | ⚠️ Future | `6198e8a8...` | `5784d9e6...` | treys-command-logic |
| C08 | Layer Inversion | Architecture | ⚠️ Proxy | `b19749d6...` | `567abbd7...` | hearth |
| C09 | API Leakage | Boundary | ⚠️ Proxy | `89d79b1c...` | `cbd16429...` | hearth |

## Status Legend
- ⚠️ **Future Enhancement**: Requires rule not yet implemented; fixture demonstrates intended behavior
- ⚠️ **Semantic Proxy**: Requires semantic analysis beyond current capabilities; serves as proxy for future enhancement

## Proposed Rules (Future)

| Case | Proposed Rule | Description |
|------|---------------|-------------|
| C02 | `no-pattern` | Pattern match on spread in mutation calls |
| C06 | `no-pattern` | Detect duplicate keys in object literals |
| C07 | `boundary.unique_export` | Enforce global uniqueness of exported IDs |
| C08 | Layer-aware guard analysis | Detect shared validation across protocol layers |
| C09 | Type leakage analysis | Detect infrastructure types in public API returns |

## Structure

```
cXX-name/
├── README.md              # Provenance, root cause, rule mapping
├── specgate.config.yml    # Project config
├── modules/               # Spec files
│   └── *.spec.yml
├── src/                   # Source files
│   ├── *-intro.ts/js      # Buggy version
│   └── *-fix.ts/js        # Fixed version
└── expected/              # Expected verdicts
    ├── intro.verdict.json
    └── fix.verdict.json
```

## Running Tests

```bash
cargo test -q golden_corpus
```

## Sources

Cases mined from:
- `/Users/treygoff/Development/openclaw-hud`
- `/Users/treygoff/Development/hearth`
- `/Users/treygoff/Development/treys-command-logic`

Full analysis: `/Users/treygoff/.openclaw/workspace/output/specgate-bughunt/*.md`
