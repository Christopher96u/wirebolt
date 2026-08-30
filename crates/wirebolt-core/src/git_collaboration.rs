use std::{
    error::Error,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const MANAGED_FILE: &str = "wirebolt.toml";
const MANAGED_DIRECTORIES: [&str; 2] = ["collections/", "environments/"];
const MAX_COMMIT_MESSAGE_BYTES: usize = 4 * 1024;
const DEFAULT_NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    /// `None` before the first commit.
    pub revision: Option<String>,
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

    fn into_operation(self, outcome: GitOperationOutcome) -> GitOperation {
        GitOperation {
            outcome,
            revision: self.revision.clone(),
            status: self,
        }
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
    TimedOut,
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
            GitErrorKind::TimedOut => "Git did not finish in time",
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
    network_timeout: Duration,
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
        let output = git_output(&canonical_root, ["rev-parse", "--show-toplevel"], &[], None)?;
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
            network_timeout: DEFAULT_NETWORK_TIMEOUT,
        })
    }

    /// Bounds `pull` and `push`, which otherwise hang for as long as SSH or
    /// HTTPS transport does on a dead network.
    #[must_use]
    pub const fn with_network_timeout(mut self, timeout: Duration) -> Self {
        self.network_timeout = timeout;
        self
    }

    /// Returns the current local Git state without fetching or mutating files.
    ///
    /// One `git status --porcelain=v2 --branch` call provides the branch,
    /// upstream, divergence, revision, and every change.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when Git cannot inspect the repository or emits
    /// malformed porcelain output.
    pub fn status(&self) -> Result<GitStatus, GitError> {
        let output = self.git([
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ])?;
        if !output.status.success() {
            return Err(GitError::new(GitErrorKind::CommandFailed, "read status"));
        }
        let mut status = parse_status(&output.stdout)?;
        status
            .changes
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(status)
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
            return Ok(before.into_operation(GitOperationOutcome::Conflicted));
        }
        // Explicit paths keep both Git commands scoped even if the worktree
        // changes underneath us; an empty pathspec would mean the whole tree.
        let paths = managed_change_paths(&before);
        if paths.is_empty() {
            return Ok(before.into_operation(GitOperationOutcome::NothingToCommit));
        }

        let add = self.git_with_paths(["add", "--all"], &paths)?;
        require_success(&add, "stage managed documents")?;
        let commit = self.git_with_paths(["commit", "--only", "-m", message], &paths)?;
        require_success(&commit, "create commit")?;
        Ok(self
            .status()?
            .into_operation(GitOperationOutcome::Committed))
    }

    /// Pulls the configured upstream only after confirming the worktree is
    /// clean. Merge conflicts are left intact and returned to the caller.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] when the worktree is dirty, no upstream exists,
    /// authentication fails, the network timeout elapses, or Git cannot
    /// complete the pull.
    pub fn pull(&self) -> Result<GitOperation, GitError> {
        let before = self.status()?;
        if before.conflicts().next().is_some() {
            return Ok(before.into_operation(GitOperationOutcome::Conflicted));
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

        let pull = self.git_network(["pull", "--no-rebase", "--no-edit", "--no-stat"])?;
        let status = self.status()?;
        if !pull.status.success() && status.conflicts().next().is_some() {
            return Ok(status.into_operation(GitOperationOutcome::Conflicted));
        }
        require_success(&pull, "pull upstream")?;
        let outcome = if status.revision == before.revision {
            GitOperationOutcome::UpToDate
        } else {
            GitOperationOutcome::Updated
        };
        Ok(status.into_operation(outcome))
    }

    /// Pushes the current branch using the user's configured Git credentials.
    /// When no upstream exists, `origin/<branch>` is configured explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] for detached HEAD, missing origin, authentication
    /// failure, rejected updates, the network timeout, or another failed Git
    /// operation.
    pub fn push(&self) -> Result<GitOperation, GitError> {
        let before = self.status()?;
        if before.conflicts().next().is_some() {
            return Ok(before.into_operation(GitOperationOutcome::Conflicted));
        }
        let branch = before
            .branch
            .as_deref()
            .ok_or_else(|| GitError::new(GitErrorKind::DetachedHead, "push current branch"))?;
        let (push, previously_had_upstream) = if before.upstream.is_some() {
            (self.git_network(["push"])?, true)
        } else {
            let remote = self.git(["remote", "get-url", "origin"])?;
            if !remote.status.success() {
                return Err(GitError::new(
                    GitErrorKind::MissingRemote,
                    "find origin remote",
                ));
            }
            (
                self.git_network(["push", "--set-upstream", "origin", branch])?,
                false,
            )
        };
        require_success(&push, "push current branch")?;
        let outcome = if previously_had_upstream && before.ahead == 0 {
            GitOperationOutcome::UpToDate
        } else {
            GitOperationOutcome::Pushed
        };
        Ok(self.status()?.into_operation(outcome))
    }

    fn git<const N: usize>(&self, arguments: [&str; N]) -> Result<Output, GitError> {
        git_output(&self.root, arguments, &[], None)
    }

    fn git_network<const N: usize>(&self, arguments: [&str; N]) -> Result<Output, GitError> {
        git_output(&self.root, arguments, &[], Some(self.network_timeout))
    }

    fn git_with_paths<const N: usize>(
        &self,
        arguments: [&str; N],
        paths: &[String],
    ) -> Result<Output, GitError> {
        git_output(&self.root, arguments, paths, None)
    }
}

