"use strict";

const assert = require("node:assert");
const path = require("node:path");
const fs = require("node:fs");
const {
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  extractPackageName,
  generateResolutionSnapshot,
  discoverWorkspacePackages,
  generateWorkspaceSnapshot,
  slashify,
  trimNodePrefix,
  looksLikePath,
  isNodeModulesPath,
  resolvePath,
  tryRealpath,
} = require("./generate-resolution-snapshot.js");

// Simple test framework
function describe(name, fn) {
  process.stdout.write(`\n${name}\n`);
  fn();
}

function it(name, fn) {
  try {
    fn();
    process.stdout.write(`  ✓ ${name}\n`);
  } catch (error) {
    process.stdout.write(`  ✗ ${name}\n`);
    process.stderr.write(`    ${error.message}\n`);
    process.stderr.write(`    at ${error.stack.split("\n")[2]}\n`);
    throw error;
  }
}

function assertStrictEqual(actual, expected, message) {
  assert.strictEqual(actual, expected, message);
}

function assertDeepStrictEqual(actual, expected, message) {
  assert.deepStrictEqual(actual, expected, message);
}

function assertThrows(fn, expectedMessage) {
  let threw = false;
  try {
    fn();
  } catch (error) {
    threw = true;
    if (expectedMessage && !error.message.includes(expectedMessage)) {
      throw new Error(
        `Expected error message to include "${expectedMessage}" but got "${error.message}"`
      );
    }
  }
  if (!threw) {
    throw new Error("Expected function to throw");
  }
}

// ============================================================================
// Utility Function Tests
// ============================================================================

describe("slashify", () => {
  it("converts platform-specific separators to forward slashes", () => {
    // On Windows, path.sep is "\\", on macOS/Linux it's "/"
    const platformPath = `src${path.sep}components${path.sep}Button.tsx`;
    const result = slashify(platformPath);
    assertStrictEqual(result, "src/components/Button.tsx");
  });

  it("leaves forward slashes unchanged", () => {
    assertStrictEqual(slashify("src/components/Button.tsx"), "src/components/Button.tsx");
  });

  it("handles empty string", () => {
    assertStrictEqual(slashify(""), "");
  });
});

describe("trimNodePrefix", () => {
  it("removes node: prefix from builtin modules", () => {
    assertStrictEqual(trimNodePrefix("node:fs"), "fs");
    assertStrictEqual(trimNodePrefix("node:path"), "path");
    assertStrictEqual(trimNodePrefix("node:util"), "util");
  });

  it("leaves unprefixed modules unchanged", () => {
    assertStrictEqual(trimNodePrefix("fs"), "fs");
    assertStrictEqual(trimNodePrefix("lodash"), "lodash");
    assertStrictEqual(trimNodePrefix("./local"), "./local");
  });

  it("handles empty string", () => {
    assertStrictEqual(trimNodePrefix(""), "");
  });
});

describe("looksLikePath", () => {
  it("returns true for absolute paths", () => {
    assertStrictEqual(looksLikePath("/absolute/path"), true);
    assertStrictEqual(looksLikePath("/"), true);
  });

  it("returns true for relative paths", () => {
    assertStrictEqual(looksLikePath("./local"), true);
    assertStrictEqual(looksLikePath("../parent"), true);
    assertStrictEqual(looksLikePath("../../grandparent"), true);
  });

  it("returns true for paths with separators", () => {
    assertStrictEqual(looksLikePath("src/components"), true);
    assertStrictEqual(looksLikePath("src\\components"), true);
  });

  it("returns false for simple package names", () => {
    assertStrictEqual(looksLikePath("lodash"), false);
    assertStrictEqual(looksLikePath("express"), false);
  });

  it("returns true for scoped packages (they contain /)", () => {
    // Scoped packages like @babel/core contain a /, so they look like paths
    assertStrictEqual(looksLikePath("@babel/core"), true);
    assertStrictEqual(looksLikePath("@types/node"), true);
  });

  it("returns false for builtin modules", () => {
    assertStrictEqual(looksLikePath("fs"), false);
    assertStrictEqual(looksLikePath("node:path"), false);
  });
});

