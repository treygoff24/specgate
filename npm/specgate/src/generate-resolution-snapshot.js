"use strict";

const wrapperVersion = require("../package.json").version;
const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");
const { builtinModules } = require("node:module");

const TRACE_LINE_LIMIT = 48;

// flatMap creates both the bare name (fs) and node: prefixed name (node:fs) to handle
// both forms that appear in import statements and TypeScript's internal module list.
const BUILTIN_MODULES = new Set(
  builtinModules.flatMap((name) => [name, name.startsWith("node:") ? name.slice(5) : `node:${name}`])
);
// Built-in Node.js modules (e.g., fs, path) can be imported without resolution.

function slashify(value) {
  return value.split(path.sep).join("/");
}

function trimNodePrefix(specifier) {
  return specifier.startsWith("node:") ? specifier.slice(5) : specifier;
}

function isBuiltinImport(specifier) {
  return BUILTIN_MODULES.has(specifier) || BUILTIN_MODULES.has(trimNodePrefix(specifier));
}

function extractPackageName(specifier) {
  const trimmed = trimNodePrefix(specifier);

  if (!trimmed || trimmed.startsWith(".") || trimmed.startsWith("/") || /^[A-Za-z]:[\\/]/.test(trimmed)) {
    return null;
  }

  const parts = trimmed.split("/").filter(Boolean);
  if (parts.length === 0) {
    return null;
  }

  if (parts[0].startsWith("@") && parts.length >= 2) {
    return `${parts[0]}/${parts[1]}`;
  }

  return parts[0];
}

function looksLikePath(value) {
  return path.isAbsolute(value) || value.startsWith(".") || value.includes("/") || value.includes("\\");
}

function isNodeModulesPath(value) {
  return value.includes("/node_modules/") || value.includes("\\node_modules\\");
}

function resolvePath(base, maybeRelative) {
  if (path.isAbsolute(maybeRelative)) {
    return path.normalize(maybeRelative);
  }
  return path.resolve(base, maybeRelative);
}

function tryRealpath(value) {
  try {
    return fs.realpathSync.native(value);
  } catch {
    return path.normalize(value);
  }
}

function toProjectPath(projectRoot, absolutePath) {
  const root = tryRealpath(projectRoot);
  const target = tryRealpath(resolvePath(root, absolutePath));
  const relative = path.relative(root, target);

  if (relative === "") {
    return ".";
  }

  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    return slashify(target);
  }

  return slashify(relative);
}

function moduleResolutionName(compilerOptions) {
  const value = compilerOptions.moduleResolution;
  if (typeof value !== "number") {
    return "default";
  }

  return ts.ModuleResolutionKind[value] || "unknown";
}

function pickTsconfig(projectRoot, explicitTsconfig) {
  if (explicitTsconfig) {
    const tsconfigPath = resolvePath(projectRoot, explicitTsconfig);
    if (!fs.existsSync(tsconfigPath)) {
      throw new Error(`tsconfig file not found: ${tsconfigPath}`);
    }
    return tsconfigPath;
  }

  const discovered = ts.findConfigFile(projectRoot, ts.sys.fileExists, "tsconfig.json");
  if (!discovered) {
    throw new Error(`could not find tsconfig.json under project root: ${projectRoot}`);
  }

  return discovered;
}

function parseTsconfig(tsconfigPath) {
  const loaded = ts.readConfigFile(tsconfigPath, ts.sys.readFile);
  if (loaded.error) {
    throw new Error(ts.flattenDiagnosticMessageText(loaded.error.messageText, "\n"));
  }

  const parsed = ts.parseJsonConfigFileContent(
    loaded.config,
    ts.sys,
    path.dirname(tsconfigPath),
    undefined,
    tsconfigPath
  );

  if (parsed.errors && parsed.errors.length > 0) {
    const rendered = parsed.errors
      .map((error) => ts.flattenDiagnosticMessageText(error.messageText, "\n"))
      .join("; ");
    throw new Error(`failed to parse tsconfig: ${rendered}`);
  }

  return parsed;
}