/// Paths of managed documents that differ from HEAD, including the old side
/// of renames so both halves are committed together. Wirebolt's own
/// temporary files are never committed.
fn managed_change_paths(status: &GitStatus) -> Vec<String> {
    let mut paths = Vec::new();
    for change in &status.changes {
        for path in std::iter::once(&change.path).chain(change.previous_path.as_ref()) {
            if is_managed_path(path) && !is_temporary_file(path) && !paths.contains(path) {
                paths.push(path.clone());
            }
        }
    }
    paths
}

fn is_managed_path(path: &str) -> bool {
    path == MANAGED_FILE
        || MANAGED_DIRECTORIES
            .iter()
            .any(|directory| path.starts_with(directory))
}

fn is_temporary_file(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| {
        name.starts_with('.')
            && name.contains(".wirebolt-")
            && Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "tmp")
    })
}

fn git_output<const N: usize>(
    root: &Path,
    arguments: [&str; N],
    paths: &[String],
    timeout: Option<Duration>,
) -> Result<Output, GitError> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !paths.is_empty() {
        command.arg("--").args(paths);
    }
    let child = command
        .spawn()
        .map_err(|_| GitError::new(GitErrorKind::GitUnavailable, "run Git"))?;
    wait_with_timeout(child, timeout)
}

/// Drains both pipes on helper threads so a chatty command never deadlocks.
/// Local commands block on `wait`; only network commands poll, so the poll
/// interval never adds latency to a status check.
fn wait_with_timeout(mut child: Child, timeout: Option<Duration>) -> Result<Output, GitError> {
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let failed = || GitError::new(GitErrorKind::CommandFailed, "wait for Git");
    let status = match timeout {
        None => child.wait().map_err(|_| failed())?,
        Some(timeout) => {
            let started = Instant::now();
            loop {
                if let Some(status) = child.try_wait().map_err(|_| failed())? {
                    break status;
                }
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(GitError::new(GitErrorKind::TimedOut, "wait for Git"));
                }
                thread::sleep(CHILD_POLL_INTERVAL);
            }
        }
    };
    let stdout = stdout.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
    let stderr = stderr.map_or_else(Vec::new, |handle| handle.join().unwrap_or_default());
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn drain<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.read_to_end(&mut bytes);
        bytes
    })
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

/// Parses `git status --porcelain=v2 --branch -z` output.
fn parse_status(bytes: &[u8]) -> Result<GitStatus, GitError> {
    let invalid = || GitError::new(GitErrorKind::InvalidOutput, "parse status record");
    let mut status = GitStatus {
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        revision: None,
        changes: Vec::new(),
    };
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| std::str::from_utf8(record).map_err(|_| invalid()));
    while let Some(record) = records.next() {
        let record = record?;
        if let Some(header) = record.strip_prefix("# ") {
            parse_branch_header(header, &mut status);
            continue;
        }
        let (kind, rest) = record.split_once(' ').ok_or_else(invalid)?;
        let change = match kind {
            "1" => parse_change(rest, 6, false)?,
            "2" => {
                let mut change = parse_change(rest, 7, false)?;
                let previous = records.next().ok_or_else(invalid)??;
                change.previous_path = Some(previous.to_owned());
                change
            }
            "u" => parse_change(rest, 8, true)?,
            "?" => GitChange {
                path: rest.to_owned(),
                previous_path: None,
                staged: GitDelta::None,
                unstaged: GitDelta::Untracked,
                conflicted: false,
            },
            "!" => continue,
            _ => return Err(invalid()),
        };
        status.changes.push(change);
    }
    Ok(status)
}

