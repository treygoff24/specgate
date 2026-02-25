use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Normalize a path to forward-slash form for deterministic output.
pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Normalize `path` relative to `project_root` when possible.
/// Falls back to normalized absolute/opaque representation when not relative.
pub fn normalize_repo_relative(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(relative) => normalize_path(relative),
        Err(_) => normalize_path(path),
    }
}

/// Return a sorted + deduplicated vector using lexical order.
pub fn stable_unique<I>(items: I) -> Vec<String>
where
    I: IntoIterator,
    I::Item: Into<String>,
{
    let mut set = BTreeSet::new();
    for item in items {
        set.insert(item.into());
    }
    set.into_iter().collect()
}

/// Build a deterministic SHA-256 hash for arbitrary bytes.
pub fn stable_hash_hex(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes.as_ref());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    hex
}

/// Hash a set of normalized fields in stable order.
pub fn stable_fingerprint(parts: &[impl AsRef<str>]) -> String {
    let mut joined = String::new();
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            joined.push('|');
        }
        joined.push_str(part.as_ref());
    }
    format!("sha256:{}", stable_hash_hex(joined.as_bytes()))
}

/// Normalize and sort paths deterministically.
pub fn normalize_and_sort_paths(paths: &[PathBuf]) -> Vec<String> {
    let mut normalized: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
    normalized.sort();
    normalized
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn path_normalization_uses_forward_slashes() {
        let path = Path::new("foo\\bar\\baz.ts");
        assert_eq!(normalize_path(path), "foo/bar/baz.ts");
    }

    #[test]
    fn stable_unique_is_sorted_and_deduped() {
        let actual = stable_unique(vec!["z", "a", "a", "m"]);
        assert_eq!(actual, vec!["a", "m", "z"]);
    }

    #[test]
    fn stable_hash_is_deterministic() {
        let left = stable_hash_hex("specgate");
        let right = stable_hash_hex("specgate");
        assert_eq!(left, right);
        assert_eq!(
            left,
            "255a7328ba24b34e581428c9f423c7beead525d675fcf2856fe251e43af69405"
        );
    }

    #[test]
    fn stable_fingerprint_is_deterministic() {
        let parts = ["module", "rule", "severity"];
        let a = stable_fingerprint(&parts);
        let b = stable_fingerprint(&parts);
        assert_eq!(a, b);
        assert_eq!(
            a,
            "sha256:95dece5be5240bcef7b92e9cdee2ba553ebe1269c41664008d938af9353fab9f"
        );
    }
}
