use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;
use wirebolt_core::{GitChange, GitDelta, GitErrorKind, GitOperationOutcome, GitWorkspace};

#[test]
fn status_reports_branch_staged_unstaged_and_untracked_changes() {
    let repository = TestRepository::new();
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.commit_all("initial workspace");

    repository.write("wirebolt.toml", "name = \"After\"\n");
    repository.git_ok(["add", "wirebolt.toml"]);
    repository.write("collections/api/requests/list.toml", "method = \"GET\"\n");
    repository.write("notes.txt", "not managed by Wirebolt\n");

    let status = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .status()
        .expect("read Git status");

    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.upstream, None);
    assert_eq!((status.ahead, status.behind), (0, 0));
    assert_eq!(
        status.changes,
        [
            GitChange {
                path: "collections/api/requests/list.toml".to_owned(),
                previous_path: None,
                staged: GitDelta::None,
                unstaged: GitDelta::Untracked,
                conflicted: false,
            },
            GitChange {
                path: "notes.txt".to_owned(),
                previous_path: None,
                staged: GitDelta::None,
                unstaged: GitDelta::Untracked,
                conflicted: false,
            },
            GitChange {
                path: "wirebolt.toml".to_owned(),
                previous_path: None,
                staged: GitDelta::Modified,
                unstaged: GitDelta::None,
                conflicted: false,
            },
        ]
    );
}

#[test]
fn commit_includes_only_wirebolt_managed_documents() {
    let repository = TestRepository::new();
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.write("notes.txt", "before\n");
    repository.commit_all("initial workspace");

    repository.write("wirebolt.toml", "name = \"After\"\n");
    repository.write("environments/dev.toml", "name = \"Dev\"\n");
    repository.write("notes.txt", "after\n");
    repository.git_ok(["add", "notes.txt"]);

    let result = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .commit("save workspace")
        .expect("commit workspace");

    assert_eq!(result.outcome, GitOperationOutcome::Committed);
    assert_eq!(
        repository.git_text(["log", "-1", "--format=%s"]),
        "save workspace"
    );
    assert_eq!(
        repository.git_lines(["show", "--format=", "--name-only", "HEAD"]),
        ["environments/dev.toml", "wirebolt.toml"]
    );
    assert!(result.status.changes.iter().any(|change| {
        change.path == "notes.txt"
            && change.staged == GitDelta::Modified
            && change.unstaged == GitDelta::None
    }));
}

