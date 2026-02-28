# `specgate` npm wrapper

Lightweight npm wrapper for Specgate distribution, including a focused TypeScript resolution snapshot generator that can be fed to `specgate doctor compare --tsc-trace`.

## What this package provides

- `specgate` wrapper binary
- `specgate-resolution-snapshot` focused trace generator

The wrapper is intentionally small and self-contained. It does not assume CI-specific environment variables or secret configuration.

## CLI usage

### Focused snapshot generation

```bash
npx specgate-resolution-snapshot \
  --project-root . \
  --from src/app/main.ts \
  --import @core/utils \
  --out .tmp/tsc-focus.json \
  --pretty
```

Equivalent command through the wrapper bin:

```bash
npx specgate resolution-snapshot \
  --from src/app/main.ts \
  --import @core/utils \
  --out .tmp/tsc-focus.json
```

### Feed snapshot into doctor compare

```bash
specgate doctor compare \
  --project-root . \
  --from src/app/main.ts \
  --import @core/utils \
  --structured-snapshot-in .tmp/tsc-focus.json \
  --parser-mode structured
```

## Snapshot JSON schema (`doctor_compare_tsc_resolution_focus`)

The generator emits a JSON object with this shape:

```json
{
  "schema_version": "1",
  "snapshot_kind": "doctor_compare_tsc_resolution_focus",
  "producer": "specgate-npm-wrapper",
  "generated_at": "2026-02-28T18:20:00.000Z",
  "project_root": "/abs/path/to/repo",
  "tsconfig_path": "tsconfig.json",
  "focus": {
    "from": "src/app/main.ts",
    "import_specifier": "@core/utils"
  },
  "resolutions": [
    {
      "source": "tsc_compiler_api",
      "from": "src/app/main.ts",
      "import": "@core/utils",
      "import_specifier": "@core/utils",
      "result_kind": "first_party",
      "resolved_to": "src/core/utils.ts",
      "trace": [
        "tsconfig: tsconfig.json",
        "module_resolution: NodeNext"
      ]
    }
  ],
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/utils.ts"
    }
  ]
}
```

`result_kind` is one of:

- `first_party`
- `third_party`
- `unresolvable`

For `third_party` results, `package_name` is included when it can be inferred.

## Rust parser compatibility contract

This shape is intentionally aligned to Specgate's `doctor compare` structured parser:

- Any object with `from` + `to` contributes an edge.
- Any object with `from` + one of `import` / `import_specifier` / `specifier` contributes a focused resolution record.
- `resolved_to` is consumed when present.
- `result_kind` is consumed when present and falls back to inference when omitted.
- `trace` is consumed as an array of strings.

Because `resolutions[]` and `edges[]` are standard JSON objects using those field names, the output can be passed directly to `--structured-snapshot-in`.

## Native `specgate` binary forwarding

The `specgate` wrapper forwards non-`resolution-snapshot` commands to a native binary if one is available at:

1. `SPECGATE_NATIVE_BIN` (env var)
2. `npm/specgate/native/<platform>/<arch>/specgate`
3. `npm/specgate/native/<platform>/specgate`

If no native binary is found, the wrapper exits with an explanatory error.
