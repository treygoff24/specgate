# Specgate Implementation Plan

**Version:** 1.0 — February 25, 2026
**Purpose:** Definitive build guide for coding agents. Every section is precise enough to implement with zero prior context.
**Language:** Rust
**MVP Scope:** Structural policy engine — module boundaries, dependency control, layer enforcement, cycle detection.

---

## 1. Project Setup

### Repository

```
~/Development/specgate/
```

Initialize with `cargo init --name specgate`. Single crate for MVP, but module layout supports future workspace splitting.

### Rust Configuration

- **Edition:** 2024
- **MSRV:** 1.85.0 (first stable edition 2024 release)
- **Profile:** `[profile.release]` with `lto = true`, `codegen-units = 1`, `strip = true`

### Directory Structure (EXACT paths)

```
specgate/
├── Cargo.toml
├── rust-toolchain.toml        # pin toolchain
├── src/
│   ├── main.rs                # CLI entrypoint (thin — delegates to lib)
│   ├── lib.rs                 # Library root, re-exports all modules
│   ├── cli/
│   │   ├── mod.rs             # Cli enum (clap derive)
│   │   ├── check.rs           # `specgate check` command
│   │   ├── init.rs            # `specgate init` command
│   │   ├── validate.rs        # `specgate validate` command
│   │   └── doctor.rs          # `specgate doctor` command
│   ├── spec/
│   │   ├── mod.rs             # Spec discovery + loading
│   │   ├── types.rs           # SpecFile, Boundaries, Constraint, Severity, SpecConfig
│   │   ├── validation.rs      # Schema validation, cross-spec conflict detection
│   │   └── config.rs          # specgate.config.yml parsing (SpecConfig)
│   ├── resolver/
│   │   ├── mod.rs             # ModuleResolver, ResolvedImport
│   │   └── classify.rs        # FirstParty/ThirdParty/Unresolvable classification
│   ├── parser/
│   │   ├── mod.rs             # FileAnalysis, ImportInfo, ExportInfo, parse_file()
│   │   └── ignore.rs          # @specgate-ignore comment parsing
│   ├── graph/
│   │   ├── mod.rs             # DependencyGraph, build(), query methods
│   │   └── discovery.rs       # File discovery (glob, exclude patterns)
│   ├── rules/
│   │   ├── mod.rs             # Rule trait, RuleContext, Violation, rule registry
│   │   ├── boundary.rs        # BoundaryCheck rule
│   │   ├── dependencies.rs    # DependencyBoundary rule
│   │   ├── circular.rs        # NoCircularDeps rule
│   │   └── layers.rs          # EnforceLayer rule
│   └── verdict/
│       ├── mod.rs             # Verdict struct, VerdictBuilder
│       └── format.rs          # JSON serialization, human summary
├── tests/
│   ├── integration.rs         # Integration test runner
│   └── fixtures/              # Test fixture mini-projects (see §10)
│       ├── basic/
│       ├── boundary-violation/
│       ├── public-api-bypass/
│       ├── circular-deps/
│       ├── forbidden-dep/
│       ├── layer-violation/
│       ├── type-only-allowed/
│       ├── escape-hatch/
│       ├── expired-ignore/
│       ├── monorepo/
│       ├── barrel-reexports/
│       └── tsconfig-paths/
└── docs/
    ├── spec-language.md
    └── getting-started.md
```

### rust-toolchain.toml

```toml
[toolchain]
channel = "stable"
```

---

## 2. Dependencies (Cargo.toml)

```toml
[package]
name = "specgate"
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
description = "Machine-checkable architectural intent for TypeScript projects"
license = "MIT"

[dependencies]
# AST parsing + module resolution (use latest 0.x — pin minor)
oxc_parser = "0"
oxc_ast = "0"
oxc_resolver = "4"
oxc_span = "0"
oxc_allocator = "0"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
serde_yml = "0.0.12"

# CLI
clap = { version = "4", features = ["derive"] }

# Graph algorithms
petgraph = "0.7"

# Glob matching
globset = "0.4"

# Error handling + diagnostics
miette = { version = "7", features = ["fancy"] }
thiserror = "2"

# JSON Schema generation
schemars = "0.8"

# Date parsing (for ignore expiry)
chrono = { version = "0.4", default-features = false, features = ["std"] }

# File walking
walkdir = "2"

# Parallelism (optional, feature-gated)
rayon = { version = "1", optional = true }

[features]
default = []
parallel = ["rayon"]

[dev-dependencies]
insta = { version = "1", features = ["yaml"] }
tempfile = "3"
pretty_assertions = "1"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

### Dependency Notes

- **`serde_yml`** — NOT `serde_yaml` (deprecated). Use `serde_yml` crate.
- **`serde_json` with `preserve_order`** — keeps key insertion order for deterministic output.
- **`BTreeMap` everywhere** — never use `HashMap`. All maps must be `BTreeMap` for deterministic iteration order. This is a project-wide rule.
- **oxc crates** — pin to the latest `0.x` at time of build. These move fast; use `cargo update` but don't auto-bump major.
- **`oxc_resolver`** — currently at v4.x, separate versioning from other oxc crates.

---

## 3. Core Types (src/spec/)

### src/spec/types.rs

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A single .spec.yml file — one per module.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SpecFile {
    /// Schema version. Must be "2".
    pub version: String,
    /// Module identifier, e.g. "api/orders", "ui/checkout".
    pub module: String,
    /// Human-readable description (not verified).
    #[serde(default)]
    pub description: Option<String>,
    /// Module boundary rules.
    #[serde(default)]
    pub boundaries: Option<Boundaries>,
    /// Architectural constraint rules.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Path to this spec file on disk (populated after loading, not deserialized).
    #[serde(skip)]
    pub spec_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Boundaries {
    /// Glob pattern for files belonging to this module. Default: inferred from module name.
    #[serde(default)]
    pub path: Option<String>,
    /// Public API entrypoint files. External modules must import through these only.
    #[serde(default)]
    pub public_api: Vec<String>,
    /// Allowed first-party import sources (DEFAULT-DENY when non-empty).
    #[serde(default)]
    pub allow_imports_from: Vec<String>,
    /// Forbidden import sources (hard deny, overrides everything).
    #[serde(default)]
    pub never_imports: Vec<String>,
    /// Modules allowed for `import type` even when runtime imports are forbidden.
    #[serde(default)]
    pub allow_type_imports_from: Vec<String>,
    /// Permitted third-party npm packages (DEFAULT-DENY when non-empty).
    #[serde(default)]
    pub allowed_dependencies: Vec<String>,
    /// Banned third-party npm packages (hard deny).
    #[serde(default)]
    pub forbidden_dependencies: Vec<String>,
    /// If true, boundary rules also apply to test files.
    #[serde(default)]
    pub enforce_in_tests: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Constraint {
    /// Rule identifier: "no-circular-deps", "enforce-layer".
    pub rule: String,
    /// Rule-specific parameters (interpreted per rule).
    #[serde(default = "default_params")]
    pub params: serde_json::Value,
    /// Severity level.
    #[serde(default)]
    pub severity: Severity,
    /// Human-readable explanation shown in violations.
    #[serde(default)]
    pub message: Option<String>,
}

fn default_params() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}
```