describe("isNodeModulesPath", () => {
  it("returns true for paths containing node_modules", () => {
    assertStrictEqual(isNodeModulesPath("/project/node_modules/lodash/index.js"), true);
    assertStrictEqual(isNodeModulesPath("C:\\project\\node_modules\\lodash\\index.js"), true);
  });

  it("returns false for paths without node_modules", () => {
    assertStrictEqual(isNodeModulesPath("/project/src/index.ts"), false);
    assertStrictEqual(isNodeModulesPath("src/components/Button.tsx"), false);
  });

  it("returns false for partial matches", () => {
    assertStrictEqual(isNodeModulesPath("/project/node_modules_backup/file.js"), false);
    assertStrictEqual(isNodeModulesPath("/project/my_node_modules/file.js"), false);
  });
});

describe("resolvePath", () => {
  it("returns absolute paths unchanged (normalized)", () => {
    const result = resolvePath("/base", "/absolute/path");
    assertStrictEqual(result, "/absolute/path");
  });

  it("resolves relative paths against base", () => {
    const result = resolvePath("/base", "relative/path");
    assertStrictEqual(result, "/base/relative/path");
  });

  it("handles dot paths", () => {
    const result = resolvePath("/base", ".");
    assertStrictEqual(result, "/base");
  });
});

