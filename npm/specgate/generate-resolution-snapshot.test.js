"use strict";

const {
  classifyResolution,
  toProjectPath,
  parseArgs,
  isBuiltinImport,
  generateResolutionSnapshot,
  discoverWorkspacePackages,
  generateWorkspaceSnapshot,
  expandWorkspaceGlob,
  wrapperVersion,
} = require("./src/generate-resolution-snapshot");

const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

function describe(name, fn) {
  console.log(`\n${name}`);
  fn();
}

function it(name, fn) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    console.log(`  ✗ ${name}`);
    console.log(`    ${error.message}`);
    process.exitCode = 1;
  }
}

function assertEqual(actual, expected, message) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `${message || "assertion failed"}\n    expected: ${JSON.stringify(expected)}\n    actual: ${JSON.stringify(actual)}`
    );
  }
}

function assertTrue(value, message) {
  if (!value) {
    throw new Error(message || "expected true");
  }
}

function assertFalse(value, message) {
  if (value) {
    throw new Error(message || "expected false");
  }
}

describe("isBuiltinImport", () => {
  it("returns true for node:fs", () => {
    assertTrue(isBuiltinImport("node:fs"));
  });

  it("returns true for fs", () => {
    assertTrue(isBuiltinImport("fs"));
  });

  it("returns true for node:path", () => {
    assertTrue(isBuiltinImport("node:path"));
  });

  it("returns true for path", () => {
    assertTrue(isBuiltinImport("path"));
  });

  it("returns false for relative imports", () => {
    assertFalse(isBuiltinImport("./utils"));
  });

  it("returns false for package imports", () => {
    assertFalse(isBuiltinImport("lodash"));
  });

  it("returns false for scoped packages", () => {
    assertFalse(isBuiltinImport("@org/package"));
  });
});

describe("classifyResolution", () => {
  it("classifies unresolvable imports", () => {
    const result = classifyResolution("./nonexistent", null);
    assertEqual(result.resultKind, "unresolvable");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, null);
  });

  it("classifies builtin imports as third_party when no resolved module", () => {
    const result = classifyResolution("fs", null);
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, "fs");
  });

  it("classifies node: prefixed builtin as third_party when no resolved module", () => {
    const result = classifyResolution("node:path", null);
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.resolvedTo, null);
    assertEqual(result.packageName, "path");
  });

  it("classifies external library imports as third_party", () => {
    const result = classifyResolution(
      "lodash",
      {
        resolvedFileName: "/node_modules/lodash/index.js",
        isExternalLibraryImport: true,
      }
    );
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.packageName, "lodash");
  });

  it("classifies first_party imports", () => {
    const result = classifyResolution(
      "./utils",
      {
        resolvedFileName: "/src/utils.ts",
        isExternalLibraryImport: false,
      }
    );
    assertEqual(result.resultKind, "first_party");
    assertEqual(result.resolvedTo, "/src/utils.ts");
    assertEqual(result.packageName, null);
  });

  it("classifies node_modules path as third_party even without isExternalLibraryImport", () => {
    const result = classifyResolution(
      "lodash",
      {
        resolvedFileName: "/project/node_modules/lodash/index.js",
        isExternalLibraryImport: false,
      }
    );
    assertEqual(result.resultKind, "third_party");
    assertEqual(result.packageName, "lodash");
  });
});