### src/spec/config.rs

```rust
/// Project-level configuration: specgate.config.yml
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpecConfig {
    /// Directories to search for .spec.yml files. Default: project root.
    #[serde(default = "default_spec_dirs")]
    pub spec_dirs: Vec<String>,
    /// Glob patterns for files to exclude from analysis.
    #[serde(default = "default_excludes")]
    pub exclude: Vec<String>,
    /// Glob patterns matching test files (excluded from boundary checks by default).
    #[serde(default = "default_test_patterns")]
    pub test_patterns: Vec<String>,
    /// Escape hatch governance.
    #[serde(default)]
    pub escape_hatches: EscapeHatchConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EscapeHatchConfig {
    /// Max new @specgate-ignore comments allowed in a single diff. None = unlimited.
    #[serde(default)]
    pub max_new_per_diff: Option<usize>,
    /// Require expiry dates on all ignore comments.
    #[serde(default)]
    pub require_expiry: bool,
}

fn default_spec_dirs() -> Vec<String> {
    vec![".".to_string()]
}

fn default_excludes() -> Vec<String> {
    vec![
        "**/node_modules/**".to_string(),
        "**/dist/**".to_string(),
        "**/build/**".to_string(),
        "**/.git/**".to_string(),
        "**/generated/**".to_string(),
    ]
}

fn default_test_patterns() -> Vec<String> {
    vec![
        "**/*.test.ts".to_string(),
        "**/*.test.tsx".to_string(),
        "**/*.spec.ts".to_string(),
        "**/*.spec.tsx".to_string(),
        "**/__tests__/**".to_string(),
        "**/__mocks__/**".to_string(),
    ]
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            spec_dirs: default_spec_dirs(),
            exclude: default_excludes(),
            test_patterns: default_test_patterns(),
            escape_hatches: EscapeHatchConfig::default(),
        }
    }
}
```

### src/spec/mod.rs

```rust
/// Discover and load all .spec.yml files from configured directories.
pub fn discover_specs(project_root: &Path, config: &SpecConfig) -> Result<Vec<SpecFile>>;

/// Load and validate a single spec file.
pub fn load_spec(path: &Path) -> Result<SpecFile>;

/// Load specgate.config.yml from project root. Returns default if not found.
pub fn load_config(project_root: &Path) -> Result<SpecConfig>;
```

Discovery logic:
1. For each directory in `config.spec_dirs`, recursively find files matching `*.spec.yml`.
2. Skip files inside `config.exclude` globs.
3. Parse each file with `serde_yml::from_str`.
4. Validate `version` field equals `"2"`.
5. Set `spec_path` on each loaded spec.
6. Sort specs by module name (deterministic order).

### src/spec/validation.rs

Validate loaded specs:
- `version` must be `"2"`
- `module` must be non-empty
- `constraints[].rule` must be a known rule ID: `"no-circular-deps"` or `"enforce-layer"`
- `boundaries.path` glob must be valid (test with `globset::Glob::new`)
- No two specs may declare the same `module` name
- Warn if `boundaries.allow_imports_from` and `boundaries.never_imports` overlap

Return `Vec<Diagnostic>` using `miette` for pretty error output.

---

## 4. Module Resolution Layer (src/resolver/)

**This is the #1 engineering priority.** If resolution is wrong, every rule produces garbage.

### src/resolver/mod.rs

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Resolved import classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedImport {
    /// Resolves to a file within the project.
    FirstParty {
        resolved_path: PathBuf,
        /// Which module this file belongs to (if any spec claims it).
        module_id: Option<String>,
    },
    /// Resolves to a third-party package (node_modules or Node builtin).
    ThirdParty {
        package_name: String,
    },
    /// Could not be resolved.
    Unresolvable {
        specifier: String,
        reason: String,
    },
}

/// Diagnostic output for `specgate doctor`.
#[derive(Debug)]
pub struct ResolutionExplanation {
    pub specifier: String,
    pub from_file: PathBuf,
    pub result: ResolvedImport,
    pub steps: Vec<String>,  // Human-readable resolution steps
}

pub struct ModuleResolver {
    project_root: PathBuf,
    oxc_resolver: oxc_resolver::Resolver,
    /// Cache: (containing_dir, specifier) -> result
    cache: BTreeMap<(PathBuf, String), ResolvedImport>,
    /// Module membership map: canonical file path -> module_id
    module_map: BTreeMap<PathBuf, String>,
}

impl ModuleResolver {
    /// Create resolver from project root.
    /// Reads tsconfig.json and package.json to configure oxc_resolver.
    pub fn new(project_root: &Path, specs: &[SpecFile]) -> Result<Self> {
        // 1. Build module_map from specs (path globs -> module_id)
        // 2. Configure oxc_resolver::ResolveOptions:
        //    - extensions: [".ts", ".tsx", ".js", ".jsx", ".json"]
        //    - condition_names: ["import", "require", "default"]
        //    - main_fields: ["main", "module"]
        //    - tsconfig: read from project_root/tsconfig.json
        //    - alias, alias_fields from tsconfig paths
        //    - modules: ["node_modules"]
        //    - symlinks: true (for monorepo workspace links)
        // 3. Return Self with empty cache
    }

    /// Resolve an import specifier from a given source file.
    pub fn resolve(&mut self, from_file: &Path, specifier: &str) -> ResolvedImport {
        // 1. Check cache (key = containing_dir of from_file + specifier)
        // 2. Call oxc_resolver.resolve(containing_dir, specifier)
        // 3. Classify result (see classify.rs)
        // 4. If FirstParty, look up module_id from module_map
        // 5. Cache and return
    }

    /// Build module membership map from specs.
    /// Maps each file path in the project to its owning module_id.
    pub fn build_module_map(
        project_root: &Path,
        specs: &[SpecFile],
    ) -> Result<BTreeMap<PathBuf, String>> {
        // For each spec with boundaries.path:
        //   Compile the glob pattern
        //   Walk project files matching the glob
        //   Map each file -> spec.module
        // Files not matching any spec get no module_id
    }

    /// Diagnostic: explain how a specifier resolves (for doctor command).
    pub fn explain_resolution(
        &mut self,
        from_file: &Path,
        specifier: &str,
    ) -> ResolutionExplanation {
        // Same as resolve() but collect human-readable step descriptions
    }
}
```

### src/resolver/classify.rs

Classification logic after `oxc_resolver` returns a path:

```rust
/// Classify a resolved path as FirstParty, ThirdParty, or Unresolvable.
pub fn classify_resolution(
    project_root: &Path,
    resolved_path: &Path,
    specifier: &str,
) -> ResolvedImport {
    // 1. If resolved_path is under node_modules/ → ThirdParty
    //    Extract package name: handle @scope/pkg (take first two segments)
    //    and bare pkg (take first segment)
    // 2. If specifier is a Node builtin (see BUILTINS list below) → ThirdParty
    //    Package name = the builtin name (e.g., "fs", "path", "child_process")
    // 3. If resolved_path is within project_root → FirstParty
    // 4. Otherwise → Unresolvable
}

