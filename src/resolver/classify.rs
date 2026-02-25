use std::path::{Component, Path};

use super::ResolvedImport;

/// Node.js built-in module names.
///
/// This includes current stable builtins plus long-standing internal aliases
/// that may appear in dependency graphs.
const NODE_BUILTINS: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
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
    "sea",
    "sqlite",
    "stream",
    "string_decoder",
    "sys",
    "test",
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
    if is_explicit_node_builtin(specifier) {
        return ResolvedImport::ThirdParty {
            package_name: extract_package_name(specifier).to_string(),
        };
    }

    if is_in_node_modules(resolved_path) {
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
///
/// Accepts both bare (`fs`) and explicit `node:` (`node:fs/promises`) forms.
pub fn is_node_builtin(specifier: &str) -> bool {
    let name = specifier.strip_prefix("node:").unwrap_or(specifier);
    is_node_builtin_name(name)
}

/// Check whether the specifier explicitly references a builtin with `node:` prefix.
pub fn is_explicit_node_builtin(specifier: &str) -> bool {
    specifier
        .strip_prefix("node:")
        .is_some_and(is_node_builtin_name)
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

fn is_node_builtin_name(name: &str) -> bool {
    let base = name.split('/').next().unwrap_or(name);
    NODE_BUILTINS.contains(&base)
}

fn is_in_node_modules(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => name == "node_modules",
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn detects_node_builtin_with_prefix() {
        assert!(is_node_builtin("node:fs/promises"));
        assert!(is_node_builtin("path"));
        assert!(is_node_builtin("node:test/reporters"));
        assert!(!is_node_builtin("@app/orders"));
    }

    #[test]
    fn detects_explicit_node_builtin() {
        assert!(is_explicit_node_builtin("node:fs/promises"));
        assert!(!is_explicit_node_builtin("fs"));
        assert!(!is_explicit_node_builtin("node:not-a-builtin"));
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

    #[test]
    fn bare_builtin_name_does_not_override_concrete_first_party_resolution() {
        let root = Path::new("/repo");
        let resolved = Path::new("/repo/src/path.ts");
        let kind = classify_resolution(root, resolved, "path");
        assert!(matches!(kind, ResolvedImport::FirstParty { .. }));
    }

    #[test]
    fn classifies_third_party_when_path_has_node_modules_component() {
        let root = Path::new("/repo");
        let resolved = Path::new("/repo/node_modules/lodash/index.js");
        let kind = classify_resolution(root, resolved, "lodash");
        assert!(matches!(kind, ResolvedImport::ThirdParty { .. }));
    }

    #[test]
    fn does_not_treat_partial_node_modules_name_as_component_match() {
        let root = Path::new("/repo");
        let resolved = Path::new("/repo/src/node_modules-utils/index.ts");
        let kind = classify_resolution(root, resolved, "./node_modules-utils/index");
        assert!(matches!(kind, ResolvedImport::FirstParty { .. }));
    }
}
