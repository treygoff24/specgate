# C07: Registry Collision (Duplicate Tool Definitions)

## Provenance
- **Intro SHA:** `6198e8a8ab1b973d0f2873c738599e84254995d7`
- **Fix SHA:** `5784d9e6e312b1d07b21cd0694d7a5dcb278f3f6`
- **Repo:** `/Users/treygoff/Development/treys-command-logic`

## Root Cause
New attachment tools added while legacy placeholder tools with same names remained, causing startup crash.

## Status
⚠️ **FUTURE ENHANCEMENT** - Requires `boundary.unique_export` rule not yet implemented.
