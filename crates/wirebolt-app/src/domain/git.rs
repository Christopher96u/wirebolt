//! Typed Git change labels and branch push availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainGitChangeKind {
    New,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflicted,
}
impl DomainGitChangeKind {
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::New => "New",
            Self::Modified => "Modified",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Copied => "Copied",
            Self::TypeChanged => "Type Changed",
            Self::Conflicted => "Conflict",
        }
        .into()
    }
}
#[must_use]
pub fn git_change_kind(
    staged: wirebolt_core::GitDelta,
    unstaged: wirebolt_core::GitDelta,
    conflicted: bool,
) -> DomainGitChangeKind {
    use DomainGitChangeKind as K;
    use wirebolt_core::GitDelta as D;
    let deltas = [staged, unstaged];
    if conflicted || deltas.contains(&D::Unmerged) {
        return K::Conflicted;
    }
    for (choices, kind) in [
        (&[D::Added, D::Untracked][..], K::New),
        (&[D::Deleted][..], K::Deleted),
        (&[D::Renamed][..], K::Renamed),
        (&[D::Copied][..], K::Copied),
        (&[D::TypeChanged][..], K::TypeChanged),
    ] {
        if deltas.iter().any(|delta| choices.contains(delta)) {
            return kind;
        }
    }
    K::Modified
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushAvailability {
    Push,
    Publish { branch: String },
    Unavailable { reason: String },
}
#[must_use]
pub fn push_availability(
    branch: Option<String>,
    upstream: Option<&str>,
    ahead: u64,
) -> PushAvailability {
    let Some(branch) = branch else {
        return PushAvailability::Unavailable {
            reason: "Check out a branch to push. HEAD is detached.".into(),
        };
    };
    if upstream.is_none() {
        return PushAvailability::Publish { branch };
    }
    if ahead == 0 {
        return PushAvailability::Unavailable {
            reason: "No local commits to push.".into(),
        };
    }
    PushAvailability::Push
}