function parseArgs(argv) {
  const args = {
    projectRoot: process.cwd(),
    pretty: false,
    workspace: false,
    tsconfigFilename: "tsconfig.json",
  };

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];

    if (token === "--help" || token === "-h") {
      args.help = true;
      continue;
    }

    if (token === "--pretty") {
      args.pretty = true;
      continue;
    }

    if (token === "--workspace") {
      args.workspace = true;
      continue;
    }

    const next = argv[i + 1];
    if (next === undefined) {
      throw new Error(`missing value for ${token}`);
    }

    if (next.startsWith("--") || next.startsWith("-")) {
      throw new Error(`argument ${token} requires a value but got flag: ${next}`);
    }

    if ((token.startsWith("--") || token.startsWith("-")) && !token.includes("=")) {
      if (next.startsWith("--") || next.startsWith("-")) {
        throw new Error(`value for ${token} cannot be a flag: ${next}`);
      }
    }

    if (token === "--from") {
      args.from = next;
      i += 1;
      continue;
    }

    if (token === "--import" || token === "--import-specifier") {
      args.importSpecifier = next;
      i += 1;
      continue;
    }

    if (token === "--project-root") {
      args.projectRoot = next;
      i += 1;
      continue;
    }

    if (token === "--tsconfig") {
      args.tsconfig = next;
      i += 1;
      continue;
    }

    if (token === "--tsconfig-filename") {
      args.tsconfigFilename = next;
      i += 1;
      continue;
    }

    if (token === "--out" || token === "--output") {
      args.output = next;
      i += 1;
      continue;
    }

    if (token.startsWith("--") || token.startsWith("-")) {
      throw new Error(`unknown argument: ${token}`);
    }
  }

  return args;
}

function buildTraceLines({
  projectRoot,
  tsconfigPath,
  fromPath,
  importSpecifier,
  compilerOptions,
  resolvedTo,
  result,
  resultKind,
}) {
  const lines = [];
  lines.push(`tsconfig: ${toProjectPath(projectRoot, tsconfigPath)}`);
  lines.push(`module_resolution: ${moduleResolutionName(compilerOptions)}`);
  lines.push(`from: ${fromPath}`);
  lines.push(`import: ${importSpecifier}`);
  lines.push(`result_kind: ${resultKind}`);

  if (resolvedTo) {
    lines.push(`resolved_to: ${resolvedTo}`);
  }

  if (result && Array.isArray(result.failedLookupLocations) && result.failedLookupLocations.length > 0) {
    const failed = result.failedLookupLocations.slice(0, 32).map((lookup) => {
      if (!looksLikePath(lookup)) {
        return `failed_lookup: ${lookup}`;
      }

      const asAbsolute = path.isAbsolute(lookup) ? lookup : resolvePath(projectRoot, lookup);
      return `failed_lookup: ${toProjectPath(projectRoot, asAbsolute)}`;
    });
    lines.push(...failed);
  }

  if (result && Array.isArray(result.resolutionDiagnostics) && result.resolutionDiagnostics.length > 0) {
    for (const diagnostic of result.resolutionDiagnostics) {
      lines.push(`diagnostic: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`);
    }
  }

  return lines.slice(0, TRACE_LINE_LIMIT);
}

function classifyResolution(importSpecifier, resolvedModule) {
  if (!resolvedModule) {
    if (isBuiltinImport(importSpecifier)) {
      return {
        resultKind: "third_party",
        resolvedTo: null,
        packageName: trimNodePrefix(importSpecifier),
      };
    }

    return {
      resultKind: "unresolvable",
      resolvedTo: null,
      packageName: null,
    };
  }

  const resolvedFileName = resolvedModule.resolvedFileName || "";
  const pathLike = looksLikePath(resolvedFileName);
  const external = resolvedModule.isExternalLibraryImport || isNodeModulesPath(slashify(resolvedFileName));

  if (external || !pathLike) {
    return {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: extractPackageName(importSpecifier),
    };
  }

  return {
    resultKind: "first_party",
    resolvedTo: resolvedFileName,
    packageName: null,
  };
}

