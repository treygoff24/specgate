use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::classify::specs_semantically_equivalent_for_rename;
use super::types::PolicyDiffErrorEntry;
use crate::spec::SpecFile;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveredSpecFileChanges {
    pub changed_spec_paths: BTreeSet<String>,
    pub fail_closed_operations: Vec<FailClosedSpecOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailClosedSpecOperation {
    Deletion {
        path: String,
    },
    RenameOrCopy {
        status: String,
        from_path: String,
        to_path: String,
        semantic_pairing: RenameCopySemanticPairing,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenameCopySemanticPairing {
    #[default]
    Unassessed,
    Equivalent,
    Different,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyGitError {
    code: &'static str,
    message: String,
}

impl PolicyGitError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PolicyGitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for PolicyGitError {}

#[derive(Debug, Clone, Default)]
pub struct LoadedSpecSnapshots {
    pub snapshots: Vec<SpecSnapshotPair>,
    pub errors: Vec<PolicyDiffErrorEntry>,
}

#[derive(Debug, Clone)]
pub struct SpecSnapshotPair {
    pub spec_path: String,
    pub base_spec: Option<SpecFile>,
    pub head_spec: Option<SpecFile>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredAndLoadedSpecSnapshots {
    pub discovered: DiscoveredSpecFileChanges,
    pub loaded: LoadedSpecSnapshots,
}

pub fn discover_and_load_spec_snapshots(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<DiscoveredAndLoadedSpecSnapshots, PolicyGitError> {
    let mut discovered = discover_spec_file_changes(project_root, base_ref, head_ref)?;
    hydrate_rename_copy_semantics(
        project_root,
        base_ref,
        head_ref,
        &mut discovered.fail_closed_operations,
    )?;
    let loaded = load_spec_snapshots_for_changed_paths(
        project_root,
        base_ref,
        head_ref,
        &discovered.changed_spec_paths,
    )?;

    Ok(DiscoveredAndLoadedSpecSnapshots { discovered, loaded })
}

fn hydrate_rename_copy_semantics(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    operations: &mut [FailClosedSpecOperation],
) -> Result<(), PolicyGitError> {
    let mut operation_indexes = Vec::new();
    let mut base_paths = Vec::new();
    let mut head_paths = Vec::new();

    for (index, operation) in operations.iter_mut().enumerate() {
        let FailClosedSpecOperation::RenameOrCopy {
            from_path,
            to_path,
            semantic_pairing,
            ..
        } = operation
        else {
            continue;
        };

        if !from_path.ends_with(".spec.yml") || !to_path.ends_with(".spec.yml") {
            *semantic_pairing = RenameCopySemanticPairing::Inconclusive;
            continue;
        }

        operation_indexes.push(index);
        base_paths.push(from_path.clone());
        head_paths.push(to_path.clone());
    }

    if operation_indexes.is_empty() {
        return Ok(());
    }

    let base_blobs = load_blob_batch_for_ref(project_root, base_ref, &base_paths)?;
    let head_blobs = load_blob_batch_for_ref(project_root, head_ref, &head_paths)?;

    for (batch_index, operation_index) in operation_indexes.into_iter().enumerate() {
        let operation = operations
            .get_mut(operation_index)
            .expect("operation index from same vector must exist");

        let FailClosedSpecOperation::RenameOrCopy {
            from_path,
            to_path,
            semantic_pairing,
            ..
        } = operation
        else {
            continue;
        };

        let mut parse_errors = Vec::new();
        let base_spec = parse_spec_blob(
            base_ref,
            from_path,
            base_blobs[batch_index].as_deref(),
            &mut parse_errors,
        );
        let head_spec = parse_spec_blob(
            head_ref,
            to_path,
            head_blobs[batch_index].as_deref(),
            &mut parse_errors,
        );

        *semantic_pairing = if !parse_errors.is_empty() {
            RenameCopySemanticPairing::Inconclusive
        } else if let (Some(base_spec), Some(head_spec)) = (base_spec, head_spec) {
            if specs_semantically_equivalent_for_rename(&base_spec, &head_spec) {
                RenameCopySemanticPairing::Equivalent
            } else {
                RenameCopySemanticPairing::Different
            }
        } else {
            RenameCopySemanticPairing::Inconclusive
        };
    }

    Ok(())
}

pub fn load_spec_snapshots_for_changed_paths(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
    changed_spec_paths: &BTreeSet<String>,
) -> Result<LoadedSpecSnapshots, PolicyGitError> {
    validate_git_worktree(project_root)?;
    validate_ref_exists(project_root, base_ref)?;
    validate_ref_exists(project_root, head_ref)?;

    if changed_spec_paths.is_empty() {
        return Ok(LoadedSpecSnapshots::default());
    }

    let mut snapshots = Vec::with_capacity(changed_spec_paths.len());
    let mut errors = Vec::new();

    let ordered_paths: Vec<String> = changed_spec_paths.iter().cloned().collect();
    let base_blobs = load_blob_batch_for_ref(project_root, base_ref, &ordered_paths)?;
    let head_blobs = load_blob_batch_for_ref(project_root, head_ref, &ordered_paths)?;

    for (index, spec_path) in ordered_paths.iter().enumerate() {
        let base_blob = base_blobs[index].as_deref();
        let head_blob = head_blobs[index].as_deref();

        let base_spec = parse_spec_blob(base_ref, spec_path, base_blob, &mut errors);
        let head_spec = parse_spec_blob(head_ref, spec_path, head_blob, &mut errors);

        snapshots.push(SpecSnapshotPair {
            spec_path: spec_path.clone(),
            base_spec,
            head_spec,
        });
    }

    Ok(LoadedSpecSnapshots { snapshots, errors })
}

pub fn discover_spec_file_changes(
    project_root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<DiscoveredSpecFileChanges, PolicyGitError> {
    validate_git_worktree(project_root)?;
    validate_ref_exists(project_root, base_ref)?;
    validate_ref_exists(project_root, head_ref)?;

    let diff_range = format!("{base_ref}..{head_ref}");
    let output = Command::new("git")
        .args([
            "diff",
            "-z",
            "--name-status",
            "--find-renames",
            "--diff-filter=ACDMRT",
            &diff_range,
            "--",
            "*.spec.yml",
        ])
        .current_dir(project_root)
        .output()
        .map_err(|error| {
            PolicyGitError::new(
                "git.command_failed",
                format!("failed to execute git diff: {error}"),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyGitError::new(
            "git.diff_failed",
            format!("git diff failed: {}", stderr.trim()),
        ));
    }

    parse_name_status_z(&output.stdout)
}

fn validate_git_worktree(project_root: &Path) -> Result<(), PolicyGitError> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project_root)
        .output()
        .map_err(|error| {
            PolicyGitError::new(
                "git.not_repository",
                format!("failed to execute git rev-parse: {error}"),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyGitError::new(
            "git.not_repository",
            format!("project root is not a git repository: {}", stderr.trim()),
        ));
    }

    let inside_work_tree = String::from_utf8_lossy(&output.stdout);
    if inside_work_tree.trim() != "true" {
        return Err(PolicyGitError::new(
            "git.not_repository",
            "project root is not inside a git worktree",
        ));
    }

    Ok(())
}

fn validate_ref_exists(project_root: &Path, reference: &str) -> Result<(), PolicyGitError> {
    let commit_ref = format!("{reference}^{{commit}}");
    let output = Command::new("git")
        .args(["cat-file", "-e", &commit_ref])
        .current_dir(project_root)
        .output()
        .map_err(|error| {
            PolicyGitError::new(
                "git.command_failed",
                format!("failed to execute git cat-file: {error}"),
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_shallow_repository(project_root)? {
        return Err(PolicyGitError::new(
            "git.shallow_clone_missing_ref",
            format!(
                "git reference '{reference}' is unavailable in this shallow clone ({stderr}). \
Use actions/checkout@v4 with fetch-depth: 0, or run `git fetch --deepen=200 origin {reference}` and retry."
            ),
        ));
    }

    Err(PolicyGitError::new(
        "git.invalid_ref",
        format!("invalid git reference '{reference}': {}", stderr.trim()),
    ))
}

fn is_shallow_repository(project_root: &Path) -> Result<bool, PolicyGitError> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(project_root)
        .output()
        .map_err(|error| {
            PolicyGitError::new(
                "git.command_failed",
                format!("failed to execute git rev-parse --is-shallow-repository: {error}"),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyGitError::new(
            "git.command_failed",
            format!(
                "git rev-parse --is-shallow-repository failed: {}",
                stderr.trim()
            ),
        ));
    }

    let is_shallow = String::from_utf8_lossy(&output.stdout);
    Ok(is_shallow.trim() == "true")
}

fn load_blob_batch_for_ref(
    project_root: &Path,
    reference: &str,
    spec_paths: &[String],
) -> Result<Vec<Option<Vec<u8>>>, PolicyGitError> {
    if spec_paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut batch_stdin = Vec::new();
    for spec_path in spec_paths {
        batch_stdin.extend_from_slice(format!("{reference}:{spec_path}").as_bytes());
        batch_stdin.push(0);
    }

    let output = Command::new("git")
        .args(["cat-file", "--batch", "-Z"])
        .current_dir(project_root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;

            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(&batch_stdin)?;
            }

            child.wait_with_output()
        })
        .map_err(|error| {
            PolicyGitError::new(
                "git.command_failed",
                format!("failed to execute git cat-file --batch -Z: {error}"),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyGitError::new(
            "git.cat_file_failed",
            format!("git cat-file --batch -Z failed: {}", stderr.trim()),
        ));
    }

    parse_batch_output(reference, spec_paths, &output.stdout)
}

fn parse_batch_output(
    reference: &str,
    spec_paths: &[String],
    raw: &[u8],
) -> Result<Vec<Option<Vec<u8>>>, PolicyGitError> {
    let mut cursor = 0;
    let mut blobs = Vec::with_capacity(spec_paths.len());

    for spec_path in spec_paths {
        let header = read_nul_terminated(raw, &mut cursor).ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!(
                    "truncated git cat-file output while reading header for {reference}:{spec_path}"
                ),
            )
        })?;

        let header_text = String::from_utf8(header.to_vec()).map_err(|_| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("non-UTF-8 header in git cat-file output for {reference}:{spec_path}"),
            )
        })?;

        if header_text.ends_with(" missing") {
            blobs.push(None);
            continue;
        }

        let mut fields = header_text.split_whitespace();
        let _oid = fields.next().ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("malformed cat-file header for {reference}:{spec_path}: {header_text}"),
            )
        })?;
        let object_type = fields.next().ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("malformed cat-file header for {reference}:{spec_path}: {header_text}"),
            )
        })?;
        let size_text = fields.next().ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("malformed cat-file header for {reference}:{spec_path}: {header_text}"),
            )
        })?;

        if object_type != "blob" {
            return Err(PolicyGitError::new(
                "git.batch_parse_error",
                format!(
                    "expected blob for {reference}:{spec_path}, got '{object_type}' ({header_text})"
                ),
            ));
        }

        let size = size_text.parse::<usize>().map_err(|_| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("invalid blob size in header for {reference}:{spec_path}: {header_text}"),
            )
        })?;

        let end = cursor.checked_add(size).ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!("blob size overflow for {reference}:{spec_path}"),
            )
        })?;

        let blob = raw.get(cursor..end).ok_or_else(|| {
            PolicyGitError::new(
                "git.batch_parse_error",
                format!(
                    "truncated git cat-file output while reading blob for {reference}:{spec_path}"
                ),
            )
        })?;
        cursor = end;

        if raw.get(cursor) != Some(&0) {
            return Err(PolicyGitError::new(
                "git.batch_parse_error",
                format!("missing NUL separator after blob for {reference}:{spec_path}"),
            ));
        }
        cursor += 1;

        blobs.push(Some(blob.to_vec()));
    }

    Ok(blobs)
}

