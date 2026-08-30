use std::{
    error::Error,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use crate::storage::is_managed_document_path;

const MAX_COMMIT_MESSAGE_BYTES: usize = 4 * 1024;
const DEFAULT_NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long a timed-out Git gets to act on SIGTERM (and drop its lock
/// files) before the whole process group is killed outright.
const TERMINATION_GRACE: Duration = Duration::from_secs(1);
/// How long to wait for the output pipes to close after a timed-out tree
/// was killed before giving up on reclaiming the drain threads.
const DRAIN_GRACE: Duration = Duration::from_secs(1);

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
/// of renames so both halves are committed together. Only files at the exact
/// places Wirebolt writes documents qualify, so notes, temporary files, and
/// anything nested where no document belongs are never committed.
fn managed_change_paths(status: &GitStatus) -> Vec<String> {
    let mut paths = Vec::new();
    for change in &status.changes {
        for path in std::iter::once(&change.path).chain(change.previous_path.as_ref()) {
            if is_managed_document_path(path) && !paths.contains(path) {
                paths.push(path.clone());
            }
        }
    }
    paths
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
        .env("LC_ALL", "C");
    if !paths.is_empty() {
        command.arg("--").args(paths);
    }
    run_command(command, timeout)
}

/// Runs a command with both pipes drained on helper threads so a chatty
/// command never deadlocks. The child leads its own process group, so the
/// helpers Git spawns (ssh, credential and remote helpers) can be ended with
/// it when the timeout elapses.
fn run_command(mut command: Command, timeout: Option<Duration>) -> Result<Output, GitError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|_| GitError::new(GitErrorKind::GitUnavailable, "run Git"))?;
    wait_with_timeout(child, timeout)
}

/// Local commands block on `wait`; only network commands poll, so the poll
/// interval never adds latency to a status check. On timeout the process
/// tree is ended first and each drain is then joined within a bounded grace:
/// once the last pipe writer is gone it finishes at once, and a writer that
/// somehow escaped the group cannot hold the caller hostage.
fn wait_with_timeout(mut child: Child, timeout: Option<Duration>) -> Result<Output, GitError> {
    let stdout = child.stdout.take().map(Drain::spawn);
    let stderr = child.stderr.take().map(Drain::spawn);
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
                    terminate_process_tree(&mut child);
                    for drain in [stdout, stderr].into_iter().flatten() {
                        drain.join_within(DRAIN_GRACE);
                    }
                    return Err(GitError::new(GitErrorKind::TimedOut, "wait for Git"));
                }
                thread::sleep(CHILD_POLL_INTERVAL);
            }
        }
    };
    let stdout = stdout.map_or_else(Vec::new, Drain::join);
    let stderr = stderr.map_or_else(Vec::new, Drain::join);
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// One pipe being read to its end on a helper thread. The bytes arrive on
/// the channel; the handle is joined once they have, so the thread is
/// reclaimed rather than left to finish on its own.
struct Drain {
    output: mpsc::Receiver<Vec<u8>>,
    thread: thread::JoinHandle<()>,
}

