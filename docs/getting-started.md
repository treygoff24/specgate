# Getting Started with Specgate

**Your first 15 minutes with Specgate.**

This guide walks you through installing, configuring, and running your first architecture checks.

---

## Prerequisites

- Rust 1.85+ (MSRV)
- Git (for blast-radius mode)
- A TypeScript/JavaScript project to guard

---

## First 15 Minutes

### Minute 0-2: Build and Install

```bash
# Fast path (release asset + checksum)
SPECGATE_TAG=v0.1.0-rc2
SPECGATE_ARCH="x86_64-unknown-linux-gnu"
SPECGATE_ARCHIVE="specgate-${SPECGATE_TAG}-${SPECGATE_ARCH}.tar.gz"
SPECGATE_URL="https://github.com/treygoff24/specgate/releases/download/${SPECGATE_TAG}/${SPECGATE_ARCHIVE}"
INSTALL_BIN_DIR="$HOME/.local/bin"

mkdir -p "$INSTALL_BIN_DIR"
if \
  curl -fsSL "$SPECGATE_URL" -o "/tmp/${SPECGATE_ARCHIVE}" && \
  curl -fsSL "${SPECGATE_URL}.sha256" -o "/tmp/${SPECGATE_ARCHIVE}.sha256" && \
  (cd /tmp && sha256sum -c "${SPECGATE_ARCHIVE}.sha256"); then
  tar -xzf "/tmp/${SPECGATE_ARCHIVE}" -C /tmp
  mv /tmp/specgate "$INSTALL_BIN_DIR/specgate"
  chmod +x "$INSTALL_BIN_DIR/specgate"
  echo "✅ Installed prebuilt specgate ${SPECGATE_TAG}"
else
  # Fallback when release assets are unavailable
  cargo install --locked --git https://github.com/treygoff24/specgate --tag "$SPECGATE_TAG"
fi

export PATH="$INSTALL_BIN_DIR:$PATH"

specgate --help
```

### Minute 3-5: Initialize Your Project

```bash
cd your-project

# Initialize specgate
specgate init
```

This creates:
- `specgate.config.yml` — Project configuration
- `modules/` — Directory for your spec files

Generated `specgate.config.yml`:

```yaml
spec_dirs:
  - "modules"
exclude: []
test_patterns: []
```

### Minute 6-10: Create Your First Spec

Create `modules/core-api.spec.yml`:

```yaml
version: "2.2"
module: core/api
description: "Core API module"
boundaries:
  public_api:
    - src/api/index.ts
    - src/api/handlers/*.ts
  allow_imports_from:
    - core/domain
    - shared/utils
  never_imports:
    - infrastructure/db
```

Key concepts:
- **`version: "2.2"`** — Must be exactly `"2.2"` (strict matching)
- **`module`** — Unique identifier (e.g., `layer/name`)
- **`public_api`** — Files other modules can import (glob patterns)
- **`allow_imports_from`** — Modules this module can import
- **`never_imports`** — Modules this module must never import

### Minute 11-13: Run Your First Check

```bash
# Basic check
specgate check
```

Example output:

```
❌ FAILED: 1 violation(s)

VIOLATION: boundary.allow_imports_from
  From: core/api (src/api/handlers/user.ts:15)
  To:   infrastructure/db
  Import: import { Database } from '../../infra/db/connection'

SUMMARY:
  Total violations: 1
  Errors: 1
  Warnings: 0
```

### Minute 14-15: Fix and Re-check

Move the forbidden import to an allowed module:

```typescript
// Before (in core/api/handlers/user.ts)
import { Database } from '../../infra/db/connection'; // ❌

// After (in core/domain/user/repository.ts)
import { Database } from '../../infra/db/connection'; // ✅ (if domain allows infra)
```

Or create a facade:

```yaml
# In modules/core-domain.spec.yml
boundaries:
  allow_imports_from:
    - infrastructure/db  # Domain can use DB
```

Re-run:

```bash
specgate check
# ✅ PASSED: No violations
```

---

## Next Steps

### Run Tests

```bash
# All tests
cargo test

# Contract fixtures
cargo test contract_fixtures

# Tier A gate
cargo test tier_a_golden

# Golden corpus
cargo test golden_corpus
```

