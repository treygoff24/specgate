use std::path::Path;

use super::ResolvedImport;

/// Node.js built-in module names.
const NODE_BUILTINS: &[&str] = &[
    "assert",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// Classify a resolved path as first-party, third-party, or unresolvable.
pub fn classify_resolution(
    project_root: &Path,
    resolved_path: &Path,
    specifier: &str,
) -> ResolvedImport {
    if is_node_builtin(specifier) {
        return ResolvedImport::ThirdParty {
            package_name: extract_package_name(specifier).to_string(),
        };
    }

    let node_modules_segment = format!(
        "{}node_modules{}",
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    let resolved_path_str = resolved_path.to_string_lossy();
    if resolved_path_str.contains(&node_modules_segment)
        || resolved_path_str.ends_with(&format!("{}node_modules", std::path::MAIN_SEPARATOR))
    {
        return ResolvedImport::ThirdParty {
            package_name: extract_package_name(specifier).to_string(),
        };
    }

    if resolved_path.starts_with(project_root) {
        return ResolvedImport::FirstParty {
            resolved_path: resolved_path.to_path_buf(),
            module_id: None,
        };
    }

    ResolvedImport::Unresolvable {
        specifier: specifier.to_string(),
        reason: "resolved path is outside project root".to_string(),
    }
}

/// Check whether the specifier references a Node.js builtin module.
pub fn is_node_builtin(specifier: &str) -> bool {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    let base = name.split('/').next().unwrap_or(name);
    NODE_BUILTINS.contains(&base)
}

/// Extract npm package name from a bare package specifier.
///
/// - `@scope/pkg/sub/path` -> `@scope/pkg`
/// - `lodash/fp` -> `lodash`
pub fn extract_package_name(specifier: &str) -> &str {
    let without_node_prefix = specifier.strip_prefix("node:").unwrap_or(specifier);

    if without_node_prefix.starts_with('@') {
        match without_node_prefix.match_indices('/').nth(1) {
            Some((idx, _)) => &without_node_prefix[..idx],
            None => without_node_prefix,
        }
    } else {
        without_node_prefix
            .split('/')
            .next()
            .unwrap_or(without_node_prefix)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn detects_node_builtin_with_prefix() {
        assert!(is_node_builtin("node:fs/promises"));
        assert!(is_node_builtin("path"));
        assert!(!is_node_builtin("@app/orders"));
    }

    #[test]
    fn extracts_package_names() {
        assert_eq!(extract_package_name("@scope/pkg/path"), "@scope/pkg");
        assert_eq!(extract_package_name("lodash/fp"), "lodash");
        assert_eq!(extract_package_name("node:fs/promises"), "fs");
    }

    #[test]
    fn classifies_first_party_when_inside_project() {
        let root = Path::new("/repo");
        let resolved = Path::new("/repo/src/index.ts");
        let kind = classify_resolution(root, resolved, "./index");
        assert!(matches!(kind, ResolvedImport::FirstParty { .. }));
    }
}
