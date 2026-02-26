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
# Clone and build
git clone <specgate-repo>
cd specgate
cargo build --release

# Binary is at ./target/release/specgate
# Add to PATH or alias
alias specgate="./target/release/specgate"
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
version: "1.0"
project_root: "."
spec_pattern: "modules/*.spec.yml"
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

Quick CI snippet:

```yaml
- name: Specgate Check
  run: specgate check --output-mode deterministic
```

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
# Check only modules changed since last commit
specgate check --since HEAD~1

# Check only modules changed since branching
specgate check --since main
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
