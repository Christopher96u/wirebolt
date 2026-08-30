use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const MANAGED_PATHS: [&str; 3] = ["wirebolt.toml", "collections", "environments"];
const MAX_COMMIT_MESSAGE_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitDelta {
    None,
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Unmerged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: GitDelta,
    pub unstaged: GitDelta,
    pub conflicted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<GitChange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitOperationOutcome {
    NothingToCommit,
    Committed,
    Updated,
    UpToDate,
    Pushed,
    Conflicted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitOperation {
    pub outcome: GitOperationOutcome,
    pub revision: Option<String>,
    pub status: GitStatus,
}

impl GitStatus {
    pub fn conflicts(&self) -> impl Iterator<Item = &GitChange> {
        self.changes.iter().filter(|change| change.conflicted)
    }

    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitErrorKind {
    GitUnavailable,
    NotRepository,
    WorkspaceNotRepositoryRoot,
    CommandFailed,
    InvalidOutput,
    InvalidCommitMessage,
    IdentityMissing,
    AuthenticationRequired,
    DirtyWorkspace,
    MissingUpstream,
    MissingRemote,
    DetachedHead,
}

#[derive(Debug)]
pub struct GitError {
    pub kind: GitErrorKind,
    operation: &'static str,
}

impl GitError {
    fn new(kind: GitErrorKind, operation: &'static str) -> Self {
        Self { kind, operation }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            GitErrorKind::GitUnavailable => "Git is unavailable",
            GitErrorKind::NotRepository => "workspace is not a Git repository",
            GitErrorKind::WorkspaceNotRepositoryRoot => {
                "workspace must be the root of its Git repository"
            }
            GitErrorKind::CommandFailed => "Git operation failed",
            GitErrorKind::InvalidOutput => "Git returned invalid status data",
            GitErrorKind::InvalidCommitMessage => "commit message is invalid",
            GitErrorKind::IdentityMissing => "Git author identity is not configured",
            GitErrorKind::AuthenticationRequired => "Git authentication failed",
            GitErrorKind::DirtyWorkspace => "workspace has uncommitted changes",
            GitErrorKind::MissingUpstream => "current branch has no upstream",
            GitErrorKind::MissingRemote => "workspace has no origin remote",
            GitErrorKind::DetachedHead => "workspace is in detached HEAD state",
        };
        write!(
            formatter,
            "{message} while attempting to {}",
            self.operation
        )
    }
}

impl Error for GitError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitWorkspace {
    root: PathBuf,
}

impl GitWorkspace {
    /// Opens a workspace whose root is also a Git worktree root.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git is unavailable, the path is not a
    /// repository, or the workspace is nested inside a larger repository.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitError> {
        let root = root.into();
        let canonical_root = fs::canonicalize(&root)
            .map_err(|_| GitError::new(GitErrorKind::NotRepository, "open repository"))?;
        let output = git_output(&canonical_root, ["rev-parse", "--show-toplevel"])?;
        if !output.status.success() {
            return Err(GitError::new(
                GitErrorKind::NotRepository,
                "open repository",
            ));
        }
        let top_level = text_line(&output.stdout, "open repository")?;
        let canonical_top_level = fs::canonicalize(top_level)
            .map_err(|_| GitError::new(GitErrorKind::InvalidOutput, "resolve repository root"))?;
        if canonical_root != canonical_top_level {
            return Err(GitError::new(
                GitErrorKind::WorkspaceNotRepositoryRoot,
                "open repository",
            ));
        }
        Ok(Self {
            root: canonical_root,
        })
    }

    /// Returns the current local Git state without fetching or mutating files.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot inspect the repository or emits
    /// malformed porcelain output.
    pub fn status(&self) -> Result<GitStatus, GitError> {
        let branch_output = self.git(["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        let branch = if branch_output.status.success() {
            Some(text_line(&branch_output.stdout, "read current branch")?.to_owned())
        } else {
            None
        };

        let upstream_output = self.git([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ])?;
        let upstream = if upstream_output.status.success() {
            Some(text_line(&upstream_output.stdout, "read upstream")?.to_owned())
        } else {
            None
        };
        let (ahead, behind) = if upstream.is_some() {
            let output = self.git(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
            if !output.status.success() {
                return Err(GitError::new(
                    GitErrorKind::CommandFailed,
                    "compare with upstream",
                ));
            }
            parse_divergence(&output.stdout)?
        } else {
            (0, 0)
        };

        let output = self.git(["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        if !output.status.success() {
            return Err(GitError::new(GitErrorKind::CommandFailed, "read status"));
        }
        let mut changes = parse_changes(&output.stdout)?;
        changes.sort_by(|left, right| left.path.cmp(&right.path));

        Ok(GitStatus {
            branch,
            upstream,
            ahead,
            behind,
            changes,
        })
    }

    /// Commits only Wirebolt-managed documents and preserves unrelated index
    /// entries.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] for invalid messages, missing Git identity, or a
    /// failed Git operation. Existing merge conflicts are returned as a
    /// [`GitOperationOutcome::Conflicted`] result without modification.
    pub fn commit(&self, message: &str) -> Result<GitOperation, GitError> {
        let message = message.trim();
        if message.is_empty() || message.len() > MAX_COMMIT_MESSAGE_BYTES {
            return Err(GitError::new(
                GitErrorKind::InvalidCommitMessage,
                "create commit",
            ));
        }

        let before = self.status()?;
        if before.conflicts().next().is_some() {
            return Ok(GitOperation {
                outcome: GitOperationOutcome::Conflicted,
                revision: current_revision(self)?,
                status: before,
            });
        }

        let managed_status = self.git_with_paths(
            ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            &MANAGED_PATHS,
        )?;
        require_success(&managed_status, "inspect managed documents")?;
        if managed_status.stdout.is_empty() {
            return Ok(GitOperation {
                outcome: GitOperationOutcome::NothingToCommit,
                revision: current_revision(self)?,
                status: before,
            });
        }

        let managed_paths = self.managed_paths()?;
        let add = self.git_with_paths(["add", "--all"], &managed_paths)?;
        require_success(&add, "stage managed documents")?;
        let commit = self.git_with_paths(["commit", "--only", "-m", message], &managed_paths)?;
        require_success(&commit, "create commit")?;
        Ok(GitOperation {
            outcome: GitOperationOutcome::Committed,
            revision: current_revision(self)?,
            status: self.status()?,
        })
    }

    /// Pulls the configured upstream only after confirming the worktree is
    /// clean. Merge conflicts are left intact and returned to the caller.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when the worktree is dirty, no upstream exists,
    /// authentication fails, or Git cannot complete the pull.
    pub fn pull(&self) -> Result<GitOperation, GitError> {
        let before = self.status()?;
        if before.conflicts().next().is_some() {
            return Ok(GitOperation {
                outcome: GitOperationOutcome::Conflicted,
                revision: current_revision(self)?,
                status: before,
            });
        }
        if !before.is_clean() {
            return Err(GitError::new(GitErrorKind::DirtyWorkspace, "pull upstream"));
        }
        if before.upstream.is_none() {
            return Err(GitError::new(
                GitErrorKind::MissingUpstream,
                "pull upstream",
            ));
        }

        let previous_revision = current_revision(self)?;
        let pull = self.git(["pull", "--no-rebase", "--no-edit", "--no-stat"])?;
        let status = self.status()?;
        if !pull.status.success() && status.conflicts().next().is_some() {
            return Ok(GitOperation {
                outcome: GitOperationOutcome::Conflicted,
                revision: current_revision(self)?,
                status,
            });
        }
        require_success(&pull, "pull upstream")?;
        let revision = current_revision(self)?;
        Ok(GitOperation {
            outcome: if revision == previous_revision {
                GitOperationOutcome::UpToDate
            } else {
                GitOperationOutcome::Updated
            },
            revision,
            status,
        })
    }

    /// Pushes the current branch using the user's configured Git credentials.
    /// When no upstream exists, `origin/<branch>` is configured explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] for detached HEAD, missing origin, authentication
    /// failure, rejected updates, or another failed Git operation.
    pub fn push(&self) -> Result<GitOperation, GitError> {
        let before = self.status()?;
        if before.conflicts().next().is_some() {
            return Ok(GitOperation {
                outcome: GitOperationOutcome::Conflicted,
                revision: current_revision(self)?,
                status: before,
            });
        }
        let branch = before
            .branch
            .as_deref()
            .ok_or_else(|| GitError::new(GitErrorKind::DetachedHead, "push current branch"))?;
        let (push, previously_had_upstream) = if before.upstream.is_some() {
            (self.git(["push"])?, true)
        } else {
            let remote = self.git(["remote", "get-url", "origin"])?;
            if !remote.status.success() {
                return Err(GitError::new(
                    GitErrorKind::MissingRemote,
                    "find origin remote",
                ));
            }
            (
                self.git(["push", "--set-upstream", "origin", branch])?,
                false,
            )
        };
        require_success(&push, "push current branch")?;
        Ok(GitOperation {
            outcome: if previously_had_upstream && before.ahead == 0 {
                GitOperationOutcome::UpToDate
            } else {
                GitOperationOutcome::Pushed
            },
            revision: current_revision(self)?,
            status: self.status()?,
        })
    }

    fn git<const N: usize>(&self, arguments: [&str; N]) -> Result<Output, GitError> {
        git_output(&self.root, arguments)
    }

    fn git_with_paths<const N: usize>(
        &self,
        arguments: [&str; N],
        paths: &[&str],
    ) -> Result<Output, GitError> {
        git_output_with_paths(&self.root, arguments, paths)
    }

    fn managed_paths(&self) -> Result<Vec<&'static str>, GitError> {
        let mut paths = Vec::new();
        for path in MANAGED_PATHS {
            let changed = self.git_with_paths(
                ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
                &[path],
            )?;
            require_success(&changed, "inspect managed paths")?;
            if !changed.stdout.is_empty() {
                paths.push(path);
            }
        }
        Ok(paths)
    }
}

fn git_output<const N: usize>(root: &Path, arguments: [&str; N]) -> Result<Output, GitError> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .output()
        .map_err(|_| GitError::new(GitErrorKind::GitUnavailable, "run Git"))
}

fn git_output_with_paths<const N: usize>(
    root: &Path,
    arguments: [&str; N],
    paths: &[&str],
) -> Result<Output, GitError> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .arg("--")
        .args(paths)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .output()
        .map_err(|_| GitError::new(GitErrorKind::GitUnavailable, "run Git"))
}

fn current_revision(workspace: &GitWorkspace) -> Result<Option<String>, GitError> {
    let output = workspace.git(["rev-parse", "--verify", "HEAD"])?;
    if output.status.success() {
        Ok(Some(
            text_line(&output.stdout, "read current revision")?.to_owned(),
        ))
    } else {
        Ok(None)
    }
}

fn require_success(output: &Output, operation: &'static str) -> Result<(), GitError> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let kind = if stderr.contains("Author identity unknown")
        || stderr.contains("unable to auto-detect email address")
    {
        GitErrorKind::IdentityMissing
    } else if stderr.contains("Authentication failed")
        || stderr.contains("Permission denied")
        || stderr.contains("could not read Username")
    {
        GitErrorKind::AuthenticationRequired
    } else {
        GitErrorKind::CommandFailed
    };
    Err(GitError::new(kind, operation))
}

fn text_line<'a>(bytes: &'a [u8], operation: &'static str) -> Result<&'a str, GitError> {
    let line = std::str::from_utf8(bytes)
        .map_err(|_| GitError::new(GitErrorKind::InvalidOutput, operation))?
        .trim();
    if line.is_empty() {
        Err(GitError::new(GitErrorKind::InvalidOutput, operation))
    } else {
        Ok(line)
    }
}

fn parse_divergence(bytes: &[u8]) -> Result<(u64, u64), GitError> {
    let line = text_line(bytes, "parse upstream divergence")?;
    let mut fields = line.split_ascii_whitespace();
    let ahead = fields
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| GitError::new(GitErrorKind::InvalidOutput, "parse ahead count"))?;
    let behind = fields
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| GitError::new(GitErrorKind::InvalidOutput, "parse behind count"))?;
    if fields.next().is_some() {
        return Err(GitError::new(
            GitErrorKind::InvalidOutput,
            "parse upstream divergence",
        ));
    }
    Ok((ahead, behind))
}