fn parse_branch_header(header: &str, status: &mut GitStatus) {
    let Some((key, value)) = header.split_once(' ') else {
        return;
    };
    match key {
        "branch.oid" if value != "(initial)" => status.revision = Some(value.to_owned()),
        "branch.head" if value != "(detached)" => status.branch = Some(value.to_owned()),
        "branch.upstream" => status.upstream = Some(value.to_owned()),
        "branch.ab" => {
            for field in value.split_ascii_whitespace() {
                if let Some(ahead) = field.strip_prefix('+') {
                    status.ahead = ahead.parse().unwrap_or(0);
                } else if let Some(behind) = field.strip_prefix('-') {
                    status.behind = behind.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

/// Parses the fields after the record type: `<XY>` followed by
/// `metadata_fields` space-separated fields and then the path.
fn parse_change(
    rest: &str,
    metadata_fields: usize,
    conflicted: bool,
) -> Result<GitChange, GitError> {
    let invalid = || GitError::new(GitErrorKind::InvalidOutput, "parse status record");
    let mut fields = rest.splitn(metadata_fields + 2, ' ');
    let xy = fields.next().ok_or_else(invalid)?.as_bytes();
    if xy.len() != 2 {
        return Err(invalid());
    }
    for _ in 0..metadata_fields {
        fields.next().ok_or_else(invalid)?;
    }
    let path = fields.next().ok_or_else(invalid)?;
    Ok(GitChange {
        path: path.to_owned(),
        previous_path: None,
        staged: delta(xy[0])?,
        unstaged: delta(xy[1])?,
        conflicted,
    })
}

fn delta(code: u8) -> Result<GitDelta, GitError> {
    match code {
        b'.' | b' ' => Ok(GitDelta::None),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2_headers_and_records() {
        let output = concat!(
            "# branch.oid 0123abcd\0",
            "# branch.head main\0",
            "# branch.upstream origin/main\0",
            "# branch.ab +2 -1\0",
            "1 M. N... 100644 100644 100644 aaaa bbbb wirebolt.toml\0",
            "2 R. N... 100644 100644 100644 aaaa bbbb R100 collections/api/requests/new name.toml\0",
            "collections/api/requests/old.toml\0",
            "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc environments/dev.toml\0",
            "? notes.txt\0",
            "! ignored.log\0",
        );

        let status = parse_status(output.as_bytes()).expect("valid porcelain");

        assert_eq!(status.revision.as_deref(), Some("0123abcd"));
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        assert_eq!(status.changes.len(), 4);
        assert_eq!(status.changes[0].staged, GitDelta::Modified);
        assert_eq!(status.changes[0].unstaged, GitDelta::None);
        assert_eq!(
            status.changes[1].path,
            "collections/api/requests/new name.toml"
        );
        assert_eq!(
            status.changes[1].previous_path.as_deref(),
            Some("collections/api/requests/old.toml")
        );
        assert!(status.changes[2].conflicted);
        assert_eq!(status.changes[2].staged, GitDelta::Unmerged);
        assert_eq!(status.changes[3].unstaged, GitDelta::Untracked);
    }

    #[test]
    fn initial_and_detached_states_have_no_revision_or_branch() {
        let output = "# branch.oid (initial)\0# branch.head (detached)\0";

        let status = parse_status(output.as_bytes()).expect("valid porcelain");

        assert_eq!(status.revision, None);
        assert_eq!(status.branch, None);
        assert!(status.is_clean());
    }

    #[test]
    fn managed_paths_exclude_unmanaged_files_and_temporary_documents() {
        let change = |path: &str, previous: Option<&str>| GitChange {
            path: path.to_owned(),
            previous_path: previous.map(str::to_owned),
            staged: GitDelta::None,
            unstaged: GitDelta::Modified,
            conflicted: false,
        };
        let status = GitStatus {
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            revision: None,
            changes: vec![
                change("notes.txt", None),
                change("wirebolt.toml", None),
                change(
                    "collections/api/requests/.list.toml.wirebolt-12-3.tmp",
                    None,
                ),
                change(
                    "collections/api/requests/new.toml",
                    Some("collections/api/requests/old.toml"),
                ),
                change("environments-archive/dev.toml", None),
            ],
        };

        assert_eq!(
            managed_change_paths(&status),
            [
                "wirebolt.toml",
                "collections/api/requests/new.toml",
                "collections/api/requests/old.toml",
            ]
        );
    }
}