/// Node.js built-in module names.
const NODE_BUILTINS: &[&str] = &[
    "assert", "buffer", "child_process", "cluster", "console", "constants",
    "crypto", "dgram", "dns", "domain", "events", "fs", "http", "http2",
    "https", "inspector", "module", "net", "os", "path", "perf_hooks",
    "process", "punycode", "querystring", "readline", "repl", "stream",
    "string_decoder", "sys", "timers", "tls", "trace_events", "tty",
    "url", "util", "v8", "vm", "wasi", "worker_threads", "zlib",
];

/// Check if a specifier is a Node builtin (with or without "node:" prefix).
pub fn is_node_builtin(specifier: &str) -> bool {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    // Also handle subpaths like "fs/promises"
    let base = name.split('/').next().unwrap_or(name);
    NODE_BUILTINS.contains(&base)
}

/// Extract npm package name from an import specifier.
/// "@scope/pkg/sub/path" → "@scope/pkg"
/// "lodash/fp" → "lodash"
pub fn extract_package_name(specifier: &str) -> &str {
    if specifier.starts_with('@') {
        // Scoped package: take @scope/name
        match specifier.match_indices('/').nth(1) {
            Some((idx, _)) => &specifier[..idx],
            None => specifier,
        }
    } else {
        // Bare package: take first segment
        specifier.split('/').next().unwrap_or(specifier)
    }
}
```

### oxc_resolver configuration

```rust
use oxc_resolver::{ResolveOptions, TsconfigOptions, TsconfigReferences};

fn build_resolve_options(project_root: &Path) -> ResolveOptions {
    let tsconfig_path = project_root.join("tsconfig.json");
    let tsconfig = if tsconfig_path.exists() {
        Some(TsconfigOptions {
            config_file: tsconfig_path,
            references: TsconfigReferences::Auto,
        })
    } else {
        None
    };

    ResolveOptions {
        extensions: vec![
            ".ts".to_string(),
            ".tsx".to_string(),
            ".js".to_string(),
            ".jsx".to_string(),
            ".json".to_string(),
        ],
        condition_names: vec![
            "import".to_string(),
            "require".to_string(),
            "default".to_string(),
        ],
        main_fields: vec![
            "module".to_string(),
            "main".to_string(),
        ],
        modules: vec!["node_modules".to_string()],
        symlinks: true,
        tsconfig,
        ..Default::default()
    }
}
```

---

## 5. AST Parser Layer (src/parser/)

**Syntactic only.** Extract imports/exports, discard AST immediately.

### src/parser/mod.rs

```rust
use std::path::{Path, PathBuf};
use chrono::NaiveDate;

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<ExportInfo>,
    pub re_exports: Vec<ReExportInfo>,
    pub has_dynamic_imports: bool,
}