describe("tryRealpath", () => {
  it("returns realpath for existing paths", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const realPath = tryRealpath(tempDir);
    assertStrictEqual(typeof realPath, "string");
    assertStrictEqual(realPath.length > 0, true);
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  it("returns normalized path for non-existent paths", () => {
    const result = tryRealpath("/nonexistent/path/that/does/not/exist");
    assertStrictEqual(result, "/nonexistent/path/that/does/not/exist");
  });
});

// ============================================================================
// Builtin Import Tests
// ============================================================================

describe("isBuiltinImport", () => {
  it("returns true for core Node.js modules", () => {
    assertStrictEqual(isBuiltinImport("fs"), true);
    assertStrictEqual(isBuiltinImport("path"), true);
    assertStrictEqual(isBuiltinImport("os"), true);
    assertStrictEqual(isBuiltinImport("http"), true);
    assertStrictEqual(isBuiltinImport("https"), true);
    assertStrictEqual(isBuiltinImport("util"), true);
    assertStrictEqual(isBuiltinImport("stream"), true);
    assertStrictEqual(isBuiltinImport("events"), true);
  });

  it("returns true for node: prefixed modules", () => {
    assertStrictEqual(isBuiltinImport("node:fs"), true);
    assertStrictEqual(isBuiltinImport("node:path"), true);
    assertStrictEqual(isBuiltinImport("node:os"), true);
    assertStrictEqual(isBuiltinImport("node:util"), true);
  });

  it("returns false for non-builtin modules", () => {
    assertStrictEqual(isBuiltinImport("lodash"), false);
    assertStrictEqual(isBuiltinImport("express"), false);
    assertStrictEqual(isBuiltinImport("react"), false);
    assertStrictEqual(isBuiltinImport("vue"), false);
  });

  it("returns false for relative imports", () => {
    assertStrictEqual(isBuiltinImport("./local"), false);
    assertStrictEqual(isBuiltinImport("../parent"), false);
    assertStrictEqual(isBuiltinImport("../../grandparent"), false);
  });

  it("returns false for absolute paths", () => {
    assertStrictEqual(isBuiltinImport("/absolute/path"), false);
  });

  it("returns false for scoped packages", () => {
    assertStrictEqual(isBuiltinImport("@scope/package"), false);
    assertStrictEqual(isBuiltinImport("@types/node"), false);
  });

  it("handles edge cases", () => {
    assertStrictEqual(isBuiltinImport(""), false);
    assertStrictEqual(isBuiltinImport("fs/promises"), true);
    assertStrictEqual(isBuiltinImport("node:fs/promises"), true);
  });
});

// ============================================================================
// Package Name Extraction Tests
// ============================================================================

describe("extractPackageName", () => {
  it("extracts package name from simple imports", () => {
    assertStrictEqual(extractPackageName("lodash"), "lodash");
    assertStrictEqual(extractPackageName("express"), "express");
    assertStrictEqual(extractPackageName("axios"), "axios");
    assertStrictEqual(extractPackageName("react"), "react");
  });

  it("extracts package name from scoped imports", () => {
    assertStrictEqual(extractPackageName("@babel/core"), "@babel/core");
    assertStrictEqual(extractPackageName("@types/node"), "@types/node");
    assertStrictEqual(extractPackageName("@scope/package"), "@scope/package");
    assertStrictEqual(extractPackageName("@org/team/package"), "@org/team");
  });

  it("extracts package name from subpath imports", () => {
    assertStrictEqual(extractPackageName("lodash/debounce"), "lodash");
    assertStrictEqual(extractPackageName("@babel/core/lib/index"), "@babel/core");
  });

  it("extracts package name from node: prefixed imports", () => {
    assertStrictEqual(extractPackageName("node:fs"), "fs");
    assertStrictEqual(extractPackageName("node:path"), "path");
    assertStrictEqual(extractPackageName("node:util"), "util");
  });

  it("returns null for relative imports", () => {
    assertStrictEqual(extractPackageName("./local"), null);
    assertStrictEqual(extractPackageName("../parent"), null);
    assertStrictEqual(extractPackageName("../../grandparent"), null);
    assertStrictEqual(extractPackageName("./"), null);
  });

  it("returns null for absolute paths", () => {
    assertStrictEqual(extractPackageName("/absolute/path"), null);
    if (path.sep === "\\") {
      assertStrictEqual(extractPackageName("C:\\absolute\\path"), null);
    }
  });

  it("returns null for empty or invalid inputs", () => {
    assertStrictEqual(extractPackageName(""), null);
    assertStrictEqual(extractPackageName("/"), null);
    assertStrictEqual(extractPackageName("///"), null);
  });

  it("handles edge cases with scoped packages", () => {
    // @ by itself is not a valid scoped package, but the function returns it
    assertStrictEqual(extractPackageName("@"), "@");
    // @/ splits to [@, ] then filter(Boolean) gives [@], so returns @
    assertStrictEqual(extractPackageName("@/"), "@");
    // @scope without package name returns @scope (first part only)
    assertStrictEqual(extractPackageName("@scope"), "@scope");
    // @scope/ splits to [@scope, ] then filter(Boolean) gives [@scope], so returns @scope
    assertStrictEqual(extractPackageName("@scope/"), "@scope");
  });
});

// ============================================================================
// Classification Tests
// ============================================================================

describe("classifyResolution", () => {
  it("classifies builtin modules as third_party with package name", () => {
    const result = classifyResolution("fs", null);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: "fs",
    });
  });

  it("classifies node: prefixed builtin modules", () => {
    const result = classifyResolution("node:path", null);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: "path",
    });
  });

  it("classifies unresolvable imports", () => {
    const result = classifyResolution("nonexistent-package", null);
    assertDeepStrictEqual(result, {
      resultKind: "unresolvable",
      resolvedTo: null,
      packageName: null,
    });
  });

  it("classifies third-party modules from node_modules", () => {
    const resolvedModule = {
      resolvedFileName: "/project/node_modules/lodash/index.js",
      isExternalLibraryImport: true,
    };
    const result = classifyResolution("lodash", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: "lodash",
    });
  });

  it("classifies scoped third-party modules", () => {
    const resolvedModule = {
      resolvedFileName: "/project/node_modules/@babel/core/lib/index.js",
      isExternalLibraryImport: true,
    };
    const result = classifyResolution("@babel/core", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: "@babel/core",
    });
  });

  it("classifies first-party modules", () => {
    const resolvedModule = {
      resolvedFileName: "/project/src/utils/index.ts",
      isExternalLibraryImport: false,
    };
    const result = classifyResolution("./utils", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "first_party",
      resolvedTo: "/project/src/utils/index.ts",
      packageName: null,
    });
  });

  it("classifies first-party modules without isExternalLibraryImport flag", () => {
    const resolvedModule = {
      resolvedFileName: "/project/src/components/Button.tsx",
    };
    const result = classifyResolution("./Button", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "first_party",
      resolvedTo: "/project/src/components/Button.tsx",
      packageName: null,
    });
  });

  it("classifies node_modules paths even without isExternalLibraryImport flag", () => {
    const resolvedModule = {
      resolvedFileName: "/project/node_modules/axios/lib/axios.js",
    };
    const result = classifyResolution("axios", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: "axios",
    });
  });

  it("handles resolved modules with empty resolvedFileName", () => {
    const resolvedModule = {
      resolvedFileName: "",
      isExternalLibraryImport: false,
    };
    const result = classifyResolution("./utils", resolvedModule);
    assertDeepStrictEqual(result, {
      resultKind: "third_party",
      resolvedTo: null,
      packageName: null,
    });
  });
});

// ============================================================================
// Path Conversion Tests
// ============================================================================

