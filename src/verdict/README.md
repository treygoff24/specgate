# verdict/

This module assembles `check` results into the final verdict formats.

Responsibilities:
- Build deterministic JSON output for `check` results
- Aggregate status from rule violations
- Expose optional metrics mode (timing metadata)
- Keep default output deterministic (no timestamps/durations unless metrics is enabled)