impl Drain {
    fn spawn<R: Read + Send + 'static>(mut reader: R) -> Self {
        let (sender, output) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = reader.read_to_end(&mut bytes);
            let _ = sender.send(bytes);
        });
        Self { output, thread }
    }

    /// Waits for the pipe to close and reclaims the thread.
    fn join(self) -> Vec<u8> {
        let bytes = self.output.recv().unwrap_or_default();
        let _ = self.thread.join();
        bytes
    }

    /// Reclaims the thread if the pipe closes within `grace`; only when the
    /// grace elapses is the thread detached, to end whenever the last writer
    /// goes away. A disconnected channel means the thread already finished
    /// (sending is its last act), so it is joined too.
    fn join_within(self, grace: Duration) {
        match self.output.recv_timeout(grace) {
            Ok(_) | Err(RecvTimeoutError::Disconnected) => {
                let _ = self.thread.join();
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Stops the child and everything it spawned: SIGTERM first so Git can
/// remove its lock files, SIGKILL for whatever is still there afterwards.
/// When the group cannot be signalled at all, the leader is killed directly
/// so the caller is never left with a live `git`.
fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let group_reached = signal_process_group(child.id(), "TERM");
        if !group_reached {
            let _ = child.kill();
        }
        let grace_deadline = Instant::now() + TERMINATION_GRACE;
        while Instant::now() < grace_deadline && !matches!(child.try_wait(), Ok(Some(_))) {
            thread::sleep(CHILD_POLL_INTERVAL);
        }
        // Helpers may outlive the leader and either process may ignore
        // SIGTERM; the group id stays valid while any member remains, and
        // signalling an empty group is harmless.
        if !signal_process_group(child.id(), "KILL") {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

/// The child is its own group leader, so its pid doubles as the group id.
/// `kill(1)` does the signalling because this crate forbids unsafe code and
/// therefore cannot call `killpg` directly; `/bin/kill` is tried first and
/// whichever `kill` is on `PATH` second.
#[cfg(unix)]
fn signal_process_group(group: u32, signal: &str) -> bool {
    ["/bin/kill", "kill"].iter().any(|program| {
        Command::new(program)
            .arg(format!("-{signal}"))
            .arg("--")
            .arg(format!("-{group}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
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
                change("collections/api/requests/drafts/wip.toml", None),
                change("collections/api/README.md", None),
                change("environments/secrets/prod.toml", None),
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

    /// Runs `script` under the network timeout and checks that the command
    /// timed out promptly and that the helper whose pid the script wrote to
    /// `$0` is gone afterwards.
    #[cfg(unix)]
    fn assert_timeout_ends_helper(script: &str) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let pid_file = directory.path().join("helper.pid");
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).arg(&pid_file);
        let started = Instant::now();

        let error = run_command(command, Some(Duration::from_millis(200)))
            .expect_err("command must time out");

        assert_eq!(error.kind, GitErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timeout took {:?}",
            started.elapsed()
        );
        let helper = fs::read_to_string(&pid_file).expect("helper pid file");
        let helper = helper.trim();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let alive = Command::new("kill")
                .args(["-0", helper])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "helper process {helper} survived the timeout"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_timed_out_command_takes_its_helper_processes_with_it() {
        // A stand-in for `git push` spawning ssh: the helper inherits the
        // output pipe and would otherwise keep it, and itself, alive.
        assert_timeout_ends_helper("sleep 30 & echo $! > \"$0\"; wait");
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_ignores_sigterm_is_still_killed_after_the_grace_period() {
        // Ignoring TERM before forking makes the helper inherit that
        // disposition, so only the KILL escalation can end either process.
        assert_timeout_ends_helper("trap '' TERM; sleep 30 & echo $! > \"$0\"; wait");
    }

    #[test]
    fn a_drain_whose_sender_is_gone_is_joined_not_detached() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        // The channel disconnects as soon as the sender drops, well before
        // the thread exits; only a real join observes the exit.
        let (sender, output) = mpsc::channel::<Vec<u8>>();
        let exited = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let exited = Arc::clone(&exited);
            move || {
                drop(sender);
                thread::sleep(Duration::from_millis(200));
                exited.store(true, Ordering::SeqCst);
            }
        });

        Drain { output, thread }.join_within(Duration::from_secs(5));

        assert!(
            exited.load(Ordering::SeqCst),
            "a disconnected channel must still join the drain thread"
        );
    }

    #[test]
    fn a_drain_that_outlives_the_grace_is_detached() {
        let (_sender, output) = mpsc::channel::<Vec<u8>>();
        let (release, hold) = mpsc::channel::<()>();
        let stuck = Drain {
            output,
            thread: thread::spawn(move || {
                let _ = hold.recv();
            }),
        };
        let started = Instant::now();

        stuck.join_within(Duration::from_millis(50));

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "join_within blocked on a thread that will not finish: {:?}",
            started.elapsed()
        );
        drop(release);
    }

    #[cfg(unix)]
    #[test]
    fn a_timed_out_command_returns_once_its_pipes_are_closed() {
        // The helper keeps stdout open; the timeout path must still return
        // after the tree is gone rather than waiting on the drain forever.
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 30 & echo started; wait");
        let started = Instant::now();

        let error = run_command(command, Some(Duration::from_millis(100)))
            .expect_err("command must time out");

        assert_eq!(error.kind, GitErrorKind::TimedOut);
        assert!(
            started.elapsed() < TERMINATION_GRACE + DRAIN_GRACE + Duration::from_secs(1),
            "timeout path took {:?}",
            started.elapsed()
        );
    }
}
