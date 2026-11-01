//! Persisted presentation migrations and Git diagnostics shared by interfaces.
use super::io::{WorkspaceGitDelta, WorkspaceGitOutcome};
use crate::domain::{DomainBody, request::PushAvailability};

#[must_use]
pub fn response_section(value: String) -> String {
    if value == "Raw" {
        "Raw HTTP".into()
    } else {
        value
    }
}
#[must_use]
pub fn response_renderer(value: String) -> String {
    if value == "Raw" { "Text".into() } else { value }
}
#[must_use]
pub fn request_section(body: &DomainBody) -> String {
    if *body == DomainBody::Empty {
        "Params"
    } else {
        "Body"
    }
    .into()
}
#[must_use]
pub fn git_porcelain(
    staged: WorkspaceGitDelta,
    unstaged: WorkspaceGitDelta,
    conflicted: bool,
) -> String {
    let code = |delta| match delta {
        WorkspaceGitDelta::None => '·',
        WorkspaceGitDelta::Added => 'A',
        WorkspaceGitDelta::Modified => 'M',
        WorkspaceGitDelta::Deleted => 'D',
        WorkspaceGitDelta::Renamed => 'R',
        WorkspaceGitDelta::Copied => 'C',
        WorkspaceGitDelta::TypeChanged => 'T',
        WorkspaceGitDelta::Untracked => '?',
        WorkspaceGitDelta::Unmerged => 'U',
    };
    format!(
        "Git status: {}",
        if conflicted {
            "UU".into()
        } else {
            format!("{}{}", code(staged), code(unstaged))
        }
    )
}
#[must_use]
pub fn git_change_kind(
    staged: WorkspaceGitDelta,
    unstaged: WorkspaceGitDelta,
    conflicted: bool,
) -> String {
    let name = |delta| match delta {
        WorkspaceGitDelta::None => "none",
        WorkspaceGitDelta::Added => "added",
        WorkspaceGitDelta::Modified => "modified",
        WorkspaceGitDelta::Deleted => "deleted",
        WorkspaceGitDelta::Renamed => "renamed",
        WorkspaceGitDelta::Copied => "copied",
        WorkspaceGitDelta::TypeChanged => "type_changed",
        WorkspaceGitDelta::Untracked => "untracked",
        WorkspaceGitDelta::Unmerged => "unmerged",
    };
    crate::domain::request::git_change_kind(name(staged), name(unstaged), conflicted)
}
#[must_use]
pub fn git_operation_label(outcome: WorkspaceGitOutcome) -> String {
    match outcome {
        WorkspaceGitOutcome::NothingToCommit => "Nothing to commit",
        WorkspaceGitOutcome::Committed => "Workspace documents committed",
        WorkspaceGitOutcome::Updated => "Workspace updated from upstream",
        WorkspaceGitOutcome::UpToDate => "Already up to date",
        WorkspaceGitOutcome::Pushed => "Current branch pushed",
        WorkspaceGitOutcome::Conflicted => "Pull stopped with conflicts",
        WorkspaceGitOutcome::MergeAborted => "Merge aborted; workspace restored to the last commit",
    }
    .into()
}
#[must_use]
pub fn git_push_help(availability: PushAvailability) -> String {
    match availability {
        PushAvailability::Push => "Push committed workspace documents to the upstream.".into(),
        PushAvailability::Publish { branch } => {
            format!("No upstream configured. Push {branch} to origin and track it as the upstream.")
        }
        PushAvailability::Unavailable { reason } => reason,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_and_default_panels_preserve_saved_behavior() {
        assert_eq!(response_section("Raw".into()), "Raw HTTP");
        assert_eq!(response_renderer("Raw".into()), "Text");
        assert_eq!(response_renderer("JSON".into()), "JSON");
        assert_eq!(request_section(&DomainBody::Empty), "Params");
        assert_eq!(
            request_section(&DomainBody::Json { value: "{}".into() }),
            "Body"
        );
    }
    #[test]
    fn git_labels_keep_porcelain_and_operation_text() {
        use WorkspaceGitDelta as D;
        for (delta, code) in [
            (D::None, '·'),
            (D::Added, 'A'),
            (D::Modified, 'M'),
            (D::Deleted, 'D'),
            (D::Renamed, 'R'),
            (D::Copied, 'C'),
            (D::TypeChanged, 'T'),
            (D::Untracked, '?'),
            (D::Unmerged, 'U'),
        ] {
            assert_eq!(
                git_porcelain(delta, D::None, false),
                format!("Git status: {code}·")
            );
        }
        assert_eq!(git_porcelain(D::Added, D::Deleted, true), "Git status: UU");
        assert_eq!(
            git_operation_label(WorkspaceGitOutcome::MergeAborted),
            "Merge aborted; workspace restored to the last commit"
        );
        assert_eq!(
            git_push_help(PushAvailability::Publish {
                branch: "main".into()
            }),
            "No upstream configured. Push main to origin and track it as the upstream."
        );
    }
}
