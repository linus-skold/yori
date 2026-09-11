//! Comparison identity and tab lifetime, independent of rendering.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FilePair {
    pub left: PathBuf,
    pub right: PathBuf,
}

impl FilePair {
    pub fn resolve(left: &Path, right: &Path) -> Result<Self, String> {
        let resolve = |path: &Path| {
            path.canonicalize()
                .map_err(|error| format!("cannot open {}: {error}", path.display()))
        };

        Ok(Self {
            left: resolve(left)?,
            right: resolve(right)?,
        })
    }

    pub fn description(&self) -> String {
        format!(
            "Baseline: {}\nLocal: {}",
            self.left.display(),
            self.right.display()
        )
    }
}

pub(super) struct Tab<T> {
    pub id: usize,
    pub pair: FilePair,
    pub content: T,
}

pub(super) struct Tabs<T> {
    pub entries: Vec<Tab<T>>,
    pub active: Option<usize>,
    next_id: usize,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            active: None,
            next_id: 0,
        }
    }
}

impl<T> Tabs<T> {
    pub fn find(&self, pair: &FilePair) -> Option<usize> {
        self.entries
            .iter()
            .find(|tab| &tab.pair == pair)
            .map(|tab| tab.id)
    }

    pub fn get(&self, id: usize) -> Option<&Tab<T>> {
        self.entries.iter().find(|tab| tab.id == id)
    }

    pub fn activate(&mut self, id: usize) {
        if self.get(id).is_some() {
            self.active = Some(id);
        }
    }

    /// Duplicate opens preserve the existing content rather than replacing it.
    pub fn insert(&mut self, pair: FilePair, content: T) -> usize {
        if let Some(id) = self.find(&pair) {
            self.activate(id);
            return id;
        }

        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Tab { id, pair, content });
        self.active = Some(id);

        id
    }

    /// Closing the active tab chooses its right neighbor, then its left neighbor.
    pub fn remove(&mut self, id: usize) {
        let Some(index) = self.entries.iter().position(|tab| tab.id == id) else {
            return;
        };

        self.entries.remove(index);
        if self.active == Some(id) {
            self.active = self
                .entries
                .get(index)
                .or_else(|| self.entries.last())
                .map(|tab| tab.id);
        }
    }

    pub fn requires_discard_confirmation(
        &self,
        target: Option<usize>,
        modified: impl Fn(&T) -> bool,
    ) -> bool {
        match target {
            Some(id) => self.get(id).is_some_and(|tab| modified(&tab.content)),
            None => self.entries.iter().any(|tab| modified(&tab.content)),
        }
    }

    pub fn label(&self, id: usize) -> String {
        let tab = self.get(id).expect("label requested for an existing tab");
        let path = &tab.pair.right;
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let matching_names = self
            .entries
            .iter()
            .filter(|other| other.pair.right.file_name() == path.file_name())
            .count();
        if matching_names == 1 {
            return name.into_owned();
        }

        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let label = format!("{name} — {}", parent.display());
        let matching_locals = self
            .entries
            .iter()
            .filter(|other| other.pair.right == *path)
            .count();
        if matching_locals > 1 {
            format!("{label} ← {}", tab.pair.left.display())
        } else {
            label
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(left: &str, right: &str) -> FilePair {
        FilePair {
            left: left.into(),
            right: right.into(),
        }
    }

    #[test]
    fn switching_and_duplicate_opens_preserve_independent_content() {
        let mut tabs = Tabs::default();
        let first = tabs.insert(pair("/base/a", "/local/a"), String::from("edited a"));
        let second = tabs.insert(pair("/base/b", "/local/b"), String::from("edited b"));

        tabs.activate(first);
        let duplicate = tabs.insert(pair("/base/b", "/local/b"), String::from("disk b"));

        assert_eq!(duplicate, second);
        assert_eq!(tabs.active, Some(second));
        assert_eq!(tabs.entries.len(), 2);
        assert_eq!(tabs.get(first).unwrap().content, "edited a");
        assert_eq!(tabs.get(second).unwrap().content, "edited b");
    }

    #[test]
    fn closing_tabs_keeps_stable_identity_and_selects_a_neighbor() {
        let mut tabs = Tabs::default();
        let a = tabs.insert(pair("a", "a"), ());
        let b = tabs.insert(pair("b", "b"), ());
        let c = tabs.insert(pair("c", "c"), ());
        tabs.activate(b);

        tabs.remove(a);
        assert_eq!(tabs.active, Some(b));

        tabs.remove(b);
        assert_eq!(tabs.active, Some(c));

        tabs.remove(c);
        assert_eq!(tabs.active, None);
        assert!(tabs.entries.is_empty());

        let next = tabs.insert(pair("d", "d"), ());
        assert!(next > c);
        tabs.remove(b);
        assert_eq!(tabs.active, Some(next));
    }

    #[test]
    fn closing_the_window_checks_inactive_tabs_and_cancellation_keeps_them() {
        let mut tabs = Tabs::default();
        let modified = tabs.insert(pair("a", "a"), true);
        let clean = tabs.insert(pair("b", "b"), false);

        assert_eq!(tabs.active, Some(clean));
        assert!(!tabs.requires_discard_confirmation(Some(clean), |dirty| *dirty));
        assert!(tabs.requires_discard_confirmation(Some(modified), |dirty| *dirty));
        assert!(tabs.requires_discard_confirmation(None, |dirty| *dirty));
        assert_eq!(tabs.entries.len(), 2);
        assert!(tabs.get(modified).unwrap().content);

        // Only the confirmed close removes the specific tab, not the active one.
        tabs.remove(modified);
        assert_eq!(tabs.active, Some(clean));
        assert!(!tabs.requires_discard_confirmation(None, |dirty| *dirty));
    }

    #[test]
    fn closing_the_last_active_tab_selects_its_left_neighbor() {
        let mut tabs = Tabs::default();
        let first = tabs.insert(pair("a", "a"), ());
        let last = tabs.insert(pair("b", "b"), ());

        tabs.remove(last);

        assert_eq!(tabs.active, Some(first));
    }

    #[test]
    fn labels_disambiguate_filenames_and_multiple_baselines() {
        let mut tabs = Tabs::default();
        let first = tabs.insert(pair("/base/one", "/one/parser.rs"), ());
        assert_eq!(tabs.label(first), "parser.rs");

        let second = tabs.insert(pair("/base/two", "/two/parser.rs"), ());
        assert_eq!(tabs.label(first), "parser.rs — /one");
        assert_eq!(tabs.label(second), "parser.rs — /two");

        tabs.insert(pair("/base/three", "/one/parser.rs"), ());
        assert_eq!(tabs.label(first), "parser.rs — /one ← /base/one");
        assert_ne!(
            tabs.find(&pair("/base/one", "/one/parser.rs")),
            tabs.find(&pair("/one/parser.rs", "/base/one"))
        );
    }

    #[test]
    fn path_aliases_resolve_to_the_same_ordered_pair() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/before.rs");
        let alias = file.parent().unwrap().join("./before.rs");

        assert_eq!(
            FilePair::resolve(&file, &file).unwrap(),
            FilePair::resolve(&alias, &file).unwrap()
        );
    }
}