#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// Raw import specifier string, e.g., "../utils", "@tanstack/react-query".
    pub specifier: String,
    /// True for `import type { ... }` or `import { type Foo }`.
    pub is_type_only: bool,
    /// 1-indexed line number.
    pub line: u32,
    /// 0-indexed column.
    pub column: u32,
    /// Present if the import has a `@specgate-ignore` comment.
    pub ignore_comment: Option<IgnoreComment>,
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    /// Exported identifier name. "__default" for default exports.
    pub name: String,
    pub is_type_only: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct ReExportInfo {
    /// Source module specifier.
    pub specifier: String,
    /// True for `export * from '...'`.
    pub is_star: bool,
    /// Named re-exports: `export { a, b } from '...'`. Empty if star.
    pub names: Vec<String>,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct IgnoreComment {
    pub reason: String,
    pub expiry: Option<NaiveDate>,
}

/// Parse a single TypeScript/JavaScript file and extract import/export information.
/// Returns Err only for I/O errors. Parse errors result in an empty FileAnalysis with a warning.
pub fn parse_file(path: &Path) -> Result<FileAnalysis> {
    // 1. Read file contents to String
    // 2. Create oxc_allocator::Allocator
    // 3. Determine source_type from file extension:
    //    .ts → TypeScript, .tsx → TypeScript + JSX, .js → JavaScript, .jsx → JavaScript + JSX
    // 4. Call oxc_parser::Parser::new(&allocator, &source, source_type).parse()
    // 5. Walk program.body statements:
    //    - Statement::ModuleDeclaration(decl) → match on variants:
    //      a. ModuleDeclaration::ImportDeclaration → extract ImportInfo
    //         - specifier = decl.source.value
    //         - is_type_only = decl.import_kind == ImportOrExportKind::Type
    //         - line/column from decl.span using source text
    //         - Check leading comments/trivia for @specgate-ignore
    //      b. ModuleDeclaration::ExportAllDeclaration → extract ReExportInfo (is_star=true)
    //      c. ModuleDeclaration::ExportNamedDeclaration →
    //         - If has source → ReExportInfo with names
    //         - If no source → ExportInfo for each specifier
    //      d. ModuleDeclaration::ExportDefaultDeclaration → ExportInfo { name: "__default", is_default: true }
    //    - Check for dynamic import(): walk expressions, look for ImportExpression
    //      Set has_dynamic_imports = true if found
    // 6. Drop allocator + AST (they go out of scope)
    // 7. Return FileAnalysis
}
```

### Implementation detail: accessing comments for @specgate-ignore

oxc provides comments via `parser.parse().program.comments` or `parser.parse().trivias`. For each `ImportDeclaration`, check if there's a leading comment on the same or preceding line that contains `@specgate-ignore`. Use `oxc_span::Span` to correlate comments with statements.

### src/parser/ignore.rs

```rust
use chrono::NaiveDate;

/// Parse a @specgate-ignore comment.
/// Expected formats:
///   @specgate-ignore: reason text here
///   @specgate-ignore until:2026-04-01: reason text here
/// Returns None if the comment doesn't contain @specgate-ignore.
pub fn parse_ignore_comment(comment_text: &str) -> Option<IgnoreComment> {
    let text = comment_text.trim();
    // Strip leading // or /* ... */
    let text = text.trim_start_matches("//").trim_start_matches("/*").trim_end_matches("*/").trim();

    if !text.starts_with("@specgate-ignore") {
        return None;
    }

    let rest = text.strip_prefix("@specgate-ignore").unwrap().trim();

    // Check for "until:YYYY-MM-DD:" prefix
    let (expiry, reason) = if let Some(rest) = rest.strip_prefix("until:") {
        // Parse date
        let colon_idx = rest.find(':').unwrap_or(rest.len());
        let date_str = rest[..colon_idx].trim();
        let expiry = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
        let reason = if colon_idx < rest.len() {
            rest[colon_idx + 1..].trim()
        } else {
            ""
        };
        (expiry, reason)
    } else {
        let reason = rest.strip_prefix(':').unwrap_or(rest).trim();
        (None, reason)
    };

    Some(IgnoreComment {
        reason: reason.to_string(),
        expiry,
    })
}
```

**Validation rule:** A `@specgate-ignore` with an empty reason is itself a violation. The engine must flag these.

---

## 6. Dependency Graph (src/graph/)

### src/graph/mod.rs

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use petgraph::graph::{DiGraph, NodeIndex};

use crate::parser::FileAnalysis;
use crate::resolver::{ModuleResolver, ResolvedImport};
use crate::spec::{SpecConfig, SpecFile};

#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    /// Module this file belongs to (None if not covered by any spec).
    pub module_id: Option<String>,
    pub analysis: FileAnalysis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    RuntimeImport,
    TypeOnlyImport,
    ReExport,
}

pub enum CycleScope {
    Internal,  // Within a single module
    External,  // Between modules
    Both,
}

pub struct DependencyGraph {
    graph: DiGraph<FileNode, EdgeKind>,
    file_index: BTreeMap<PathBuf, NodeIndex>,
    module_membership: BTreeMap<PathBuf, String>,
}

impl DependencyGraph {
    /// Build the complete dependency graph for the project.
    pub fn build(
        project_root: &Path,
        specs: &[SpecFile],
        resolver: &mut ModuleResolver,
        config: &SpecConfig,
    ) -> Result<Self> {
        // 1. Discover all .ts/.tsx/.js/.jsx files (see discovery.rs)
        //    Respect config.exclude globs
        // 2. Build module membership map from specs
        // 3. Parse each file → FileAnalysis
        // 4. Add each file as a node in the DiGraph
        // 5. For each file, for each import:
        //    a. Resolve via resolver.resolve(file_path, specifier)
        //    b. If FirstParty → add edge to target file node
        //       EdgeKind = TypeOnlyImport if is_type_only, else RuntimeImport
        //    c. For re-exports → add edge with EdgeKind::ReExport
        //    d. ThirdParty and Unresolvable → no edge (tracked in FileAnalysis)
        // 6. Return DependencyGraph
    }

    /// Get all file nodes belonging to a module.
    pub fn files_in_module(&self, module_id: &str) -> Vec<&FileNode> {
        // Filter nodes where module_id matches. Return sorted by path.
    }

    /// Get all module IDs that have at least one file importing from the given module.
    pub fn importers_of_module(&self, module_id: &str) -> BTreeSet<String> {
        // 1. Collect all file nodes in the target module
        // 2. For each, find all incoming edges in the graph
        // 3. Collect the module_ids of source nodes
        // 4. Return deduplicated set
    }

    /// Find strongly connected components (cycles).
    pub fn find_cycles(&self, scope: CycleScope) -> Vec<Vec<NodeIndex>> {
        // 1. Use petgraph::algo::tarjan_scc(&self.graph)
        // 2. Filter to SCCs with >1 node
        // 3. Based on scope:
        //    Internal: only SCCs where all nodes share the same module_id
        //    External: collapse nodes to module-level, find module-level SCCs
        //    Both: return all SCCs with >1 node
        // 4. Sort SCCs deterministically (by first file path in each SCC)
    }

    /// Determine which modules are affected by changes to the given files.
    /// Returns the modules containing changed files PLUS modules that import from them.
    pub fn affected_modules(&self, changed_files: &[PathBuf]) -> BTreeSet<String> {
        // 1. Find module_ids of changed files
        // 2. For each changed module, add importers_of_module
        // 3. Return union
    }

    /// Look up the module_id for a given file path.
    pub fn module_of_file(&self, path: &Path) -> Option<&str> {
        self.module_membership.get(path).map(|s| s.as_str())
    }
}
```

### src/graph/discovery.rs

```rust
use std::path::{Path, PathBuf};
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

/// Discover all TypeScript/JavaScript source files in the project.
/// Excludes paths matching any pattern in `exclude_patterns`.
pub fn discover_source_files(
    project_root: &Path,
    exclude_patterns: &[String],
) -> Result<Vec<PathBuf>> {
    // 1. Build GlobSet from exclude_patterns
    // 2. WalkDir from project_root
    // 3. Filter to files with extensions: .ts, .tsx, .js, .jsx
    // 4. Exclude files matching any exclude glob
    // 5. Canonicalize paths
    // 6. Sort for deterministic order
    // 7. Return
}
```

---

## 7. Rule Implementations (src/rules/)

### src/rules/mod.rs

```rust
use std::path::PathBuf;

use crate::graph::DependencyGraph;
use crate::resolver::ModuleResolver;
use crate::spec::{Severity, SpecConfig, SpecFile};

/// Every rule implements this trait.
pub trait Rule: Send + Sync {
    /// Unique rule identifier.
    fn id(&self) -> &str;
    /// Evaluate the rule against a module's files. Return all violations found.
    fn evaluate(&self, ctx: &RuleContext) -> Vec<Violation>;
}

/// Context provided to each rule during evaluation.
pub struct RuleContext<'a> {
    pub spec: &'a SpecFile,
    pub graph: &'a DependencyGraph,
    pub resolver: &'a ModuleResolver,
    pub config: &'a SpecConfig,
    /// Current date — for checking ignore expiry.
    pub today: chrono::NaiveDate,
}

/// A single violation found by a rule.
#[derive(Debug, Clone, Serialize)]
pub struct Violation {
    /// Path to the .spec.yml that declared this rule.
    pub spec_file: PathBuf,
    /// Module ID.
    pub module: String,
    /// Rule ID (e.g., "boundaries.never_imports").
    pub rule: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable violation message.
    pub message: String,
    /// Source file containing the violation.
    pub file: PathBuf,
    /// 1-indexed line number.
    pub line: u32,
    /// 0-indexed column.
    pub column: u32,
    /// Optional fix suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<String>,
    /// Rule-specific structured data.
    #[serde(flatten)]
    pub details: BTreeMap<String, serde_json::Value>,
}

/// Get all built-in rules.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(boundary::BoundaryCheck),
        Box::new(dependencies::DependencyBoundary),
        Box::new(circular::NoCircularDeps),
        Box::new(layers::EnforceLayer),
    ]
}

/// Check if a file path matches any test pattern.
pub fn is_test_file(path: &Path, test_patterns: &[String]) -> bool {
    // Build GlobSet from test_patterns, check if path matches any
}
```

### src/rules/boundary.rs — BoundaryCheck

**Rule IDs emitted:** `boundaries.never_imports`, `boundaries.allow_imports_from`, `boundaries.public_api`, `boundaries.type_only`

```rust
pub struct BoundaryCheck;

impl Rule for BoundaryCheck {
    fn id(&self) -> &str { "boundaries" }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Violation> {
        let boundaries = match &ctx.spec.boundaries {
            Some(b) => b,
            None => return vec![],  // No boundaries defined → nothing to check
        };

        let module_files = ctx.graph.files_in_module(&ctx.spec.module);
        let mut violations = vec![];

        for file_node in &module_files {
            // Skip test files unless enforce_in_tests is true
            if !boundaries.enforce_in_tests
                && is_test_file(&file_node.path, &ctx.config.test_patterns)
            {
                continue;
            }

            for import in &file_node.analysis.imports {
                // Skip imports with valid (non-expired) @specgate-ignore
                if let Some(ref ignore) = import.ignore_comment {
                    if ignore.reason.is_empty() {
                        // Empty reason is a violation itself
                        violations.push(/* bare @specgate-ignore violation */);
                    } else if ignore.expiry.map_or(true, |d| d >= ctx.today) {
                        continue; // Valid suppression, skip this import
                    }
                    // Expired ignore → fall through to normal checking
                }

                let resolved = ctx.resolver.resolve(&file_node.path, &import.specifier);

                match &resolved {
                    ResolvedImport::FirstParty { resolved_path, module_id } => {
                        let target_module = module_id.as_deref().unwrap_or("(unknown)");

                        // Same module → skip boundary checks
                        if module_id.as_deref() == Some(ctx.spec.module.as_str()) {
                            continue;
                        }

                        // 1. Check never_imports (deny overrides all)
                        if matches_any_pattern(target_module, &boundaries.never_imports) {
                            violations.push(Violation {
                                rule: "boundaries.never_imports".into(),
                                message: format!(
                                    "Module '{}' imports from forbidden module '{}'",
                                    ctx.spec.module, target_module
                                ),
                                details: btreemap!{
                                    "import_source" => import.specifier.clone(),
                                    "resolved_to" => resolved_path.display().to_string(),
                                },
                                ..base_violation(ctx, file_node, import)
                            });
                            continue;
                        }

                        // 2. Check allow_imports_from (default-deny when non-empty)
                        if !boundaries.allow_imports_from.is_empty() {
                            let allowed = matches_any_pattern(
                                target_module,
                                &boundaries.allow_imports_from,
                            );
                            let type_allowed = import.is_type_only
                                && matches_any_pattern(
                                    target_module,
                                    &boundaries.allow_type_imports_from,
                                );

                            if !allowed && !type_allowed {
                                violations.push(Violation {
                                    rule: "boundaries.allow_imports_from".into(),
                                    message: format!(
                                        "Module '{}' is not allowed to import from '{}'",
                                        ctx.spec.module, target_module
                                    ),
                                    ..base_violation(ctx, file_node, import)
                                });
                                continue;
                            }
                        }

                        // 3. Check public_api enforcement
                        //    Find the TARGET module's spec. If it has public_api defined,
                        //    verify the resolved_path is one of the declared entrypoints.
                        check_public_api(ctx, &boundaries, file_node, import, resolved_path, target_module, &mut violations);
                    }
                    _ => {} // ThirdParty handled by DependencyBoundary rule
                }
            }
        }

        violations
    }
}

/// Check if a module name matches any glob pattern in the list.
/// Supports exact match ("shared/types") and glob ("ui/**").
fn matches_any_pattern(module_id: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        if pattern.contains('*') || pattern.contains('?') {
            globset::Glob::new(pattern)
                .ok()
                .and_then(|g| g.compile_matcher().is_match(module_id).then_some(()))
                .is_some()
        } else {
            module_id == pattern
        }
    })
}
```

**Public API enforcement detail:**
When checking if an import goes through a declared entrypoint, resolve the `public_api` file names relative to the target module's `boundaries.path`. For example, if module `api/orders` has `public_api: ["index.ts"]` and `path: "src/api/orders/**/*"`, then the entrypoint is `src/api/orders/index.ts`. An import resolving to `src/api/orders/internal/helper.ts` is a violation.

### src/rules/dependencies.rs — DependencyBoundary

**Rule IDs emitted:** `boundaries.forbidden_dependencies`, `boundaries.allowed_dependencies`

```rust
pub struct DependencyBoundary;

impl Rule for DependencyBoundary {
    fn id(&self) -> &str { "dependencies" }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Violation> {
        let boundaries = match &ctx.spec.boundaries {
            Some(b) => b,
            None => return vec![],
        };

        // Skip if neither allowed nor forbidden are configured
        if boundaries.allowed_dependencies.is_empty()
            && boundaries.forbidden_dependencies.is_empty()
        {
            return vec![];
        }

        let module_files = ctx.graph.files_in_module(&ctx.spec.module);
        let mut violations = vec![];

        for file_node in &module_files {
            for import in &file_node.analysis.imports {
                // Check valid @specgate-ignore (same logic as boundary.rs)
                if has_valid_ignore(import, ctx.today) { continue; }

                let resolved = ctx.resolver.resolve(&file_node.path, &import.specifier);

                if let ResolvedImport::ThirdParty { package_name } = &resolved {
                    // 1. Check forbidden_dependencies (hard deny, even in tests)
                    if boundaries.forbidden_dependencies.contains(package_name) {
                        violations.push(Violation {
                            rule: "boundaries.forbidden_dependencies".into(),
                            message: format!(
                                "Module '{}' imports forbidden dependency '{}'",
                                ctx.spec.module, package_name
                            ),
                            ..base_violation(ctx, file_node, import)
                        });
                        continue;
                    }

                    // 2. Check allowed_dependencies (default-deny when non-empty)
                    //    Skip test files for this check (unless enforce_in_tests)
                    if !boundaries.allowed_dependencies.is_empty() {
                        if !boundaries.enforce_in_tests
                            && is_test_file(&file_node.path, &ctx.config.test_patterns)
                        {
                            continue;
                        }
                        if !boundaries.allowed_dependencies.contains(package_name) {
                            violations.push(Violation {
                                rule: "boundaries.allowed_dependencies".into(),
                                message: format!(
                                    "Module '{}' imports undeclared dependency '{}'",
                                    ctx.spec.module, package_name
                                ),
                                ..base_violation(ctx, file_node, import)
                            });
                        }
                    }
                }
            }
        }

        violations
    }
}
```

### src/rules/circular.rs — NoCircularDeps

**Rule ID emitted:** `no-circular-deps`

```rust
pub struct NoCircularDeps;

impl Rule for NoCircularDeps {
    fn id(&self) -> &str { "no-circular-deps" }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Violation> {
        // Only runs if spec has constraint with rule == "no-circular-deps"
        let constraint = ctx.spec.constraints.iter()
            .find(|c| c.rule == "no-circular-deps");
        let constraint = match constraint {
            Some(c) => c,
            None => return vec![],
        };

        let scope = match constraint.params.get("scope").and_then(|v| v.as_str()) {
            Some("internal") => CycleScope::Internal,
            Some("external") => CycleScope::External,
            _ => CycleScope::Both,  // Default to "both"
        };

        let cycles = ctx.graph.find_cycles(scope);
        let mut violations = vec![];

        // Filter cycles to those involving files in this module
        let module_files: BTreeSet<PathBuf> = ctx.graph
            .files_in_module(&ctx.spec.module)
            .iter()
            .map(|f| f.path.clone())
            .collect();

        for cycle in &cycles {
            let cycle_paths: Vec<&Path> = cycle.iter()
                .filter_map(|idx| ctx.graph.node(idx))
                .map(|n| n.path.as_path())
                .collect();

            // Only report if at least one file in the cycle belongs to this module
            if !cycle_paths.iter().any(|p| module_files.contains(*p)) {
                continue;
            }

            let cycle_display = cycle_paths.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" → ");

            // Report violation at the first file in the cycle that belongs to this module
            let first_file = cycle_paths.iter()
                .find(|p| module_files.contains(**p))
                .unwrap();

            violations.push(Violation {
                rule: "no-circular-deps".into(),
                severity: constraint.severity,
                message: constraint.message.clone().unwrap_or_else(|| {
                    format!("Circular dependency detected: {}", cycle_display)
                }),
                file: first_file.to_path_buf(),
                line: 0,
                column: 0,
                details: btreemap!{
                    "cycle" => serde_json::to_value(&cycle_display).unwrap(),
                    "cycle_length" => serde_json::Value::Number(cycle.len().into()),
                },
                ..default_violation(ctx)
            });
        }

        violations
    }
}
```

### src/rules/layers.rs — EnforceLayer

**Rule ID emitted:** `enforce-layer`

```rust
pub struct EnforceLayer;

impl Rule for EnforceLayer {
    fn id(&self) -> &str { "enforce-layer" }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Violation> {
        let constraint = ctx.spec.constraints.iter()
            .find(|c| c.rule == "enforce-layer");
        let constraint = match constraint {
            Some(c) => c,
            None => return vec![],
        };

        // Parse layers param: ordered list, top (index 0) to bottom (index N-1)
        // Higher index = lower layer. Lower layers cannot import from higher layers (lower index).
        let layers: Vec<String> = constraint.params.get("layers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if layers.is_empty() {
            return vec![];
        }

        // Build layer index: module_id_prefix → layer_rank (0 = highest)
        let layer_rank: BTreeMap<&str, usize> = layers.iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();

        let module_files = ctx.graph.files_in_module(&ctx.spec.module);

        // Determine this module's layer rank
        let this_rank = layer_rank.iter()
            .find(|(prefix, _)| ctx.spec.module.starts_with(**prefix) || ctx.spec.module == **prefix)
            .map(|(_, &rank)| rank);

        let this_rank = match this_rank {
            Some(r) => r,
            None => return vec![],  // Module not in any layer → rule doesn't apply
        };

        let mut violations = vec![];

        for file_node in &module_files {
            for import in &file_node.analysis.imports {
                if has_valid_ignore(import, ctx.today) { continue; }

                let resolved = ctx.resolver.resolve(&file_node.path, &import.specifier);

                if let ResolvedImport::FirstParty { module_id: Some(target_module), .. } = &resolved {
                    // Find target module's layer rank
                    let target_rank = layer_rank.iter()
                        .find(|(prefix, _)| {
                            target_module.starts_with(**prefix) || target_module == **prefix
                        })
                        .map(|(_, &rank)| rank);

                    if let Some(target_rank) = target_rank {
                        // Violation if importing from a HIGHER layer (lower rank number)
                        if target_rank < this_rank {
                            let this_layer = &layers[this_rank];
                            let target_layer = &layers[target_rank];
                            violations.push(Violation {
                                rule: "enforce-layer".into(),
                                severity: constraint.severity,
                                message: format!(
                                    "Layer violation: '{}' (layer '{}') imports from '{}' (layer '{}') — lower layers cannot import from higher layers",
                                    ctx.spec.module, this_layer, target_module, target_layer
                                ),
                                ..base_violation(ctx, file_node, import)
                            });
                        }
                    }
                }
            }
        }

        violations
    }
}
```

---

## 8. Verdict Builder (src/verdict/)

### src/verdict/mod.rs

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use serde::Serialize;

use crate::rules::Violation;
use crate::spec::Severity;

#[derive(Debug, Serialize)]
pub struct Verdict {
    /// Schema version.
    pub specgate: String,
    /// PASS or FAIL.
    pub verdict: VerdictStatus,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Total execution time in milliseconds.
    pub duration_ms: u64,
    /// Number of spec files checked.
    pub specs_checked: usize,
    /// Total number of rule evaluations.
    pub rules_evaluated: usize,
    /// Rules that passed (no violations).
    pub passed: usize,
    /// Error-severity violations.
    pub failed: usize,
    /// Warning-severity violations.
    pub warnings: usize,
    /// Suppression report.
    pub suppressions: SuppressionReport,
    /// All violations (after filtering out valid suppressions).
    pub violations: Vec<Violation>,
    /// Human-readable summary line.
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum VerdictStatus {
    Pass,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct SuppressionReport {
    /// Total active (valid, non-expired) suppressions.
    pub total: usize,
    /// Suppressions that are new in the current diff (if diff mode).
    pub new_in_diff: usize,
    /// Suppressions whose expiry date has passed.
    pub expired: Vec<ExpiredSuppression>,
}

#[derive(Debug, Serialize)]
pub struct ExpiredSuppression {
    pub file: PathBuf,
    pub line: u32,
    pub reason: String,
    pub expired_on: NaiveDate,
}

pub struct VerdictBuilder {
    violations: Vec<Violation>,
    suppressions_total: usize,
    suppressions_new_in_diff: usize,
    expired_suppressions: Vec<ExpiredSuppression>,
    specs_checked: usize,
    rules_evaluated: usize,
}

impl VerdictBuilder {
    pub fn new() -> Self { /* ... */ }

    /// Add violations from a rule evaluation.
    pub fn add_violations(&mut self, violations: Vec<Violation>) { /* ... */ }

    /// Record suppression stats.
    pub fn record_suppression(&mut self, /* ... */) { /* ... */ }

    /// Build the final verdict.
    pub fn build(self, duration_ms: u64) -> Verdict {
        // 1. Sort violations: by file path, then line, then rule (deterministic)
        // 2. Count errors vs warnings
        // 3. Determine verdict: FAIL if any error-severity violations remain
        // 4. Generate summary string
        // 5. Timestamp: chrono::Utc::now().to_rfc3339()
        // 6. Return Verdict
    }
}
```

### src/verdict/format.rs

```rust
/// Serialize verdict to JSON with sorted keys (deterministic output).
pub fn to_json(verdict: &Verdict) -> String {
    serde_json::to_string_pretty(verdict).unwrap()
}

/// Generate human-readable summary for stderr.
pub fn to_human_summary(verdict: &Verdict) -> String {
    // Example: "3 errors, 1 warning across 12 specs. 7 suppressions (1 new)."
    // Include color codes if stderr is a terminal (use std::io::IsTerminal)
}
```

**Determinism guarantee:** Same input → byte-identical JSON output. This is ensured by:
- `BTreeMap` everywhere (no HashMap)
- Violations sorted by (file, line, column, rule)
- `serde_json` with `preserve_order` feature
- No random or system-dependent values in violations (timestamps only in top-level metadata)

---

## 9. CLI Layer (src/cli/)

### src/cli/mod.rs

```rust
use clap::{Parser, Args, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "specgate",
    version,
    about = "Machine-checkable architectural intent for TypeScript projects"
)]
pub enum Cli {
    /// Run verification against specs
    Check(CheckArgs),
    /// Initialize specgate in a project
    Init(InitArgs),
    /// Validate spec files without running checks
    Validate(ValidateArgs),
    /// Diagnostic: show how the engine resolves paths and modules
    Doctor(DoctorArgs),
}

#[derive(Args)]
pub struct CheckArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Diff-aware mode: only check modules affected by changes since this ref
    #[arg(long)]
    pub diff: Option<String>,
    /// Output format
    #[arg(long, default_value = "json")]
    pub format: OutputFormat,
    /// CI mode: set exit code based on verdict
    #[arg(long)]
    pub ci: bool,
    /// Maximum warnings before failing (in addition to errors)
    #[arg(long)]
    pub max_warnings: Option<usize>,
}

#[derive(Args)]
pub struct InitArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Specific file to diagnose (if omitted, shows full project diagnostics)
    pub file: Option<PathBuf>,
    /// Project root directory
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(ValueEnum, Clone, Default)]
pub enum OutputFormat {
    #[default]
    Json,
    Human,
}
```

### src/cli/check.rs

```rust
pub fn run(args: CheckArgs) -> Result<ExitCode> {
    // 1. Canonicalize args.root
    // 2. Load config: spec::load_config(&root)
    // 3. Discover specs: spec::discover_specs(&root, &config)
    // 4. Validate specs: spec::validation::validate(&specs) — exit 2 on error
    // 5. Create resolver: ModuleResolver::new(&root, &specs)
    // 6. Build graph: DependencyGraph::build(&root, &specs, &resolver, &config)
    // 7. If diff mode: determine affected modules, filter specs
    // 8. For each spec, for each rule in all_rules():
    //    a. Build RuleContext
    //    b. Evaluate rule
    //    c. Add violations to VerdictBuilder
    // 9. Build verdict
    // 10. Output:
    //     - JSON format → stdout (verdict JSON)
    //     - Human format → stderr (human summary)
    // 11. Exit code:
    //     - 0 if verdict is PASS
    //     - 1 if verdict is FAIL
    //     - Also fail if max_warnings exceeded
}
```

### src/cli/init.rs

```rust
pub fn run(args: InitArgs) -> Result<ExitCode> {
    // 1. Create specgate.config.yml with defaults
    // 2. Create an example .spec.yml
    // 3. Print instructions to stderr
    // Exit 0
}
```

### src/cli/validate.rs

```rust
pub fn run(args: ValidateArgs) -> Result<ExitCode> {
    // 1. Load config
    // 2. Discover and load specs
    // 3. Run validation
    // 4. Print results (errors → stderr, exit 2; valid → "N specs valid", exit 0)
}
```

### src/cli/doctor.rs

```rust
pub fn run(args: DoctorArgs) -> Result<ExitCode> {
    // If args.file is Some:
    //   1. Show which module the file belongs to
    //   2. Show which specs apply
    //   3. For each import in the file, show full resolution chain
    //   4. Show which boundary rules would apply
    // If args.file is None:
    //   1. Show all discovered specs
    //   2. Show module → file count mapping
    //   3. Show tsconfig alias resolution table
    //   4. Show any config warnings
    // Exit 0
}
```

### src/main.rs

```rust
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = specgate::cli::Cli::parse();
    let result = match cli {
        specgate::cli::Cli::Check(args) => specgate::cli::check::run(args),
        specgate::cli::Cli::Init(args) => specgate::cli::init::run(args),
        specgate::cli::Cli::Validate(args) => specgate::cli::validate::run(args),
        specgate::cli::Cli::Doctor(args) => specgate::cli::doctor::run(args),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("specgate: {e:?}");
            ExitCode::from(2)
        }
    }
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Verdict: PASS |
| 1 | Verdict: FAIL (violations found) |
| 2 | Configuration/parse error (invalid spec, missing config, etc.) |

