# C06: Duplicate Object Key Shadowing (HTTPS Downgrade)

## Provenance
- **Intro SHA:** `8e78a621610714fd8f3b41c291550d66bd27da48`
- **Fix SHA:** `93baa0e030c9cb7586a63a46b441a1f5098163c2`
- **Repo:** `/Users/treygoff/Development/openclaw-hud`

## Root Cause
Two `wsUrl` keys in object literal caused silent overwrite due to JavaScript's last-write-wins semantics.

## Status
⚠️ **FUTURE ENHANCEMENT** - Requires `no-pattern` rule not yet implemented.