fn read_nul_terminated<'a>(raw: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let start = *cursor;
    let relative_end = raw.get(start..)?.iter().position(|byte| *byte == b'\0')?;
    let end = start + relative_end;
    *cursor = end + 1;
    Some(&raw[start..end])
}

fn parse_spec_blob(
    reference: &str,
    spec_path: &str,
    blob: Option<&[u8]>,
    errors: &mut Vec<PolicyDiffErrorEntry>,
) -> Option<SpecFile> {
    let blob = blob?;

    let source = match String::from_utf8(blob.to_vec()) {
        Ok(source) => source,
        Err(_) => {
            errors.push(PolicyDiffErrorEntry {
                code: "policy.spec_blob_non_utf8".to_string(),
                message: format!("{reference}:{spec_path} is not valid UTF-8"),
                spec_path: Some(spec_path.to_string()),
            });
            return None;
        }
    };

    let mut spec: SpecFile = match yaml_serde::from_str(&source) {
        Ok(spec) => spec,
        Err(error) => {
            errors.push(PolicyDiffErrorEntry {
                code: "policy.spec_parse_error".to_string(),
                message: format!("failed to parse {reference}:{spec_path}: {error}"),
                spec_path: Some(spec_path.to_string()),
            });
            return None;
        }
    };

    spec.spec_path = Some(PathBuf::from(spec_path));
    Some(spec)
}