### Set Up CI

Start with [MVP Merge Gate](mvp-merge-gate.md), then use [CI Gate Understanding](CI-GATE-UNDERSTANDING.md) for full pipeline options.

For a ready-to-use consumer workflow, copy `docs/examples/specgate-consumer-github-actions.yml`
into your repository's `.github/workflows/specgate.yml`.

Run the gate locally before pushing:

```bash
./scripts/ci/mvp_gate.sh
```

Quick CI snippet:

```yaml
- name: Specgate Check
  run: specgate check --output-mode metrics | tee .specgate-verdict.json
```

The consumer workflow template uploads `.specgate-verdict.json` as the `specgate-verdict`
artifact and records a concise telemetry summary in `GITHUB_STEP_SUMMARY`.

### Diagnose Resolver Parity Mismatches

When an import resolves differently between Specgate and TypeScript, run focused parity mode:

```bash
specgate doctor compare \
  --project-root . \
  --from src/app/main.ts \
  --import @core/utils \
  --tsc-trace trace.log
```

`--tsc-trace` accepts JSON edge payloads or raw `tsc --traceResolution` text.
Look for:
- `parity_verdict` (`MATCH`/`DIFF`)
- `specgate_resolution` and `tsc_trace_resolution`
- `actionable_mismatch_hint` when parity is `DIFF`

### Explore More Specs

Create specs for each architectural layer:

```yaml
# modules/infrastructure-db.spec.yml
version: "2.2"
module: infrastructure/db
description: "Database infrastructure"
boundaries:
  public_api:
    - src/infra/db/index.ts
  # No allow_imports_from = can import from anywhere

# modules/core-domain.spec.yml
version: "2.2"
module: core/domain
description: "Domain logic"
boundaries:
  public_api:
    - src/domain/**/*.ts
  allow_imports_from:
    - shared/utils
  never_imports:
    - core/api  # Domain should not depend on API
```

### Use Blast-Radius Mode

Only check affected modules:

```bash
# Full run is safest when history/base refs are not available yet.
specgate check

# For PR/delta checks, use an explicit tracked ref.
# Requires that origin/main exists and includes project history.
git fetch origin main --depth=1
specgate check --since origin/main

# Avoid these in fresh/new repos unless refs exist:
# - --since HEAD~1 (only valid when current branch has a parent commit)
# - --since main (only valid when local branch `main` exists)
```

---

## Common Patterns

### Layer Architecture

```yaml
# Enforce layer ordering: api → domain → infrastructure
# modules/core-api.spec.yml
boundaries:
  allow_imports_from:
    - core/domain
    - shared/utils
  never_imports:
    - infrastructure/db

# modules/core-domain.spec.yml
boundaries:
  allow_imports_from:
    - infrastructure/db
    - shared/utils
```

### Module Encapsulation

```yaml
# Force all imports through public API
boundaries:
  public_api:
    - src/my-module/index.ts
    - src/my-module/public/**/*.ts
  enforce_canonical_imports: true
```

### Dependency Governance

```yaml
# Prevent sensitive dependencies
boundaries:
  forbidden_dependencies:
    - "node:fs"  # No direct filesystem access
    - "axios"    # Use our http client instead
```

---

## Troubleshooting

### "Version 2 not supported"

Change `version: "2"` to `version: "2.2"` in your spec files.

### "Module not found"

Ensure module IDs in `allow_imports_from` match exactly (case-sensitive).

### "Exit code 2"

Runtime error — check:
- Spec file syntax (run `specgate validate`)
- File paths exist
- Config file is valid YAML

---

## See Also

- [Operator Guide](OPERATOR_GUIDE.md) — Complete onboarding
- [Spec Language Reference](spec-language.md) — YAML format
- [CI Gate Understanding](CI-GATE-UNDERSTANDING.md) — CI patterns
- [Implementation Plan](specgate-implementation-plan-v1.1.md) — MVP status
- [Dogfood Rollout Checklist](DOGFOOD_ROLLOUT_CHECKLIST.md) — Dogfood rollout steps
- [Baseline Policy](BASELINE_POLICY.md) — Baseline and stale-entry policy
- [Releasing Guide](../RELEASING.md) — Dogfood and release workflow
