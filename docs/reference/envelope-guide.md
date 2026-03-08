# Envelope Enforcement Guide

This guide explains how Specgate enforces runtime validation site coverage for boundary contracts.

## What it does

When a contract declares `envelope: required`, Specgate performs a targeted AST check on the files matched by `match.files`:

1. The file imports the envelope package (as configured by `envelope.import_patterns`).
2. The file calls the validator with the exact contract ID as the first argument.

Concretely, it looks for calls equivalent to `boundary.validate('create_user', ...)` for contract `create_user`.

If the check cannot find both parts in the matched scope, it emits `boundary.envelope_missing` as a **warning**.

## Why a wrapper package (instead of direct `zod.parse()`)

Specgate’s AST check needs deterministic static evidence that the contract you declared is the same contract the code is validating.

Without a wrapper call that includes a contract ID literal, Specgate can’t confidently link code to a specific contract. For example, `zod.parse(data)` may validate data but does not declare **which** contract is being enforced.

A wrapper call gives a mechanical anchor:

- first argument must be a literal string contract ID (e.g. `"create_user"`)
- that exact string is compared to the contract `id`

The wrapper is configurable so teams can keep their own naming and packaging:

```yaml
# specgate.config.yml
envelope:
  enabled: true                    # master switch
  import_patterns:                 # packages to look for
    - "specgate-envelope"
    - "@myorg/validation"
  function_pattern: "boundary.validate"  # call pattern to match
```

## Supported patterns

Specgate’s envelope check is intentionally practical and supports the following invocation shapes:

- ESM import: `import { boundary } from 'specgate-envelope'`
- Destructured import: `import { validate } from 'specgate-envelope'`
- Renamed import: `import { boundary as b } from 'specgate-envelope'`
- CJS require: `const { boundary } = require('specgate-envelope')`
- Template literals: ``boundary.validate(`create_user`, data)``
- `as const`: `boundary.validate('create_user' as const, data)`
- Optional chaining: `boundary?.validate('create_user', data)`

## Export patterns and `match.pattern` scoping

If `match.pattern` is specified, Specgate looks for the named export in the matched file and scopes envelope-call matching to that export. A valid `boundary.validate(...)` call elsewhere in the same file does **not** satisfy the contract.

Supported export forms include:

- `export function name() { ... }`
- `export async function name() { ... }`
- `export const name = () => { ... }`
- `export const name = function() { ... }`
- `export default function name() { ... }`
- `export const name = withWrapper(() => { ... })` (HOC/wrapper patterns)
- `export { name }` (indirect exports with prior declaration)
- Class methods within exported classes

If your codebase uses export patterns not listed above, omit `match.pattern` to fall back to file-level checking, or restructure the export to use a supported form.

## Known limitations (by design)

- Presence-based, not control-flow: a call inside `if (...) { ... }` still satisfies coverage.
- No cross-file resolution: if validation happens in a helper, point `match.files` at the helper.
- Re-exported functions from other modules are not followed cross-file.
- Class dotted patterns (for example `UserService.createUser`) are not supported — use just the method name (`createUser`).
- Computed member expressions are not detected: `boundary['validate'](...)` does not match.
- Dynamic imports are not detected: `const { boundary } = await import('...')`.
- Type-only imports don't count (they are erased at runtime).

## Disabling envelope checks

Set this in `specgate.config.yml`:

```yaml
envelope:
  enabled: false
```

That disables all envelope checks project-wide.