---

## 10. Test Strategy

### Unit Tests

Each module has `#[cfg(test)] mod tests` with focused unit tests:

- **spec/**: Parse valid YAML, reject invalid YAML, validate cross-spec conflicts
- **resolver/**: `extract_package_name`, `is_node_builtin`, `classify_resolution`
- **parser/**: Parse imports from TS source strings, parse `@specgate-ignore` comments with/without expiry
- **rules/**: Each rule tested with mock `RuleContext` — construct `DependencyGraph` and `FileAnalysis` manually
- **verdict/**: Determinism test — same violations → identical JSON

Use `insta` for snapshot testing of verdict JSON output. Snapshots live alongside tests.

### Integration Test Fixtures

Each fixture is a self-contained mini TypeScript project with source files, spec files, and expected verdicts.

**Fixture structure:**
```
tests/fixtures/<name>/
├── specgate.config.yml       # (optional)
├── tsconfig.json              # (if needed)
├── package.json               # (if needed)
├── src/
│   └── ...                    # TypeScript source files
├── *.spec.yml                 # Spec files
└── expected-verdict.json      # Expected verdict output (snapshot)
```

**Required fixtures:**

| Fixture | What it tests | Expected verdict |
|---------|--------------|-----------------|
| `basic/` | Simple project, 2 modules, valid boundaries | PASS |
| `boundary-violation/` | Module imports from `never_imports` target | FAIL: `boundaries.never_imports` |
| `public-api-bypass/` | External import skips index.ts entrypoint | FAIL: `boundaries.public_api` |
| `circular-deps/` | A→B→C→A circular chain | FAIL: `no-circular-deps` |
| `forbidden-dep/` | Browser module imports `fs` | FAIL: `boundaries.forbidden_dependencies` |
| `layer-violation/` | Lower layer imports from upper layer | FAIL: `enforce-layer` |
| `type-only-allowed/` | `import type` from otherwise-forbidden module | PASS |
| `escape-hatch/` | `@specgate-ignore` with reason suppresses violation | PASS (with suppression reported) |
| `expired-ignore/` | Expired `@specgate-ignore` does NOT suppress | FAIL |
| `monorepo/` | Workspace packages with cross-package imports | PASS/FAIL depending on specs |
| `barrel-reexports/` | `export * from` chains resolve correctly | PASS |
| `tsconfig-paths/` | `@/` alias imports via tsconfig paths | PASS |

**Integration test runner (tests/integration.rs):**
```rust
#[test]
fn test_fixture_basic() {
    run_fixture("basic", VerdictStatus::Pass);
}

fn run_fixture(name: &str, expected_status: VerdictStatus) {
    let fixture_dir = Path::new("tests/fixtures").join(name);
    // Run specgate check on fixture_dir
    // Compare verdict status
    // Snapshot test the full verdict JSON with insta
}
```

**Determinism test:**
```rust
#[test]
fn verdict_is_deterministic() {
    // Run check twice on the same fixture
    // Assert JSON outputs are byte-identical
}
```

### Golden Corpus

Track real agent-produced bugs in `tests/fixtures/golden/`. Each case:
- A minimal reproduction of an actual agent bug
- The spec that would have caught it
- Expected violation output
- Document: what failure mode (from §1 of MVP spec), how specgate catches it

Start building from Phase 1. Target: 10+ cases before v0.1.0 release.

---

## 11. Build Phases (Agent Orchestration)

### Phase 1 — Foundation (Sequential, 1 agent)

**Must complete before any other phase.** All subsequent work depends on these interfaces.

**Files to create:**
- `Cargo.toml`
- `rust-toolchain.toml`
- `src/main.rs` (stub: parse CLI, print "not implemented")
- `src/lib.rs` (declare all modules)
- `src/spec/mod.rs`, `src/spec/types.rs`, `src/spec/config.rs`, `src/spec/validation.rs`
- `src/resolver/mod.rs`, `src/resolver/classify.rs`
- `src/parser/mod.rs`, `src/parser/ignore.rs`
- `src/cli/mod.rs` (CLI types only, no command implementations)
- `src/rules/mod.rs` (Rule trait + Violation type only, no implementations)
- `src/graph/mod.rs` (DependencyGraph struct + method signatures only)
- `src/verdict/mod.rs` (Verdict struct only)

**Deliverables:**
1. Project compiles and runs (`cargo build`, `cargo test`)
2. Spec types fully defined and serde-deserializable
3. `load_config()` and `discover_specs()` working with unit tests
4. `ModuleResolver::new()` and `resolve()` working with unit tests
5. `parse_file()` working for .ts/.tsx files with unit tests
6. `parse_ignore_comment()` working with unit tests
7. All public trait/struct signatures are final — Phase 2 agents code against these

**Acceptance criteria:** `cargo test` passes. A test can load a .spec.yml, parse a .ts file, and resolve an import.

### Phase 2 — Graph + Rules (Parallel, up to 5 agents)

Each agent receives the Phase 1 crate (read-only) and works in an isolated git worktree. Each agent creates only the files listed below.

**Agent A: Dependency Graph** (`src/graph/mod.rs`, `src/graph/discovery.rs`)
- Implement `DependencyGraph::build()`
- Implement `discover_source_files()`
- Implement `files_in_module()`, `importers_of_module()`, `find_cycles()`, `affected_modules()`
- Unit tests with small hand-constructed graphs

**Agent B: Boundary Rule** (`src/rules/boundary.rs`)
- Implement `BoundaryCheck` rule
- All four sub-rules: never_imports, allow_imports_from, public_api, type-only exceptions
- Test file exclusion logic
- @specgate-ignore suppression handling
- Unit tests with mock RuleContext

**Agent C: Dependency Rule** (`src/rules/dependencies.rs`)
- Implement `DependencyBoundary` rule
- forbidden_dependencies and allowed_dependencies
- Node builtin detection
- Unit tests

**Agent D: Circular Deps Rule** (`src/rules/circular.rs`)
- Implement `NoCircularDeps` rule
- Internal, external, both scopes
- Tarjan SCC via petgraph
- Unit tests

**Agent E: Layer Rule** (`src/rules/layers.rs`)
- Implement `EnforceLayer` rule
- Layer rank calculation, upward-import detection
- Unit tests

**Merge strategy:** Each agent's PR is reviewed and merged independently. Conflicts should be minimal since agents own separate files. Run `cargo test` after each merge.

### Phase 3 — Integration (Sequential, 1-2 agents)

**Depends on:** All Phase 2 agents merged.

**Files to create/complete:**
- `src/verdict/mod.rs` (full VerdictBuilder implementation)
- `src/verdict/format.rs`
- `src/cli/check.rs` (full implementation)
- `src/cli/init.rs`
- `src/cli/validate.rs`
- `tests/integration.rs`
- All fixture directories under `tests/fixtures/`

**Deliverables:**
1. `specgate check` produces correct JSON verdict
2. `specgate check --ci` returns correct exit codes
3. `specgate init` creates config + example spec
4. `specgate validate` validates spec files
5. All 12 integration test fixtures passing
6. Determinism test passing
7. Diff-aware mode: `specgate check --diff HEAD~1` works

### Phase 4 — Polish (1 agent)

**Files to create/complete:**
- `src/cli/doctor.rs` (full implementation)
- `docs/spec-language.md`
- `docs/getting-started.md`
- `README.md`
- Fix hints on all violation types
- Error messages with miette diagnostics (source snippets, labels)
- Performance: profile on a 500-file fixture, optimize hot paths
- `tests/fixtures/golden/` — at least 5 real agent bug reproductions

**Acceptance criteria:**
- `specgate doctor src/some/file.ts` shows resolution chain
- All error messages include actionable fix hints
- `cargo build --release` produces a single static binary
- README has: installation, quickstart, spec language reference, CI setup

---

## 12. Performance Targets

| Scenario | Target |
|----------|--------|
| 50 files, 5 specs | < 2 seconds |
| 500 files, 30 specs | < 5 seconds |
| 5000+ files, 100+ specs | < 30 seconds |
| Diff-aware, single file change | < 1 second |

**Strategy:**
- Syntactic parsing only — never invoke TypeScript type checker
- Drop ASTs immediately after extracting FileAnalysis
- Cache module resolution results (same specifier + same directory = same result)
- Use `BTreeMap` for determinism (acceptable perf tradeoff for MVP scale)
- Optional: `--features parallel` enables rayon for file parsing on large projects
- Profile with `cargo flamegraph` in Phase 4

---

## 13. Error Handling Strategy

Use `miette` for all user-facing errors. Use `thiserror` for internal error types.

```rust
// src/lib.rs (or src/error.rs)
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum SpecgateError {
    #[error("Failed to read spec file: {path}")]
    #[diagnostic(code(specgate::spec::read_error))]
    SpecReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Invalid spec file: {path}")]
    #[diagnostic(code(specgate::spec::invalid))]
    SpecValidationError {
        path: PathBuf,
        #[help]
        help: String,
    },

    #[error("Failed to parse source file: {path}")]
    #[diagnostic(code(specgate::parser::parse_error))]
    ParseError {
        path: PathBuf,
        #[help]
        help: String,
    },

    #[error("Module resolution failed")]
    #[diagnostic(code(specgate::resolver::failed))]
    ResolutionError {
        specifier: String,
        from_file: PathBuf,
        #[help]
        help: String,
    },
}
```

**Policy:** Parse errors are warnings, not hard failures. If a file can't be parsed, log a warning and skip it. The engine should degrade gracefully — a single unparseable file shouldn't block checking 500 other files.