pub fn list_tracked_files_scoped(
    project_root: &Path,
    reference: &str,
    prefixes: &BTreeSet<String>,
) -> Result<BTreeSet<String>, PolicyGitError> {
    validate_git_worktree(project_root)?;
    validate_ref_exists(project_root, reference)?;

    if prefixes.is_empty() {
        return Ok(BTreeSet::new());
    }

    let mut command = Command::new("git");
    command
        .arg("ls-tree")
        .arg("-r")
        .arg("-z")
        .arg("--name-only")
        .arg(reference)
        .arg("--");

    for prefix in prefixes {
        command.arg(prefix);
    }

    let output = command
        .current_dir(project_root)
        .output()
        .map_err(|error| {
            PolicyGitError::new(
                "git.command_failed",
                format!("failed to execute git ls-tree: {error}"),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PolicyGitError::new(
            "git.ls_tree_failed",
            format!("git ls-tree failed: {}", stderr.trim()),
        ));
    }

    let mut files = BTreeSet::new();
    for token in output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|token| !token.is_empty())
    {
        files.insert(parse_utf8_token(token, "path")?);
    }

    Ok(files)
}

pub fn parse_name_status_z(raw: &[u8]) -> Result<DiscoveredSpecFileChanges, PolicyGitError> {
    let mut parsed = DiscoveredSpecFileChanges::default();

    if raw.is_empty() {
        return Ok(parsed);
    }

    let tokens: Vec<&[u8]> = raw
        .split(|byte| *byte == b'\0')
        .filter(|t| !t.is_empty())
        .collect();

    let mut index = 0;
    while index < tokens.len() {
        let status = parse_utf8_token(tokens[index], "status")?;
        index += 1;

        let status_kind = status.chars().next().ok_or_else(|| {
            PolicyGitError::new("git.diff_parse_error", "encountered empty git status token")
        })?;

        match status_kind {
            'A' | 'M' | 'T' => {
                let path = next_path_token(&tokens, &mut index, &status)?;
                if path.ends_with(".spec.yml") {
                    parsed.changed_spec_paths.insert(path);
                }
            }
            'D' => {
                let path = next_path_token(&tokens, &mut index, &status)?;
                if path.ends_with(".spec.yml") {
                    parsed
                        .fail_closed_operations
                        .push(FailClosedSpecOperation::Deletion { path });
                }
            }
            'R' | 'C' => {
                let from_path = next_path_token(&tokens, &mut index, &status)?;
                let to_path = next_path_token(&tokens, &mut index, &status)?;
                if from_path.ends_with(".spec.yml") || to_path.ends_with(".spec.yml") {
                    parsed
                        .fail_closed_operations
                        .push(FailClosedSpecOperation::RenameOrCopy {
                            status,
                            from_path,
                            to_path,
                            semantic_pairing: RenameCopySemanticPairing::Unassessed,
                        });
                }
            }
            other => {
                return Err(PolicyGitError::new(
                    "git.diff_parse_error",
                    format!("unsupported git status '{other}' in diff output"),
                ));
            }
        }
    }

    Ok(parsed)
}

fn next_path_token(
    tokens: &[&[u8]],
    index: &mut usize,
    status: &str,
) -> Result<String, PolicyGitError> {
    let token = tokens.get(*index).ok_or_else(|| {
        PolicyGitError::new(
            "git.diff_parse_error",
            format!("truncated git diff output after status '{status}'"),
        )
    })?;
    *index += 1;

    parse_utf8_token(token, "path")
}

fn parse_utf8_token(token: &[u8], label: &str) -> Result<String, PolicyGitError> {
    String::from_utf8(token.to_vec()).map_err(|_| {
        PolicyGitError::new(
            "git.diff_parse_error",
            format!("git diff emitted non-UTF-8 {label} token"),
        )
    })
}