#[test]
fn push_sets_up_origin_and_publishes_the_current_branch() {
    let remote = BareRepository::new();
    let repository = TestRepository::new();
    repository.add_origin(remote.path());
    repository.write("wirebolt.toml", "name = \"Shared\"\n");
    repository.commit_all("initial workspace");

    let result = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .push()
        .expect("push workspace");

    assert_eq!(result.outcome, GitOperationOutcome::Pushed);
    assert_eq!(result.status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((result.status.ahead, result.status.behind), (0, 0));
    assert_eq!(
        remote.git_text(["show", "main:wirebolt.toml"]),
        "name = \"Shared\""
    );
}

#[test]
fn pull_updates_a_clean_workspace_from_its_upstream() {
    let remote = BareRepository::new();
    let repository = TestRepository::new();
    repository.add_origin(remote.path());
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.commit_all("initial workspace");
    repository.git_ok(["push", "--set-upstream", "origin", "main"]);

    let collaborator = TestRepository::clone_from(remote.path());
    collaborator.write("wirebolt.toml", "name = \"After\"\n");
    collaborator.commit_all("update workspace");
    collaborator.git_ok(["push"]);

    let result = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .pull()
        .expect("pull workspace");

    assert_eq!(result.outcome, GitOperationOutcome::Updated);
    assert_eq!(repository.read("wirebolt.toml"), "name = \"After\"\n");
    assert!(result.status.is_clean());
}

#[test]
fn pull_surfaces_conflicts_without_resolving_them() {
    let remote = BareRepository::new();
    let repository = TestRepository::new();
    repository.add_origin(remote.path());
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.commit_all("initial workspace");
    repository.git_ok(["push", "--set-upstream", "origin", "main"]);

    let collaborator = TestRepository::clone_from(remote.path());
    collaborator.write("wirebolt.toml", "name = \"Remote\"\n");
    collaborator.commit_all("remote edit");
    collaborator.git_ok(["push"]);
    repository.write("wirebolt.toml", "name = \"Local\"\n");
    repository.commit_all("local edit");

    let result = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .pull()
        .expect("surface pull conflict");

    assert_eq!(result.outcome, GitOperationOutcome::Conflicted);
    assert_eq!(
        result
            .status
            .conflicts()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        ["wirebolt.toml"]
    );
    let contents = repository.read("wirebolt.toml");
    assert!(contents.contains("<<<<<<< HEAD"));
    assert!(contents.contains("name = \"Local\""));
    assert!(contents.contains("name = \"Remote\""));
}

#[test]
fn pull_refuses_to_overwrite_uncommitted_work() {
    let remote = BareRepository::new();
    let repository = TestRepository::new();
    repository.add_origin(remote.path());
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.commit_all("initial workspace");
    repository.git_ok(["push", "--set-upstream", "origin", "main"]);
    repository.write("wirebolt.toml", "name = \"Dirty\"\n");

    let error = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .pull()
        .expect_err("dirty pull must fail");

    assert_eq!(error.kind, GitErrorKind::DirtyWorkspace);
    assert_eq!(repository.read("wirebolt.toml"), "name = \"Dirty\"\n");
}

#[test]
fn commit_reports_a_clean_workspace_without_creating_a_commit() {
    let repository = TestRepository::new();
    repository.write("wirebolt.toml", "name = \"Clean\"\n");
    repository.commit_all("initial workspace");
    let before = repository.git_text(["rev-parse", "HEAD"]);

    let result = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .commit("nothing changed")
        .expect("clean commit result");

    assert_eq!(result.outcome, GitOperationOutcome::NothingToCommit);
    assert_eq!(repository.git_text(["rev-parse", "HEAD"]), before);
}

#[test]
fn commit_rejects_an_empty_message_without_staging_files() {
    let repository = TestRepository::new();
    repository.write("wirebolt.toml", "name = \"Before\"\n");
    repository.commit_all("initial workspace");
    repository.write("wirebolt.toml", "name = \"After\"\n");

    let error = GitWorkspace::open(repository.path())
        .expect("open Git workspace")
        .commit("   ")
        .expect_err("empty message must fail");

    assert_eq!(error.kind, GitErrorKind::InvalidCommitMessage);
    assert_eq!(
        repository.git_text(["status", "--porcelain=v1"]),
        "M wirebolt.toml"
    );
}

#[test]
fn workspace_must_be_the_repository_root() {
    let repository = TestRepository::new();
    repository.write("nested/wirebolt.toml", "name = \"Nested\"\n");

    let error = GitWorkspace::open(repository.path().join("nested"))
        .expect_err("nested workspace must be rejected");

    assert_eq!(error.kind, GitErrorKind::WorkspaceNotRepositoryRoot);
}

struct TestRepository {
    _directory: TempDir,
    root: std::path::PathBuf,
}

impl TestRepository {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary repository");
        let root = directory.path().to_owned();
        let repository = Self {
            _directory: directory,
            root,
        };
        repository.git_ok(["init", "-b", "main"]);
        repository.configure_identity();
        repository
    }

    fn clone_from(remote: &Path) -> Self {
        let directory = tempfile::tempdir().expect("temporary clone parent");
        let root = directory.path().join("workspace");
        let output = Command::new("git")
            .args(["clone", remote.to_string_lossy().as_ref()])
            .arg(&root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .output()
            .expect("clone repository");
        assert!(
            output.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let repository = Self {
            _directory: directory,
            root,
        };
        repository.configure_identity();
        repository
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, relative_path: &str, contents: &str) {
        let path = self.path().join(relative_path);
        fs::create_dir_all(path.parent().expect("file parent")).expect("create parent");
        fs::write(path, contents).expect("write repository file");
    }

    fn read(&self, relative_path: &str) -> String {
        fs::read_to_string(self.path().join(relative_path)).expect("read repository file")
    }

    fn add_origin(&self, remote: &Path) {
        let remote = remote.to_string_lossy();
        self.git_ok(["remote", "add", "origin", remote.as_ref()]);
    }

    fn configure_identity(&self) {
        self.git_ok(["config", "user.name", "Wirebolt Tests"]);
        self.git_ok(["config", "user.email", "wirebolt@example.invalid"]);
    }

    fn commit_all(&self, message: &str) {
        self.git_ok(["add", "--all"]);
        self.git_ok(["commit", "-m", message]);
    }

    fn git_ok<const N: usize>(&self, arguments: [&str; N]) -> Output {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(arguments)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .output()
            .expect("run Git");
        assert!(
            output.status.success(),
            "Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn git_text<const N: usize>(&self, arguments: [&str; N]) -> String {
        String::from_utf8(self.git_ok(arguments).stdout)
            .expect("Git UTF-8 output")
            .trim()
            .to_owned()
    }

    fn git_lines<const N: usize>(&self, arguments: [&str; N]) -> Vec<String> {
        self.git_text(arguments)
            .lines()
            .map(ToOwned::to_owned)
            .collect()
    }
}

struct BareRepository {
    directory: TempDir,
}

impl BareRepository {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary bare repository");
        let output = Command::new("git")
            .arg("-C")
            .arg(directory.path())
            .args(["init", "--bare", "-b", "main"])
            .env("LC_ALL", "C")
            .output()
            .expect("initialize bare repository");
        assert!(output.status.success());
        Self { directory }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn git_text<const N: usize>(&self, arguments: [&str; N]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(arguments)
            .env("LC_ALL", "C")
            .output()
            .expect("run Git in bare repository");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("Git UTF-8 output")
            .trim()
            .to_owned()
    }
}
