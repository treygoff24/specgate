"use strict";

const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");
const { builtinModules } = require("node:module");

const TRACE_LINE_LIMIT = 48;

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

function printHelp() {
  const lines = [
    "Generate a focused TypeScript module-resolution snapshot for specgate doctor compare.",
    "",
    "Usage:",
    "  specgate-resolution-snapshot --from <file> --import <specifier> [options]",
    "",
    "Required:",
    "  --from <file>           Importing file path (relative to --project-root or absolute)",
    "  --import <specifier>    Import specifier to resolve",
    "",
    "Options:",
    "  --project-root <path>   Project root (default: current working directory)",
    "  --tsconfig <path>       Explicit tsconfig path (default: auto-find tsconfig.json)",
    "  --out <path>            Write JSON to file instead of stdout",
    "  --pretty                Pretty-print JSON",
    "  --help                  Show this help",
    "",
    "Example:",
    "  specgate-resolution-snapshot --from src/app/main.ts --import @core/utils --out .tmp/trace.focus.json --pretty"
  ];

  process.stdout.write(`${lines.join("\n")}\n`);
}

function runCli(argv) {
  if (argv === undefined) {
    argv = process.argv.slice(2);
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
  runCli,
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  extractPackageName,
};

if (require.main === module) {
  process.exitCode = runCli(process.argv.slice(2));
}