fn parse_changes(bytes: &[u8]) -> Result<Vec<GitChange>, GitError> {
    let records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut changes = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() < 4 || record[2] != b' ' {
            return Err(GitError::new(
                GitErrorKind::InvalidOutput,
                "parse status record",
            ));
        }
        let x = record[0];
        let y = record[1];
        let path = path_text(&record[3..])?;
        let renamed_or_copied = matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C');
        let previous_path = if renamed_or_copied {
            index += 1;
            Some(path_text(records.get(index).ok_or_else(|| {
                GitError::new(GitErrorKind::InvalidOutput, "parse renamed path")
            })?)?)
        } else {
            None
        };
        let untracked = x == b'?' && y == b'?';
        changes.push(GitChange {
            path,
            previous_path,
            staged: if untracked { GitDelta::None } else { delta(x)? },
            unstaged: if untracked {
                GitDelta::Untracked
            } else {
                delta(y)?
            },
            conflicted: is_conflict(x, y),
        });
        index += 1;
    }
    Ok(changes)
}

fn path_text(bytes: &[u8]) -> Result<String, GitError> {
    std::str::from_utf8(bytes)
        .map(ToOwned::to_owned)
        .map_err(|_| GitError::new(GitErrorKind::InvalidOutput, "decode repository path"))
}

fn delta(code: u8) -> Result<GitDelta, GitError> {
    match code {
        b' ' => Ok(GitDelta::None),
        b'A' => Ok(GitDelta::Added),
        b'M' => Ok(GitDelta::Modified),
        b'D' => Ok(GitDelta::Deleted),
        b'R' => Ok(GitDelta::Renamed),
        b'C' => Ok(GitDelta::Copied),
        b'T' => Ok(GitDelta::TypeChanged),
        b'U' => Ok(GitDelta::Unmerged),
        _ => Err(GitError::new(
            GitErrorKind::InvalidOutput,
            "parse status code",
        )),
    }
}

fn is_conflict(x: u8, y: u8) -> bool {
    matches!(
        (x, y),
        (b'D' | b'U', b'D') | (b'A' | b'D' | b'U', b'U') | (b'A' | b'U', b'A')
    )
}