describe("toProjectPath", () => {
  it("converts absolute paths to relative project paths", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const absolutePath = path.join(projectRoot, "src", "index.ts");
    const result = toProjectPath(projectRoot, absolutePath);
    assertStrictEqual(result, "src/index.ts");
  });

  it("handles paths within project root", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const absolutePath = path.join(projectRoot, "src", "generate-resolution-snapshot.js");
    const result = toProjectPath(projectRoot, absolutePath);
    assertStrictEqual(result, "src/generate-resolution-snapshot.js");
  });

  it("returns dot for project root itself", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const result = toProjectPath(projectRoot, projectRoot);
    assertStrictEqual(result, ".");
  });

  it("normalizes path separators to forward slashes", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const absolutePath = path.join(projectRoot, "src", "index.ts");
    const result = toProjectPath(projectRoot, absolutePath);
    assertStrictEqual(result.includes("\\"), false);
  });

  it("handles paths outside project root", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const outsidePath = "/outside/project/file.ts";
    const result = toProjectPath(projectRoot, outsidePath);
    assertStrictEqual(result, "/outside/project/file.ts");
  });

  it("handles relative paths", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const result = toProjectPath(projectRoot, "src/index.ts");
    assertStrictEqual(result, "src/index.ts");
  });

  it("handles nested directories", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const absolutePath = path.join(projectRoot, "src", "deep", "nested", "file.ts");
    const result = toProjectPath(projectRoot, absolutePath);
    assertStrictEqual(result, "src/deep/nested/file.ts");
  });
});

// ============================================================================
// Argument Parsing Tests
// ============================================================================

describe("parseArgs", () => {
  it("parses required arguments", () => {
    const args = parseArgs(["--from", "src/app.ts", "--import", "./utils"]);
    assertStrictEqual(args.from, "src/app.ts");
    assertStrictEqual(args.importSpecifier, "./utils");
  });

  it("uses default project-root", () => {
    const args = parseArgs(["--from", "src/app.ts", "--import", "./utils"]);
    assertStrictEqual(args.projectRoot, process.cwd());
  });

  it("parses optional arguments", () => {
    const args = parseArgs([
      "--from",
      "src/app.ts",
      "--import",
      "./utils",
      "--project-root",
      "/custom/root",
      "--tsconfig",
      "custom-tsconfig.json",
      "--out",
      "output.json",
      "--pretty",
    ]);
    assertStrictEqual(args.from, "src/app.ts");
    assertStrictEqual(args.importSpecifier, "./utils");
    assertStrictEqual(args.projectRoot, "/custom/root");
    assertStrictEqual(args.tsconfig, "custom-tsconfig.json");
    assertStrictEqual(args.output, "output.json");
    assertStrictEqual(args.pretty, true);
  });

  it("accepts --import-specifier as alias for --import", () => {
    const args = parseArgs(["--from", "src/app.ts", "--import-specifier", "./utils"]);
    assertStrictEqual(args.importSpecifier, "./utils");
  });

  it("accepts --output as alias for --out", () => {
    const args = parseArgs([
      "--from",
      "src/app.ts",
      "--import",
      "./utils",
      "--output",
      "output.json",
    ]);
    assertStrictEqual(args.output, "output.json");
  });

  it("parses --help flag", () => {
    const args = parseArgs(["--help"]);
    assertStrictEqual(args.help, true);
  });

  it("parses -h as help flag", () => {
    const args = parseArgs(["-h"]);
    assertStrictEqual(args.help, true);
  });

  it("throws on unknown arguments", () => {
    assertThrows(
      () => parseArgs(["--from", "src/app.ts", "--unknown", "value"]),
      "unknown argument: --unknown"
    );
  });

  it("throws when argument missing value", () => {
    assertThrows(
      () => parseArgs(["--from"]),
      "missing value for --from"
    );
  });

  it("throws when --import missing value", () => {
    assertThrows(
      () => parseArgs(["--from", "src/app.ts", "--import"]),
      "missing value for --import"
    );
  });

  it("throws when argument value is another flag", () => {
    assertThrows(
      () => parseArgs(["--from", "--import", "./utils"]),
      "argument --from requires a value but got flag: --import"
    );
  });

  it("throws when value looks like a flag with equals", () => {
    assertThrows(
      () => parseArgs(["--from", "--import=./utils"]),
      "argument --from requires a value but got flag: --import=./utils"
    );
  });

  it("handles multiple flags in sequence", () => {
    const args = parseArgs([
      "--from", "src/app.ts",
      "--import", "./utils",
      "--project-root", "/root",
      "--tsconfig", "tsconfig.json",
      "--out", "out.json",
      "--pretty"
    ]);
    assertStrictEqual(args.from, "src/app.ts");
    assertStrictEqual(args.importSpecifier, "./utils");
    assertStrictEqual(args.projectRoot, "/root");
    assertStrictEqual(args.tsconfig, "tsconfig.json");
    assertStrictEqual(args.output, "out.json");
    assertStrictEqual(args.pretty, true);
  });

  it("handles empty argv", () => {
    const args = parseArgs([]);
    assertStrictEqual(args.from, undefined);
    assertStrictEqual(args.importSpecifier, undefined);
    assertStrictEqual(args.projectRoot, process.cwd());
    assertStrictEqual(args.pretty, false);
  });
});

