//! Watch directories so atomic file replacements do not detach the watch.

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub(crate) struct FileWatch {
    watcher: RecommendedWatcher,
    directories: HashSet<PathBuf>,
    targets: Arc<Mutex<HashSet<PathBuf>>>,
}

impl FileWatch {
    pub fn new() -> Result<(Self, async_channel::Receiver<()>), String> {
        let (sender, receiver) = async_channel::bounded(1);
        let targets = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
        let observed = Arc::clone(&targets);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let relevant = match event {
                Ok(event) if event.kind.is_access() => false,
                Ok(event) => observed.lock().is_ok_and(|paths| {
                    event
                        .paths
                        .iter()
                        .any(|event_path| paths.iter().any(|path| path.starts_with(event_path)))
                }),
                Err(_) => true,
            };
            if relevant {
                // Coalesce bursts without dropping the need to re-read current state.
                let _ = sender.try_send(());
            }
        })
        .map_err(|error| error.to_string())?;

        Ok((
            Self {
                watcher,
                directories: HashSet::new(),
                targets,
            },
            receiver,
        ))
    }

    pub fn set_paths(&mut self, paths: HashSet<PathBuf>) -> Result<(), String> {
        let directories: HashSet<_> = paths
            .iter()
            .filter_map(|path| path.parent().map(PathBuf::from))
            .collect();
        *self.targets.lock().map_err(|error| error.to_string())? = paths;

        for removed in self.directories.difference(&directories) {
            let _ = self.watcher.unwatch(removed);
        }
        self.directories.retain(|path| directories.contains(path));
        // Re-arm on activation even for known paths: removing/recreating a
        // directory can invalidate its native watch without changing its name.
        for directory in &directories {
            self.watcher
                .watch(directory, RecursiveMode::NonRecursive)
                .map_err(|error| error.to_string())?;
        }
        self.directories = directories;

        Ok(())
    }
}
