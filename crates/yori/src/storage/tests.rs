use super::*;
use std::{path::PathBuf, time::Duration};

#[test]
fn atomic_save_preserves_source_bytes_permissions_attributes_and_old_open_handles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "old\r\n").unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o754)).unwrap();
    let mut old_handle = File::open(&path).unwrap();
    rustix::fs::fsetxattr(&old_handle, "user.yori-test", b"keep", XattrFlags::empty()).unwrap();
    let expected = Snapshot::read(&path).unwrap();
    let bytes = "\t新\r\nlast line".as_bytes();

    let saved = save(&path, &expected, bytes).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(saved, Snapshot::read(&path).unwrap());
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o7777, 0o754);
    assert_eq!(
        read_attributes(&File::open(&path).unwrap()).unwrap(),
        vec![(CString::new("user.yori-test").unwrap(), b"keep".to_vec())]
    );
    let mut old = String::new();
    old_handle.read_to_string(&mut old).unwrap();
    assert_eq!(
        old, "old\r\n",
        "replacement must not truncate the old inode"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn external_edits_deletions_and_new_destinations_need_exact_approval() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("result.rs");
    let missing = Snapshot::read(&path).unwrap();
    std::fs::write(&path, "external").unwrap();

    assert!(matches!(
        save(&path, &missing, b"ours"),
        Err(SaveError::Changed(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"external");
    let approved = Snapshot::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        save(&path, &approved, b"ours"),
        Err(SaveError::Changed(Snapshot::Missing))
    ));

    save(&path, &Snapshot::Missing, b"ours").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"ours");
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
}

#[test]
fn edit_during_staging_does_not_overwrite_disk_and_cleans_up_temporary_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.rs");
    std::fs::write(&path, "before").unwrap();
    let expected = Snapshot::read(&path).unwrap();

    let result = save_with_before_replace(&path, &expected, b"ours", || {
        std::fs::write(&path, "external").unwrap();
    });

    assert!(matches!(result, Err(SaveError::Changed(_))));
    assert_eq!(std::fs::read(&path).unwrap(), b"external");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn readonly_hardlinked_and_retargeted_paths_are_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.rs");
    std::fs::write(&path, "before").unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o444)).unwrap();
    let snapshot = Snapshot::read(&path).unwrap();
    assert!(matches!(
        save(&path, &snapshot, b"ours"),
        Err(SaveError::Failed(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"before");

    std::fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
    let alias = directory.path().join("alias.rs");
    std::fs::hard_link(&path, &alias).unwrap();
    let snapshot = Snapshot::read(&path).unwrap();
    assert!(matches!(
        save(&path, &snapshot, b"ours"),
        Err(SaveError::Failed(_))
    ));
    assert_eq!(std::fs::read(&alias).unwrap(), b"before");

    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&alias, &path).unwrap();
    assert!(Snapshot::read(&path).is_err());
    assert!(save(&path, &snapshot, b"ours").is_err());
    assert!(path.is_symlink());
    assert_eq!(std::fs::read(&alias).unwrap(), b"before");
}

#[test]
fn directory_watcher_survives_replacement_deletion_and_recreation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local.rs");
    std::fs::write(&path, "initial").unwrap();
    let (mut watch, events) = FileWatch::new().unwrap();
    watch.set_paths([path.clone()].into()).unwrap();

    for value in [Some("replaced"), None, Some("recreated")] {
        while events.try_recv().is_ok() {}
        let (done, completion) = std::sync::mpsc::channel();
        let events = events.clone();
        let worker = std::thread::spawn(move || {
            done.send(events.recv_blocking()).unwrap();
        });
        if let Some(value) = value {
            let temporary = directory.path().join("replacement");
            std::fs::write(&temporary, value).unwrap();
            std::fs::rename(&temporary, &path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }

        completion
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(Snapshot::read(&path).unwrap().is_missing(), value.is_none());
    }
    watch
        .set_paths(std::collections::HashSet::<PathBuf>::new())
        .unwrap();
}