// ============================================================================
// Snapshot Generation Tests
// ============================================================================

describe("generateResolutionSnapshot", () => {
  it("throws when missing --from argument", () => {
    assertThrows(
      () => generateResolutionSnapshot({ importSpecifier: "./utils" }),
      "missing required --from"
    );
  });

  it("throws when missing --import argument", () => {
    assertThrows(
      () => generateResolutionSnapshot({ from: "src/app.ts" }),
      "missing required --import"
    );
  });

  it("throws when --from file does not exist", () => {
    assertThrows(
      () =>
        generateResolutionSnapshot({
          from: "nonexistent.ts",
          importSpecifier: "./utils",
          projectRoot: process.cwd(),
        }),
      "--from file not found"
    );
  });

  it("throws when tsconfig cannot be found", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    fs.writeFileSync(path.join(tempDir, "app.ts"), "export const app = 1;");
    assertThrows(
      () =>
        generateResolutionSnapshot({
          from: "app.ts",
          importSpecifier: "./utils",
          projectRoot: tempDir,
        }),
      "could not find tsconfig.json"
    );
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  it("throws when explicit tsconfig does not exist", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    fs.writeFileSync(path.join(tempDir, "app.ts"), "export const app = 1;");
    assertThrows(
      () =>
        generateResolutionSnapshot({
          from: "app.ts",
          importSpecifier: "./utils",
          projectRoot: tempDir,
          tsconfig: "nonexistent.json",
        }),
      "tsconfig file not found"
    );
    fs.rmSync(tempDir, { recursive: true, force: true });
  });
});

// ============================================================================
// Integration Tests
// ============================================================================