describe("toProjectPath", () => {
  it("returns relative path for files inside project", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      const result = toProjectPath(tmpDir, path.join(tmpDir, "src", "utils.ts"));
      assertEqual(result, "src/utils.ts");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns . for project root", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      const result = toProjectPath(tmpDir, tmpDir);
      assertEqual(result, ".");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns absolute path for files outside project", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    const outsideFile = path.join(os.tmpdir(), "outside.ts");
    try {
      fs.writeFileSync(outsideFile, "");
      const result = toProjectPath(tmpDir, outsideFile);
      assertTrue(result.startsWith("/"));
      assertTrue(result.includes("outside.ts"));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
      try {
        fs.unlinkSync(outsideFile);
      } catch {
        // ignore
      }
    }
  });

  it("normalizes path separators to forward slashes", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      const result = toProjectPath(tmpDir, path.join(tmpDir, "src", "utils.ts"));
      assertTrue(!result.includes("\\"), `path should not contain backslashes: ${result}`);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

describe("parseArgs", () => {
  it("parses required arguments", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import", "lodash"]);
    assertEqual(args.from, "src/main.ts");
    assertEqual(args.importSpecifier, "lodash");
  });

  it("parses optional arguments", () => {
    const args = parseArgs([
      "--from",
      "src/main.ts",
      "--import",
      "lodash",
      "--project-root",
      "/project",
      "--tsconfig",
      "tsconfig.json",
      "--out",
      "output.json",
      "--pretty",
    ]);
    assertEqual(args.from, "src/main.ts");
    assertEqual(args.importSpecifier, "lodash");
    assertEqual(args.projectRoot, "/project");
    assertEqual(args.tsconfig, "tsconfig.json");
    assertEqual(args.output, "output.json");
    assertTrue(args.pretty);
  });

  it("parses --import-specifier alias", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import-specifier", "lodash"]);
    assertEqual(args.importSpecifier, "lodash");
  });

  it("parses --output alias", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import", "lodash", "--output", "output.json"]);
    assertEqual(args.output, "output.json");
  });

  it("throws on missing value", () => {
    let error;
    try {
      parseArgs(["--from"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing value"));
  });

  it("throws on flag as value", () => {
    let error;
    try {
      parseArgs(["--from", "--import", "lodash"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("requires a value but got flag"));
  });

  it("throws on unknown argument", () => {
    let error;
    try {
      parseArgs(["--from", "src/main.ts", "--import", "lodash", "--unknown", "value"]);
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("unknown argument"));
  });

  it("parses --help flag", () => {
    const args = parseArgs(["--help"]);
    assertTrue(args.help);
  });

  it("parses -h flag", () => {
    const args = parseArgs(["-h"]);
    assertTrue(args.help);
  });
});

describe("module exports", () => {
  it("exports wrapperVersion matching package.json", () => {
    assertEqual(wrapperVersion, require("./package.json").version);
  });
});

describe("generateResolutionSnapshot", () => {
  it("throws on missing --from", () => {
    let error;
    try {
      generateResolutionSnapshot({ importSpecifier: "lodash" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing required --from"));
  });

  it("throws on missing --import", () => {
    let error;
    try {
      generateResolutionSnapshot({ from: "src/main.ts" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("missing required --import"));
  });

  it("throws on non-existent --from file", () => {
    let error;
    try {
      generateResolutionSnapshot({ from: "nonexistent.ts", importSpecifier: "lodash" });
    } catch (e) {
      error = e;
    }
    assertTrue(error && error.message.includes("--from file not found"));
  });

  it("generates snapshot for builtin import", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    const testFile = path.join(tmpDir, "test.ts");
    const tsconfigPath = path.join(tmpDir, "tsconfig.json");

    try {
      fs.writeFileSync(testFile, 'import "fs";');
      fs.writeFileSync(
        tsconfigPath,
        JSON.stringify({
          compilerOptions: {
            module: "NodeNext",
            moduleResolution: "NodeNext",
          },
        })
      );

      const snapshot = generateResolutionSnapshot({
        from: testFile,
        importSpecifier: "fs",
        projectRoot: tmpDir,
        tsconfig: tsconfigPath,
      });

      assertEqual(snapshot.schema_version, "1");
      assertEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_focus");
      assertEqual(snapshot.focus.from, "test.ts");
      assertEqual(snapshot.focus.import_specifier, "fs");
      assertTrue(snapshot.resolutions.length > 0);
      assertEqual(snapshot.resolutions[0].import, "fs");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("generates snapshot for relative import", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    const testFile = path.join(tmpDir, "src", "test.ts");
    const targetFile = path.join(tmpDir, "src", "utils.ts");
    const tsconfigPath = path.join(tmpDir, "tsconfig.json");

    try {
      fs.mkdirSync(path.dirname(testFile), { recursive: true });
      fs.writeFileSync(testFile, 'import "./utils";');
      fs.writeFileSync(targetFile, "export const x = 1;");
      fs.writeFileSync(
        tsconfigPath,
        JSON.stringify({
          compilerOptions: {
            module: "NodeNext",
            moduleResolution: "NodeNext",
          },
        })
      );

      const snapshot = generateResolutionSnapshot({
        from: testFile,
        importSpecifier: "./utils",
        projectRoot: tmpDir,
        tsconfig: tsconfigPath,
      });

      assertEqual(snapshot.schema_version, "1");
      assertEqual(snapshot.focus.from, "src/test.ts");
      assertEqual(snapshot.focus.import_specifier, "./utils");
      assertEqual(snapshot.resolutions[0].result_kind, "first_party");
      assertEqual(snapshot.resolutions[0].resolved_to, "src/utils.ts");
      assertTrue(snapshot.edges.length > 0);
      assertEqual(snapshot.edges[0].from, "src/test.ts");
      assertEqual(snapshot.edges[0].to, "src/utils.ts");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("includes wrapper_version in focused snapshot", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    const testFile = path.join(tmpDir, "test.ts");
    const tsconfigPath = path.join(tmpDir, "tsconfig.json");

    try {
      fs.writeFileSync(testFile, 'import "fs";');
      fs.writeFileSync(
        tsconfigPath,
        JSON.stringify({
          compilerOptions: {
            module: "NodeNext",
            moduleResolution: "NodeNext",
          },
        })
      );

      const snapshot = generateResolutionSnapshot({
        from: testFile,
        importSpecifier: "fs",
        projectRoot: tmpDir,
        tsconfig: tsconfigPath,
      });

      assertTrue(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(snapshot.wrapper_version));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ============================================================================
// expandWorkspaceGlob Tests
// ============================================================================

describe("expandWorkspaceGlob", () => {
  it("returns matching dirs for a simple packages/* pattern", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "packages", "alpha"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "packages", "beta"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "packages/*");
      const names = results.map((r) => path.basename(r)).sort();
      assertEqual(JSON.stringify(names), JSON.stringify(["alpha", "beta"]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("filters to suffix-matching dirs for packages/*-web pattern", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "packages", "admin-web"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "packages", "admin-api"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "packages", "public-web"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "packages", "cli"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "packages/*-web");
      const names = results.map((r) => path.basename(r)).sort();
      assertEqual(JSON.stringify(names), JSON.stringify(["admin-web", "public-web"]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns nested subpath dirs for apps/*/pkg pattern", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "apps", "frontend", "pkg"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "apps", "backend", "pkg"), { recursive: true });
      // This one is missing the pkg subdir — should be excluded
      fs.mkdirSync(path.join(tmpDir, "apps", "nopkg"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "apps/*/pkg");
      const parents = results.map((r) => path.basename(path.dirname(r))).sort();
      assertEqual(JSON.stringify(parents), JSON.stringify(["backend", "frontend"]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns literal path when pattern has no wildcard", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "shared"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "shared");
      assertTrue(results.length === 1, "should find exactly one directory");
      assertTrue(results[0].endsWith("shared"), "result should end with 'shared'");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns empty array when prefix directory does not exist", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      const results = expandWorkspaceGlob(tmpDir, "nonexistent/*");
      assertEqual(JSON.stringify(results), JSON.stringify([]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns empty array for double-star with suffix pattern (unsupported)", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "packages", "admin-web"), { recursive: true });
      fs.mkdirSync(path.join(tmpDir, "packages", "public-web"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "packages/**-web");
      assertEqual(JSON.stringify(results), JSON.stringify([]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns empty array for double-star with subPath pattern (unsupported)", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.mkdirSync(path.join(tmpDir, "apps", "frontend", "pkg"), { recursive: true });

      const results = expandWorkspaceGlob(tmpDir, "apps/**/pkg");
      assertEqual(JSON.stringify(results), JSON.stringify([]));
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

// ============================================================================
// Workspace Discovery Tests
// ============================================================================

describe("discoverWorkspacePackages", () => {
  it("finds npm workspaces from package.json", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "alpha"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "alpha", "package.json"),
        JSON.stringify({ name: "alpha" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "alpha", "tsconfig.json"),
        JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
      );

      const packages = discoverWorkspacePackages(tmpDir);
      assertTrue(Array.isArray(packages), "should return an array");
      assertTrue(packages.length >= 1, "should find at least one package");
      const alpha = packages.find((p) => p.name === "alpha");
      assertTrue(alpha !== undefined, "should find alpha package");
      assertTrue(alpha.dir.includes("alpha"), "dir should contain alpha");
      assertTrue(typeof alpha.tsconfigPath === "string", "tsconfigPath should be a string");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("finds pnpm workspaces from pnpm-workspace.yaml", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - extensions/*\n"
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "web"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "web", "package.json"),
        JSON.stringify({ name: "web" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "web", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );
      fs.mkdirSync(path.join(tmpDir, "extensions", "shared"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "extensions", "shared", "package.json"),
        JSON.stringify({ name: "shared" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "extensions", "shared", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const packages = discoverWorkspacePackages(tmpDir);
      assertTrue(Array.isArray(packages), "should return an array");
      assertEqual(packages.length, 2);

      const names = packages.map((p) => p.name).sort();
      assertEqual(names[0], "shared");
      assertEqual(names[1], "web");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns empty array for non-workspace project", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "simple-pkg" })
      );

      const packages = discoverWorkspacePackages(tmpDir);
      assertTrue(Array.isArray(packages), "should return an array");
      assertEqual(packages.length, 0);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("matches Rust parity: same packages for identical input", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n"
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "core"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core", "package.json"),
        JSON.stringify({ name: "core" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "app"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "app", "package.json"),
        JSON.stringify({ name: "app" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "app", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const packages = discoverWorkspacePackages(tmpDir);
      // Rust parity: sorted by name
      const names = packages.map((p) => p.name);
      const sortedNames = [...names].sort();
      assertEqual(JSON.stringify(names), JSON.stringify(sortedNames), "packages should be sorted by name");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("respects --tsconfig-filename override", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "beta"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "beta", "package.json"),
        JSON.stringify({ name: "beta" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "beta", "tsconfig.build.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      // With default filename, no tsconfig found
      const defaultPackages = discoverWorkspacePackages(tmpDir, "tsconfig.json");
      const defaultBeta = defaultPackages.find((p) => p.name === "beta");
      assertTrue(defaultBeta === undefined || defaultBeta.tsconfigPath === null, "should not find tsconfig.json");

      // With custom filename, tsconfig found
      const customPackages = discoverWorkspacePackages(tmpDir, "tsconfig.build.json");
      const customBeta = customPackages.find((p) => p.name === "beta");
      assertTrue(customBeta !== undefined, "should find beta package");
      assertTrue(customBeta.tsconfigPath !== null, "should find tsconfig.build.json");
      assertTrue(customBeta.tsconfigPath.includes("tsconfig.build.json"), "tsconfigPath should point to custom file");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("discovers only suffix-matching packages from pnpm-workspace.yaml with suffix glob", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "pnpm-workspace.yaml"),
        "packages:\n  - \"packages/*-lib\"\n"
      );

      // core-lib should match the *-lib pattern
      fs.mkdirSync(path.join(tmpDir, "packages", "core-lib"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core-lib", "package.json"),
        JSON.stringify({ name: "core-lib" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core-lib", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      // core-app should NOT match the *-lib pattern
      fs.mkdirSync(path.join(tmpDir, "packages", "core-app"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core-app", "package.json"),
        JSON.stringify({ name: "core-app" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "core-app", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      // utils-lib should match the *-lib pattern
      fs.mkdirSync(path.join(tmpDir, "packages", "utils-lib"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "utils-lib", "package.json"),
        JSON.stringify({ name: "utils-lib" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "utils-lib", "tsconfig.json"),
        JSON.stringify({ compilerOptions: {} })
      );

      const packages = discoverWorkspacePackages(tmpDir);
      const names = packages.map((p) => p.name).sort();
      assertEqual(JSON.stringify(names), JSON.stringify(["core-lib", "utils-lib"]));
      assertFalse(names.includes("core-app"), "core-app should not be discovered");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

describe("generateWorkspaceSnapshot", () => {
  it("produces batch output with correct schema shape", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "gamma"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "gamma", "package.json"),
        JSON.stringify({ name: "gamma" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "gamma", "tsconfig.json"),
        JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
      );

      const snapshot = generateWorkspaceSnapshot(tmpDir);

      assertEqual(snapshot.schema_version, "1");
      assertEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_batch");
      assertEqual(snapshot.producer, "specgate-npm-wrapper");
      assertTrue(typeof snapshot.generated_at === "string", "generated_at should be a string");
      assertTrue(typeof snapshot.project_root === "string", "project_root should be a string");
      assertTrue(Array.isArray(snapshot.packages), "packages should be an array");
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("returns empty packages array for non-workspace project", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "simple" })
      );

      const snapshot = generateWorkspaceSnapshot(tmpDir);
      assertEqual(snapshot.snapshot_kind, "doctor_compare_tsc_resolution_batch");
      assertEqual(snapshot.packages.length, 0);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  it("includes wrapper_version in workspace snapshot", () => {
    const tmpDir = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "specgate-test-")));
    try {
      fs.writeFileSync(
        path.join(tmpDir, "package.json"),
        JSON.stringify({ name: "root", workspaces: ["packages/*"] })
      );
      fs.mkdirSync(path.join(tmpDir, "packages", "gamma"), { recursive: true });
      fs.writeFileSync(
        path.join(tmpDir, "packages", "gamma", "package.json"),
        JSON.stringify({ name: "gamma" })
      );
      fs.writeFileSync(
        path.join(tmpDir, "packages", "gamma", "tsconfig.json"),
        JSON.stringify({ compilerOptions: { moduleResolution: "node" } })
      );

      const snapshot = generateWorkspaceSnapshot(tmpDir);
      assertTrue(!!snapshot.wrapper_version);
      assertEqual(snapshot.wrapper_version, wrapperVersion);
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });
});

describe("parseArgs workspace flags", () => {
  it("parses --workspace flag", () => {
    const args = parseArgs(["--workspace"]);
    assertTrue(args.workspace === true, "--workspace should be true");
  });

  it("parses --tsconfig-filename flag", () => {
    const args = parseArgs(["--workspace", "--tsconfig-filename", "tsconfig.build.json"]);
    assertEqual(args.tsconfigFilename, "tsconfig.build.json");
  });

  it("does not set workspace by default", () => {
    const args = parseArgs(["--from", "src/main.ts", "--import", "lodash"]);
    assertTrue(!args.workspace, "--workspace should be falsy by default");
  });
});

console.log("\n");