function generateResolutionSnapshot(options) {
  if (!options || !options.from) {
    throw new Error("missing required --from <file>");
  }

  if (!options.importSpecifier) {
    throw new Error("missing required --import <specifier>");
  }

  const projectRoot = tryRealpath(resolvePath(process.cwd(), options.projectRoot || process.cwd()));
  const fromAbsolute = resolvePath(projectRoot, options.from);

  if (!fs.existsSync(fromAbsolute)) {
    throw new Error(`--from file not found: ${toProjectPath(projectRoot, fromAbsolute)}`);
  }

  const tsconfigPath = pickTsconfig(projectRoot, options.tsconfig);
  const parsedConfig = parseTsconfig(tsconfigPath);
  const compilerOptions = parsedConfig.options || {};

  const host = {
    fileExists: ts.sys.fileExists,
    readFile: ts.sys.readFile,
    directoryExists: ts.sys.directoryExists ? ts.sys.directoryExists.bind(ts.sys) : undefined,
    getCurrentDirectory: () => projectRoot,
    getDirectories: ts.sys.getDirectories ? ts.sys.getDirectories.bind(ts.sys) : undefined,
    realpath: tryRealpath,
  };

  // Cache key function uses lowercase normalization for case-insensitive filesystems
  // (macOS, Windows). On case-sensitive systems (Linux), this could theoretically cause
  // cache collisions for files differing only in case, but TypeScript's module resolution
  // is already case-aware and consistent within a project. The lowercase normalization
  // ensures cache hits across platforms when the same project is developed on different OSes.
  const cache = ts.createModuleResolutionCache(projectRoot, (v) => v.toLowerCase(), compilerOptions);
  const resolutionResult = ts.resolveModuleName(
    options.importSpecifier,
    fromAbsolute,
    compilerOptions,
    host,
    cache
  );

  const classified = classifyResolution(options.importSpecifier, resolutionResult.resolvedModule);
  const fromProjectPath = toProjectPath(projectRoot, fromAbsolute);
  const resolvedProjectPath =
    classified.resolvedTo && looksLikePath(classified.resolvedTo)
      ? toProjectPath(projectRoot, resolvePath(projectRoot, classified.resolvedTo))
      : null;

  const trace = buildTraceLines({
    projectRoot,
    tsconfigPath,
    fromPath: fromProjectPath,
    importSpecifier: options.importSpecifier,
    compilerOptions,
    resolvedTo: resolvedProjectPath,
    result: resolutionResult,
    resultKind: classified.resultKind,
  });

  const resolutionRecord = {
    source: "tsc_compiler_api",
    from: fromProjectPath,
    import: options.importSpecifier,
    import_specifier: options.importSpecifier,
    result_kind: classified.resultKind,
    trace,
  };

  if (resolvedProjectPath) {
    resolutionRecord.resolved_to = resolvedProjectPath;
  }

  if (classified.packageName) {
    resolutionRecord.package_name = classified.packageName;
  }

  const edges = [];
  if (classified.resultKind === "first_party" && resolvedProjectPath) {
    edges.push({
      from: fromProjectPath,
      to: resolvedProjectPath,
    });
  }

  return {
    schema_version: "1",
    snapshot_kind: "doctor_compare_tsc_resolution_focus",
    producer: "specgate-npm-wrapper",
    wrapper_version: wrapperVersion,
    generated_at: new Date().toISOString(),
    project_root: slashify(projectRoot),
    tsconfig_path: toProjectPath(projectRoot, tsconfigPath),
    focus: {
      from: fromProjectPath,
      import_specifier: options.importSpecifier,
    },
    resolutions: [resolutionRecord],
    edges,
  };
}

// ---------------------------------------------------------------------------
// Workspace discovery helpers
// ---------------------------------------------------------------------------

/**
 * Parse pnpm-workspace.yaml and return workspace glob patterns.
 * This is a minimal parser — pnpm-workspace.yaml uses a simple YAML structure
 * with a `packages` key containing a list of strings.
 *
 * @param {string} content - Raw YAML content
 * @returns {string[]}
 */
