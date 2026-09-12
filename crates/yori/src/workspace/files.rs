//! Per-tab disk versions: dismissing a notice never approves an overwrite.

use crate::{comparison::ComparisonPaths, storage::Snapshot};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Baseline,
    Local,
    Base,
    Incoming,
    Result,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Self::Baseline => "Baseline",
            Self::Local => "Local",
            Self::Base => "Base",
            Self::Incoming => "Incoming",
            Self::Result => "Result",
        }
    }
}

pub(super) struct TrackedFile {
    pub role: Role,
    pub path: PathBuf,
    pub accepted: Snapshot,
    pub current: Result<Snapshot, String>,
    dismissed: Option<Result<Snapshot, String>>,
}

pub(super) struct Files {
    pub entries: Vec<TrackedFile>,
}

impl Files {
    pub fn load(paths: &ComparisonPaths) -> Result<Self, String> {
        let roles = match paths {
            ComparisonPaths::Diff { .. } => vec![Role::Baseline, Role::Local],
            ComparisonPaths::Merge(_) => {
                vec![Role::Base, Role::Local, Role::Incoming, Role::Result]
            }
        };
        let entries = roles
            .into_iter()
            .zip(paths.paths())
            .map(|(role, path)| {
                let accepted = Snapshot::read(path)?;
                if role != Role::Result {
                    accepted.document(path)?;
                }

                Ok(TrackedFile {
                    role,
                    path: path.to_owned(),
                    current: Ok(accepted.clone()),
                    accepted,
                    dismissed: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        Ok(Self { entries })
    }

    pub fn file(&self, role: Role) -> &TrackedFile {
        self.entries
            .iter()
            .find(|file| file.role == role)
            .expect("role belongs to comparison")
    }

    pub fn notice(&self) -> Option<&TrackedFile> {
        self.entries.iter().find(|file| {
            let missing_input = file.role != self.target().role
                && file.current.as_ref().is_ok_and(Snapshot::is_missing);
            !missing_input
                && file.current != Ok(file.accepted.clone())
                && file.dismissed.as_ref() != Some(&file.current)
        })
    }

    pub fn target(&self) -> &TrackedFile {
        self.entries.last().expect("comparison has a destination")
    }

    pub fn dismiss(&mut self, role: Role, observed: Result<Snapshot, String>) {
        if let Some(file) = self.entries.iter_mut().find(|file| file.role == role) {
            // A newer change while the dialog was open still needs its own decision.
            file.dismissed = Some(observed);
        }
    }

    pub fn accept(&mut self, role: Role, snapshot: Snapshot) {
        if let Some(file) = self.entries.iter_mut().find(|file| file.role == role) {
            file.accepted = snapshot.clone();
            file.current = Ok(snapshot);
            file.dismissed = None;
        }
    }

    pub fn saved(&mut self, snapshot: &Snapshot) {
        let path = self.target().path.clone();
        // If RESULT aliases an input, keep that input's immutable editor snapshot
        // but don't report our own save as somebody else's incoming change.
        for file in &mut self.entries {
            if file.path == path {
                file.accepted = snapshot.clone();
                file.current = Ok(snapshot.clone());
                file.dismissed = None;
            }
        }
    }
}
