use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::process::Command;

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
    },
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
