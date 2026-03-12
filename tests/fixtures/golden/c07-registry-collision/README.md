# C07: Registry Collision (Duplicate Tool Definitions)

## Provenance
- **Intro SHA:** `6198e8a8ab1b973d0f2873c738599e84254995d7`
- **Fix SHA:** `5784d9e6e312b1d07b21cd0694d7a5dcb278f3f6`
- **Repo:** `/Users/treygoff/Development/treys-command-logic`

## Root Cause
New attachment tools added while legacy placeholder tools with same names remained, causing startup crash.

## Status
The `boundary.unique_export` rule is now implemented and enforces export name uniqueness within a module boundary. This fixture demonstrates a runtime data collision (duplicate tool name values in arrays), which is not detectable by static export-name analysis. The export names (`attachmentTools`, `noteTools`, `TOOL_DEFINITIONS`) are unique; the collision is in array element values at runtime.
