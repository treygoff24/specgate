"use strict";

const assert = require("node:assert");
const path = require("node:path");
const {
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  extractPackageName,
  generateResolutionSnapshot,
} = require("./generate-resolution-snapshot.js");

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

describe("isBuiltinImport", () => {
  it("returns true for core Node.js modules", () => {
    assertStrictEqual(isBuiltinImport("fs"), true);
    assertStrictEqual(isBuiltinImport("path"), true);
    assertStrictEqual(isBuiltinImport("os"), true);
    assertStrictEqual(isBuiltinImport("http"), true);
    assertStrictEqual(isBuiltinImport("https"), true);
  });

  it("returns true for node: prefixed modules", () => {
    assertStrictEqual(isBuiltinImport("node:fs"), true);
    assertStrictEqual(isBuiltinImport("node:path"), true);
    assertStrictEqual(isBuiltinImport("node:os"), true);
  });

  it("returns false for non-builtin modules", () => {
    assertStrictEqual(isBuiltinImport("lodash"), false);
    assertStrictEqual(isBuiltinImport("express"), false);
    assertStrictEqual(isBuiltinImport("./local"), false);
    assertStrictEqual(isBuiltinImport("../parent"), false);
    assertStrictEqual(isBuiltinImport("@scope/package"), false);
  });
});

describe("extractPackageName", () => {
  it("extracts package name from simple imports", () => {
    assertStrictEqual(extractPackageName("lodash"), "lodash");
    assertStrictEqual(extractPackageName("express"), "express");
    assertStrictEqual(extractPackageName("axios"), "axios");
  });

  it("extracts package name from scoped imports", () => {
    assertStrictEqual(extractPackageName("@babel/core"), "@babel/core");
    assertStrictEqual(extractPackageName("@types/node"), "@types/node");
    assertStrictEqual(extractPackageName("@scope/package/subpath"), "@scope/package");
  });

  it("extracts package name from node: prefixed imports", () => {
    assertStrictEqual(extractPackageName("node:fs"), "fs");
    assertStrictEqual(extractPackageName("node:path"), "path");
  });

  it("returns null for relative imports", () => {
    assertStrictEqual(extractPackageName("./local"), null);
    assertStrictEqual(extractPackageName("../parent"), null);
    assertStrictEqual(extractPackageName("../../grandparent"), null);
  });

  it("returns null for absolute paths", () => {
    assertStrictEqual(extractPackageName("/absolute/path"), null);
    if (path.sep === "\\") {
      assertStrictEqual(extractPackageName("C:\\absolute\\path"), null);
    }
  });

  it("returns null for empty string", () => {
    assertStrictEqual(extractPackageName(""), null);
  });
});

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
});

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
});

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

  it("throws when argument value is another flag", () => {
    assertThrows(
      () => parseArgs(["--from", "--import", "./utils"]),
      "argument --from requires a value but got flag: --import"
    );
  });
});

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
    const tempDir = require("node:fs").mkdtempSync("/tmp/specgate-test-");
    require("node:fs").writeFileSync(path.join(tempDir, "app.ts"), "export const app = 1;");
    assertThrows(
      () =>
        generateResolutionSnapshot({
          from: "app.ts",
          importSpecifier: "./utils",
          projectRoot: tempDir,
        }),
      "could not find tsconfig.json"
    );
    require("node:fs").rmSync(tempDir, { recursive: true, force: true });
  });
});

describe("Integration: Happy Path", () => {
  it("generates complete resolution snapshot with valid inputs", () => {
    const testProjectRoot = path.join(__dirname, "..", "..");
    const testFile = path.join("src", "cli", "mod.rs");

    if (!require("node:fs").existsSync(path.join(testProjectRoot, testFile))) {
      process.stdout.write("    (skipped - test file not found)\n");
      return;
    }

    const tsconfigPath = path.join(testProjectRoot, "tsconfig.json");
    if (!require("node:fs").existsSync(tsconfigPath)) {
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

    if (!require("node:fs").existsSync(path.join(testProjectRoot, testFile))) {
      process.stdout.write("    (skipped - test file not found)\n");
      return;
    }

    const tempDir = require("node:fs").mkdtempSync("/tmp/specgate-test-");
    const tempTsconfig = path.join(tempDir, "tsconfig.json");
    const tempFile = path.join(tempDir, "test.ts");
    const tempModule = path.join(tempDir, "module.ts");

    require("node:fs").writeFileSync(
      tempTsconfig,
      JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
    );
    require("node:fs").writeFileSync(tempFile, "import { mod } from './module';");
    require("node:fs").writeFileSync(tempModule, "export const mod = 1;");

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
      require("node:fs").rmSync(tempDir, { recursive: true, force: true });
    }
  });
});

process.stdout.write("\nAll tests passed!\n");