describe("Integration: Happy Path", () => {
  it("generates complete resolution snapshot with valid inputs", () => {
    const testProjectRoot = path.join(__dirname, "..", "..");
    const testFile = path.join("src", "cli", "mod.rs");

    if (!fs.existsSync(path.join(testProjectRoot, testFile))) {
      process.stdout.write("    (skipped - test file not found)\n");
      return;
    }

    const tsconfigPath = path.join(testProjectRoot, "tsconfig.json");
    if (!fs.existsSync(tsconfigPath)) {
      process.stdout.write("    (skipped - tsconfig.json not found)\n");
      return;
    }

    const snapshot = generateResolutionSnapshot({
      from: testFile,
      importSpecifier: "clap",
      projectRoot: testProjectRoot,
      tsconfig: "tsconfig.json",
    });

    assertStrictEqual(snapshot.schema_version, "1");
    assertStrictEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_focus");
    assertStrictEqual(snapshot.producer, "specgate-npm-wrapper");
    assert(typeof snapshot.generated_at === "string");
    assertStrictEqual(snapshot.focus.from, testFile);
    assertStrictEqual(snapshot.focus.import_specifier, "clap");
    assert(Array.isArray(snapshot.resolutions));
    assert(Array.isArray(snapshot.edges));

    const resolution = snapshot.resolutions[0];
    assertStrictEqual(resolution.source, "tsc_compiler_api");
    assertStrictEqual(resolution.import, "clap");
    assertStrictEqual(resolution.result_kind, "third_party");
    assertStrictEqual(resolution.package_name, "clap");
    assert(Array.isArray(resolution.trace));
    assert(resolution.trace.length > 0);
  });

  it("generates snapshot for first-party import", () => {
    const testProjectRoot = path.join(__dirname);
    const testFile = path.join("src", "generate-resolution-snapshot.js");

    if (!fs.existsSync(path.join(testProjectRoot, testFile))) {
      process.stdout.write("    (skipped - test file not found)\n");
      return;
    }

    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");
    const tempModule = path.join(tempDir, "module.ts");

    fs.writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    fs.writeFileSync(tempFile, "import { mod } from './module';");
    fs.writeFileSync(tempModule, "export const mod = 1;");

    try {
      const snapshot = generateResolutionSnapshot({
        from: "test.ts",
        importSpecifier: "./module",
        projectRoot: tempDir,
      });

      assertStrictEqual(snapshot.focus.from, "test.ts");
      assertStrictEqual(snapshot.focus.import_specifier, "./module");

      const resolution = snapshot.resolutions[0];
      assertStrictEqual(resolution.result_kind, "first_party");
      assert(resolution.resolved_to.includes("module.ts"));
      assert(resolution.package_name === undefined);

      assertStrictEqual(snapshot.edges.length, 1);
      assertStrictEqual(snapshot.edges[0].from, "test.ts");
      assert(snapshot.edges[0].to.includes("module.ts"));
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("generates snapshot for builtin module import", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");

    fs.writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    fs.writeFileSync(tempFile, "import * as fs from 'fs';");

    try {
      const snapshot = generateResolutionSnapshot({
        from: "test.ts",
        importSpecifier: "fs",
        projectRoot: tempDir,
      });

      assertStrictEqual(snapshot.focus.from, "test.ts");
      assertStrictEqual(snapshot.focus.import_specifier, "fs");

      const resolution = snapshot.resolutions[0];
      assertStrictEqual(resolution.result_kind, "third_party");
      assertStrictEqual(resolution.package_name, "fs");
      assert(resolution.resolved_to === undefined || resolution.resolved_to === null);

      assertStrictEqual(snapshot.edges.length, 0);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("generates snapshot for third-party package import", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");
    const nodeModulesDir = path.join(tempDir, "node_modules", "test-pkg");

    fs.mkdirSync(nodeModulesDir, { recursive: true });
    fs.writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    fs.writeFileSync(tempFile, "import { foo } from 'test-pkg';");
    fs.writeFileSync(
      path.join(nodeModulesDir, "index.js"),
      "exports.foo = 'bar';"
    );
    fs.writeFileSync(
      path.join(nodeModulesDir, "package.json"),
      JSON.stringify({ name: "test-pkg", main: "index.js" })
    );

    try {
      const snapshot = generateResolutionSnapshot({
        from: "test.ts",
        importSpecifier: "test-pkg",
        projectRoot: tempDir,
      });

      assertStrictEqual(snapshot.focus.from, "test.ts");
      assertStrictEqual(snapshot.focus.import_specifier, "test-pkg");

      const resolution = snapshot.resolutions[0];
      assertStrictEqual(resolution.result_kind, "third_party");
      assertStrictEqual(resolution.package_name, "test-pkg");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("handles unresolvable imports gracefully", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");

    fs.writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    fs.writeFileSync(tempFile, "import { foo } from 'nonexistent-pkg-12345';");

    try {
      const snapshot = generateResolutionSnapshot({
        from: "test.ts",
        importSpecifier: "nonexistent-pkg-12345",
        projectRoot: tempDir,
      });

      assertStrictEqual(snapshot.focus.from, "test.ts");
      assertStrictEqual(snapshot.focus.import_specifier, "nonexistent-pkg-12345");

      const resolution = snapshot.resolutions[0];
      assertStrictEqual(resolution.result_kind, "unresolvable");
      // package_name is undefined (not null) for unresolvable imports
      assertStrictEqual(resolution.package_name, undefined);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("writes output to file when --out is specified", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");
    const outputFile = path.join(tempDir, "output.json");

    fs.writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    fs.writeFileSync(tempFile, "import * as fs from 'fs';");

    try {
      // Note: generateResolutionSnapshot doesn't write to file directly,
      // runCli handles that. This test verifies the snapshot structure.
      const snapshot = generateResolutionSnapshot({
        from: "test.ts",
        importSpecifier: "fs",
        projectRoot: tempDir,
      });

      // Verify snapshot can be serialized
      const json = JSON.stringify(snapshot, null, 2);
      assert(json.length > 0);
      assert(json.includes('"schema_version": "1"'));
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });
});

// ============================================================================
// Edge Case Tests
// ============================================================================

describe("Edge Cases", () => {
  it("handles paths with special characters", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const result = toProjectPath(projectRoot, "src/file-with-dashes_and_underscores.ts");
    assertStrictEqual(result.includes("\\"), false);
  });

  it("handles deeply nested paths", () => {
    const projectRoot = path.resolve(__dirname, "..");
    const deepPath = path.join(projectRoot, "a", "b", "c", "d", "e", "f", "file.ts");
    const result = toProjectPath(projectRoot, deepPath);
    assertStrictEqual(result, "a/b/c/d/e/f/file.ts");
  });

  it("handles package names with dots", () => {
    assertStrictEqual(extractPackageName("@types/node"), "@types/node");
    assertStrictEqual(extractPackageName("lodash.debounce"), "lodash.debounce");
  });

  it("handles package names with hyphens", () => {
    assertStrictEqual(extractPackageName("is-plain-object"), "is-plain-object");
  });

  it("classifies with null/undefined resolvedModule gracefully", () => {
    const resultNull = classifyResolution("lodash", null);
    assertStrictEqual(resultNull.resultKind, "unresolvable");

    const resultUndefined = classifyResolution("fs", undefined);
    assertStrictEqual(resultUndefined.resultKind, "third_party");
  });
});

// ============================================================================
// Workspace Discovery Tests
// ============================================================================

describe("discoverWorkspacePackages", () => {
  it("finds npm workspaces from package.json array workspaces field", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tempDir, "packages", "alpha"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "alpha", "package.json"),
        JSON.stringify({ name: "alpha" })
      );
      fs.writeFileSync(
        path.join(tempDir, "packages", "alpha", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const packages = discoverWorkspacePackages(tempDir);
      assert(Array.isArray(packages));
      assertStrictEqual(packages.length, 1);
      assertStrictEqual(packages[0].name, "alpha");
      assert(packages[0].dir.includes("alpha"));
      assertStrictEqual(typeof packages[0].tsconfigPath, "string");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("finds pnpm workspaces from pnpm-workspace.yaml", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - extensions/*\n"
      );
      fs.mkdirSync(path.join(tempDir, "packages", "web"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "web", "package.json"),
        JSON.stringify({ name: "web" })
      );
      fs.writeFileSync(
        path.join(tempDir, "packages", "web", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );
      fs.mkdirSync(path.join(tempDir, "extensions", "shared"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "extensions", "shared", "package.json"),
        JSON.stringify({ name: "shared" })
      );
      fs.writeFileSync(
        path.join(tempDir, "extensions", "shared", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const packages = discoverWorkspacePackages(tempDir);
      assertStrictEqual(packages.length, 2);
      const names = packages.map((p) => p.name);
      assert(names.includes("web"));
      assert(names.includes("shared"));
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("returns empty array for non-workspace project", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "simple-pkg", version: "1.0.0" })
      );

      const packages = discoverWorkspacePackages(tempDir);
      assert(Array.isArray(packages));
      assertStrictEqual(packages.length, 0);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("returns sorted packages by name (parity with Rust)", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n"
      );
      for (const name of ["zebra", "alpha", "mango"]) {
        fs.mkdirSync(path.join(tempDir, "packages", name), { recursive: true });
        fs.writeFileSync(
          path.join(tempDir, "packages", name, "package.json"),
          JSON.stringify({ name })
        );
        fs.writeFileSync(
          path.join(tempDir, "packages", name, "tsconfig.json"),
          JSON.stringify({ compilerOptions: {} })
        );
      }

      const packages = discoverWorkspacePackages(tempDir);
      const names = packages.map((p) => p.name);
      const sortedNames = [...names].sort();
      assertDeepStrictEqual(names, sortedNames);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("uses custom tsconfig filename when specified", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tempDir, "packages", "beta"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "beta", "package.json"),
        JSON.stringify({ name: "beta" })
      );
      // Only provide tsconfig.build.json, not tsconfig.json
      fs.writeFileSync(
        path.join(tempDir, "packages", "beta", "tsconfig.build.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const defaultPackages = discoverWorkspacePackages(tempDir, "tsconfig.json");
      const defaultBeta = defaultPackages.find((p) => p.name === "beta");
      assert(defaultBeta === undefined || defaultBeta.tsconfigPath === null,
        "should not find tsconfig.json when only tsconfig.build.json exists");

      const customPackages = discoverWorkspacePackages(tempDir, "tsconfig.build.json");
      const customBeta = customPackages.find((p) => p.name === "beta");
      assert(customBeta !== undefined, "should find beta package with custom filename");
      assert(customBeta.tsconfigPath !== null, "tsconfigPath should not be null");
      assert(customBeta.tsconfigPath.includes("tsconfig.build.json"));
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("skips package directories without package.json", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      // dir with package.json
      fs.mkdirSync(path.join(tempDir, "packages", "valid"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "valid", "package.json"),
        JSON.stringify({ name: "valid" })
      );
      // dir without package.json
      fs.mkdirSync(path.join(tempDir, "packages", "nopkg"), { recursive: true });
      fs.writeFileSync(path.join(tempDir, "packages", "nopkg", "index.js"), "");

      const packages = discoverWorkspacePackages(tempDir);
      assertStrictEqual(packages.length, 1);
      assertStrictEqual(packages[0].name, "valid");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("handles package.json workspaces.packages object form", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: { packages: ["packages/*"] } })
      );
      fs.mkdirSync(path.join(tempDir, "packages", "omega"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "omega", "package.json"),
        JSON.stringify({ name: "omega" })
      );

      const packages = discoverWorkspacePackages(tempDir);
      assertStrictEqual(packages.length, 1);
      assertStrictEqual(packages[0].name, "omega");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });
});

// ============================================================================
// Workspace Snapshot Generation Tests
// ============================================================================

describe("generateWorkspaceSnapshot", () => {
  it("produces batch output with correct schema shape", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tempDir, "packages", "gamma"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "gamma", "package.json"),
        JSON.stringify({ name: "gamma" })
      );
      fs.writeFileSync(
        path.join(tempDir, "packages", "gamma", "tsconfig.json"),
        JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
      );

      const snapshot = generateWorkspaceSnapshot(tempDir);

      assertStrictEqual(snapshot.schema_version, "1");
      assertStrictEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_batch");
      assertStrictEqual(snapshot.producer, "specgate-npm-wrapper");
      assertStrictEqual(typeof snapshot.generated_at, "string");
      assertStrictEqual(typeof snapshot.project_root, "string");
      assert(Array.isArray(snapshot.packages));
      assertStrictEqual(snapshot.packages.length, 1);
      assertStrictEqual(snapshot.packages[0].name, "gamma");
      assertStrictEqual(typeof snapshot.packages[0].dir, "string");
      assertStrictEqual(typeof snapshot.packages[0].tsconfig_path, "string");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("returns empty packages array for non-workspace project", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "simple" })
      );

      const snapshot = generateWorkspaceSnapshot(tempDir);
      assertStrictEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_batch");
      assertStrictEqual(snapshot.packages.length, 0);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("excludes packages without tsconfig from batch packages list", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      // Package with tsconfig
      fs.mkdirSync(path.join(tempDir, "packages", "has-ts"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "has-ts", "package.json"),
        JSON.stringify({ name: "has-ts" })
      );
      fs.writeFileSync(
        path.join(tempDir, "packages", "has-ts", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );
      // Package without tsconfig
      fs.mkdirSync(path.join(tempDir, "packages", "no-ts"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "no-ts", "package.json"),
        JSON.stringify({ name: "no-ts" })
      );

      const snapshot = generateWorkspaceSnapshot(tempDir);
      assertStrictEqual(snapshot.packages.length, 1);
      assertStrictEqual(snapshot.packages[0].name, "has-ts");
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });

  it("uses custom tsconfig-filename option", () => {
    const tempDir = fs.mkdtempSync("/tmp/specgate-test-");
    try {
      fs.writeFileSync(
        path.join(tempDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tempDir, "packages", "delta"), { recursive: true });
      fs.writeFileSync(
        path.join(tempDir, "packages", "delta", "package.json"),
        JSON.stringify({ name: "delta" })
      );
      fs.writeFileSync(
        path.join(tempDir, "packages", "delta", "tsconfig.build.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const snapshot = generateWorkspaceSnapshot(tempDir, { tsconfigFilename: "tsconfig.build.json" });
      assertStrictEqual(snapshot.packages.length, 1);
      assertStrictEqual(snapshot.packages[0].name, "delta");
      assert(snapshot.packages[0].tsconfig_path.includes("tsconfig.build.json"));
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  });
});

// ============================================================================
// parseArgs Workspace Flag Tests
// ============================================================================

describe("parseArgs --workspace flags", () => {
  it("parses --workspace boolean flag", () => {
    const args = parseArgs(["--workspace"]);
    assertStrictEqual(args.workspace, true);
  });

  it("parses --tsconfig-filename flag", () => {
    const args = parseArgs(["--workspace", "--tsconfig-filename", "tsconfig.build.json"]);
    assertStrictEqual(args.tsconfigFilename, "tsconfig.build.json");
  });

  it("defaults workspace to false when not provided", () => {
    const args = parseArgs(["--from", "src/app.ts", "--import", "./utils"]);
    assertStrictEqual(args.workspace, false);
  });

  it("defaults tsconfigFilename to tsconfig.json", () => {
    const args = parseArgs(["--workspace"]);
    assertStrictEqual(args.tsconfigFilename, "tsconfig.json");
  });
});

process.stdout.write("\n✅ All tests passed!\n");