function parsePnpmWorkspaceYaml(content) {
  const patterns = [];
  let inPackages = false;

  for (const rawLine of content.split("\n")) {
    const line = rawLine;
    const trimmed = line.trimEnd();

    if (/^packages\s*:/.test(trimmed)) {
      inPackages = true;
      continue;
    }

    if (inPackages) {
      // A new top-level key ends the packages list
      if (/^[a-zA-Z]/.test(trimmed) && !trimmed.startsWith(" ") && !trimmed.startsWith("-")) {
        inPackages = false;
        continue;
      }

      const listMatch = trimmed.match(/^\s*-\s*["']?(.+?)["']?\s*$/);
      if (listMatch) {
        const raw = listMatch[1].trim();
        if (raw) {
          patterns.push(raw);
        }
      }
    }
  }

  return patterns;
}

// Expand a single workspace glob pattern relative to projectRoot.
// Supports simple dir/* patterns, suffix patterns like packages/*-web,
// nested patterns like apps/*/pkg, and dir/** double-star for deep traversal.
// Only a single * wildcard segment is supported. Complex glob syntax
// (character classes, brace expansion) is not handled.
// Double-star with suffix (e.g., **-web) is not supported and returns empty.
// Double-star with subPath (e.g., **/pkg) is not supported and returns empty.
function expandWorkspaceGlob(projectRoot, pattern) {
  const normalized = pattern.replace(/\\/g, "/").replace(/^\.\//, "").replace(/\/$/, "");

  if (!normalized.includes("*")) {
    const candidate = path.join(projectRoot, normalized);
    if (fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()) {
      return [candidate];
    }
    return [];
  }

  // Support patterns like "packages/*", "packages/**", "packages/*-web",
  // or "apps/*/pkg" (where text appears after the wildcard segment).
  const starIndex = normalized.indexOf("*");
  const prefix = normalized.slice(0, starIndex).replace(/\/$/, "");
  const searchRoot = prefix ? path.join(projectRoot, prefix) : projectRoot;

  if (!fs.existsSync(searchRoot) || !fs.statSync(searchRoot).isDirectory()) {
    return [];
  }

  // Extract suffix within the wildcard segment (e.g., "-web" from "packages/*-web")
  // and any trailing subpath (e.g., "pkg" from "apps/*/pkg").
  const afterStar = normalized.slice(starIndex + 1);
  const nextSlash = afterStar.indexOf("/");
  const segmentSuffix = nextSlash === -1 ? afterStar : afterStar.slice(0, nextSlash);
  // subPath is the path component that must exist inside the matched directory
  // (e.g., "pkg" for "apps/*/pkg").
  const subPath = nextSlash === -1 ? "" : afterStar.slice(nextSlash + 1);

  // Guard: double-star combined with a suffix (e.g., "**-web") is unsupported.
  // For "packages/**-web", starIndex lands on the first "*", making afterStar
  // equal to "*-web" and segmentSuffix equal to "*-web". A plain "**" produces
  // segmentSuffix "*" which is the conventional "no suffix" sentinel — that is
  // handled correctly below, so only guard when segmentSuffix contains "*" but
  // is not exactly "*".
  if (segmentSuffix.includes("*") && segmentSuffix !== "*") {
    process.stderr.write(
      `[specgate] Warning: expandWorkspaceGlob does not support double-star with suffix patterns (got "${pattern}"). Returning empty.\n`
    );
    return [];
  }

  const isDeep = normalized.includes("**");

  // Guard: double-star with subPath (e.g., "**/pkg") would require recursive
  // scanning at every depth, which is unsupported.  Reject early.
  if (isDeep && subPath) {
    process.stderr.write(
      `[specgate] Warning: expandWorkspaceGlob does not support double-star with subPath patterns (got "${pattern}"). Returning empty.\n`
    );
    return [];
  }

  const results = [];

  function walkDir(dir, depth) {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      if (!entry.isDirectory()) {
        continue;
      }

      // Apply suffix filter: if the glob has text after `*` in the same
      // segment (e.g., `*-web`), only include dirs whose name ends with
      // that suffix.
      if (segmentSuffix && segmentSuffix !== "*" && !entry.name.endsWith(segmentSuffix)) {
        continue;
      }

      const fullPath = path.join(dir, entry.name);

      if (subPath) {
        // For nested patterns like "apps/*/pkg", check that the required
        // subPath directory exists inside the matched directory.
        const nested = path.join(fullPath, subPath);
        try {
          if (fs.statSync(nested).isDirectory()) {
            results.push(nested);
          }
        } catch {
          // nested subPath does not exist — skip
        }
      } else {
        results.push(fullPath);
      }

      if (isDeep && !subPath && depth < 8) {
        walkDir(fullPath, depth + 1);
      }
    }
  }

  walkDir(searchRoot, 0);
  return results;
}

/**
 * Discover workspace packages in a project.
 *
 * Checks for pnpm-workspace.yaml first, then falls back to package.json
 * `workspaces` field. For each discovered package directory, reads
 * package.json for the name and checks for a tsconfig file.
 *
 * Returns results sorted by name (parity with Rust implementation).
 *
 * @param {string} projectRoot - Absolute path to the project root
 * @param {string} [tsconfigFilename] - Name of the tsconfig file to look for (default: "tsconfig.json")
 * @returns {Array<{name: string, dir: string, tsconfigPath: string|null}>}
 */
function discoverWorkspacePackages(projectRoot, tsconfigFilename = "tsconfig.json") {
  const root = tryRealpath(resolvePath(process.cwd(), projectRoot));
  let globPatterns = [];

  // 1. Try pnpm-workspace.yaml first.
  // pnpm-workspace.yaml is authoritative when present: pnpm ignores the
  // package.json `workspaces` field entirely when this file exists, so we
  // mirror that precedence here.
  const pnpmYamlPath = path.join(root, "pnpm-workspace.yaml");
  if (fs.existsSync(pnpmYamlPath)) {
    try {
      const content = fs.readFileSync(pnpmYamlPath, "utf8");
      globPatterns = parsePnpmWorkspaceYaml(content);
    } catch {
      // ignore parse errors; fall through to package.json
    }
  }

  // 2. Fall back to package.json workspaces (npm / Yarn classic)
  if (globPatterns.length === 0) {
    const pkgJsonPath = path.join(root, "package.json");
    if (fs.existsSync(pkgJsonPath)) {
      try {
        const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
        const workspaces = pkgJson.workspaces;
        if (Array.isArray(workspaces)) {
          globPatterns = workspaces;
        } else if (workspaces && Array.isArray(workspaces.packages)) {
          globPatterns = workspaces.packages;
        }
      } catch {
        // ignore parse errors
      }
    }
  }

  if (globPatterns.length === 0) {
    return [];
  }

  // Expand all glob patterns and deduplicate
  const candidateDirsSet = new Set();
  for (const pattern of globPatterns) {
    for (const dir of expandWorkspaceGlob(root, pattern)) {
      candidateDirsSet.add(dir);
    }
  }

  const packages = [];

  for (const dir of candidateDirsSet) {
    // Each package directory must have a package.json
    const pkgJsonPath = path.join(dir, "package.json");
    if (!fs.existsSync(pkgJsonPath)) {
      continue;
    }

    let name;
    try {
      const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
      name = pkgJson.name || path.basename(dir);
    } catch {
      name = path.basename(dir);
    }

    const tsconfigPath = path.join(dir, tsconfigFilename);
    packages.push({
      name,
      dir: slashify(dir),
      tsconfigPath: fs.existsSync(tsconfigPath) ? slashify(tsconfigPath) : null,
    });
  }

  // Sort by name, matching Rust parity
  packages.sort((a, b) => a.name.localeCompare(b.name));

  return packages;
}

/**
 * Generate a batch workspace snapshot for all packages in a workspace.
 *
 * @param {string} projectRoot - Absolute path to the project root
 * @param {object} [options]
 * @param {string} [options.tsconfigFilename] - Name of tsconfig file to look for (default: "tsconfig.json")
 * @returns {object} Workspace snapshot object
 */
function generateWorkspaceSnapshot(projectRoot, options = {}) {
  const { tsconfigFilename = "tsconfig.json" } = options;
  const root = tryRealpath(resolvePath(process.cwd(), projectRoot));
  const discovered = discoverWorkspacePackages(root, tsconfigFilename);

  // Only include packages that have a tsconfig
  const packageResults = discovered
    .filter((pkg) => pkg.tsconfigPath !== null)
    .map((pkg) => ({
      name: pkg.name,
      dir: toProjectPath(root, pkg.dir),
      tsconfig_path: toProjectPath(root, pkg.tsconfigPath),
    }));

  return {
    schema_version: "1",
    snapshot_kind: "doctor_compare_tsc_resolution_batch",
    producer: "specgate-npm-wrapper",
    wrapper_version: wrapperVersion,
    generated_at: new Date().toISOString(),
    project_root: slashify(root),
    packages: packageResults,
  };
}

function printHelp() {
  const lines = [
    "Generate a TypeScript module-resolution snapshot for specgate doctor compare.",
    "",
    "Usage (focused mode):",
    "  specgate-resolution-snapshot --from <file> --import <specifier> [options]",
    "",
    "Usage (workspace batch mode):",
    "  specgate-resolution-snapshot --workspace [options]",
    "",
    "Focused mode (required):",
    "  --from <file>                Importing file path (relative to --project-root or absolute)",
    "  --import <specifier>         Import specifier to resolve",
    "",
    "Workspace batch mode:",
    "  --workspace                  Discover all packages in workspace and produce batch snapshot",
    "  --tsconfig-filename <name>   Tsconfig filename to look for in each package (default: tsconfig.json)",
    "",
    "Shared options:",
    "  --project-root <path>        Project root (default: current working directory)",
    "  --tsconfig <path>            Explicit tsconfig path (focused mode only; default: auto-find tsconfig.json)",
    "  --out <path>                 Write JSON to file instead of stdout",
    "  --pretty                     Pretty-print JSON",
    "  --help                       Show this help",
    "",
    "Examples:",
    "  specgate-resolution-snapshot --from src/app/main.ts --import @core/utils --out .tmp/trace.focus.json --pretty",
    "  specgate-resolution-snapshot --workspace --out .tmp/workspace.batch.json --pretty",
    "  specgate-resolution-snapshot --workspace --tsconfig-filename tsconfig.build.json"
  ];

  process.stdout.write(`${lines.join("\n")}\n`);
}

/**
 * Run the CLI with the given argument vector.
 *
 * @param {string[]} argv - Command-line arguments.
 * @returns {number} Exit code (0 for success, 1 for failure)
 */
function runCli(argv) {
  if (!Array.isArray(argv)) {
    process.stderr.write("Argument error: runCli(argv) requires an explicit argv array\n");
    return 1;
  }

  let args;
  try {
    args = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`Argument error: ${error.message}\n`);
    process.stderr.write("Run with --help for usage.\n");
    return 1;
  }

  if (args.help) {
    printHelp();
    return 0;
  }

  if (args.workspace) {
    try {
      const snapshot = generateWorkspaceSnapshot(args.projectRoot, {
        tsconfigFilename: args.tsconfigFilename,
      });
      const spacing = args.pretty ? 2 : 0;
      const payload = `${JSON.stringify(snapshot, null, spacing)}\n`;

      if (args.output) {
        const outputPath = resolvePath(process.cwd(), args.output);
        fs.mkdirSync(path.dirname(outputPath), { recursive: true });
        fs.writeFileSync(outputPath, payload, "utf8");
      } else {
        process.stdout.write(payload);
      }

      return 0;
    } catch (error) {
      process.stderr.write(`Failed to generate workspace snapshot: ${error.message}\n`);
      return 1;
    }
  }

  try {
    const snapshot = generateResolutionSnapshot(args);
    const spacing = args.pretty ? 2 : 0;
    const payload = `${JSON.stringify(snapshot, null, spacing)}\n`;

    if (args.output) {
      const outputPath = resolvePath(process.cwd(), args.output);
      fs.mkdirSync(path.dirname(outputPath), { recursive: true });
      fs.writeFileSync(outputPath, payload, "utf8");
    } else {
      process.stdout.write(payload);
    }

    return 0;
  } catch (error) {
    process.stderr.write(`Failed to generate resolution snapshot: ${error.message}\n`);
    return 1;
  }
}

module.exports = {
  generateResolutionSnapshot,
  generateWorkspaceSnapshot,
  discoverWorkspacePackages,
  runCli,
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  extractPackageName,
  wrapperVersion,
  // Utility functions exported for testing
  slashify,
  trimNodePrefix,
  looksLikePath,
  isNodeModulesPath,
  resolvePath,
  tryRealpath,
  expandWorkspaceGlob,
};

if (require.main === module) {
  process.exitCode = runCli(process.argv.slice(2));
}
