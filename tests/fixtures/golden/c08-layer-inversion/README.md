# C08: Layer Inversion (WS Origin Policy Applied to HTTP)

## Provenance
- **Intro SHA:** `b19749d6a486032e7198f17b9a823d7d90375c2c`
- **Fix SHA:** `567abbd7ed435367e5ab28f881d7f975e5d55dd7`
- **Repo:** `/Users/treygoff/Development/hearth`

## Root Cause
HTTP handler rejected requests when `Origin` header was absent, reusing strict WS-origin semantics.

## Status
⚠️ **SEMANTIC PROXY** - The `enforce-layer` rule exists but requires semantic analysis to detect shared validation logic.

Note: Module renamed to "http" to match layer configuration.
