//! Source-faithful file snapshots and guarded atomic replacement, independent of GPUI.

mod watch;
pub(crate) use watch::FileWatch;

#[cfg(test)]
mod tests;

use rustix::fs::{OFlags, XattrFlags};
use std::{
    ffi::CString,
    fs::{File, Metadata, OpenOptions, Permissions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Arc,
};
use yori_document::Document;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    links: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl From<&Metadata> for Identity {
    fn from(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            mode: meta.mode(),
            uid: meta.uid(),
            gid: meta.gid(),
            links: meta.nlink(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileVersion {
    bytes: Arc<[u8]>,
    identity: Identity,
    attributes: Vec<(CString, Vec<u8>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Snapshot {
    Missing,
    File(FileVersion),
}

impl Snapshot {
    pub fn read(path: &Path) -> Result<Self, String> {
        read_snapshot(path).map_err(|error| format!("cannot inspect {}: {error}", path.display()))
    }

    pub fn document(&self, path: &Path) -> Result<Document, String> {
        let Self::File(file) = self else {
            return Err(format!("file no longer exists: {}", path.display()));
        };

        Document::from_bytes(file.bytes.to_vec())
            .map_err(|error| format!("cannot open {}: {error}", path.display()))
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

fn read_snapshot(path: &Path) -> Result<Snapshot, String> {
    // Workspace paths are canonical when opened. Don't follow a symlink that
    // someone subsequently puts at that target; a save must not replace it either.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits().cast_signed())
        .open(path);
    let mut file = match file {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::Missing),
        Err(error) => return Err(error.to_string()),
    };
    let before = file.metadata().map_err(|error| error.to_string())?;
    if !before.is_file() {
        return Err("not a regular file".into());
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let attributes = read_attributes(&file)?;
    let after = file.metadata().map_err(|error| error.to_string())?;
    if Identity::from(&before) != Identity::from(&after) || after.len() != bytes.len() as u64 {
        return Err("file changed while it was being read; retry".into());
    }

    Ok(Snapshot::File(FileVersion {
        bytes: bytes.into(),
        identity: Identity::from(&after),
        attributes,
    }))
}

fn read_attributes(file: &File) -> Result<Vec<(CString, Vec<u8>)>, String> {
    let length = match rustix::fs::flistxattr(file, &mut [] as &mut [u8]) {
        Ok(length) => length,
        Err(rustix::io::Errno::NOTSUP) => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut names = vec![0; length];
    let length =
        rustix::fs::flistxattr(file, names.as_mut_slice()).map_err(|error| error.to_string())?;
    let mut attributes = Vec::new();
    for name in names[..length]
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        let length = rustix::fs::fgetxattr(file, &name, &mut [] as &mut [u8])
            .map_err(|error| error.to_string())?;
        let mut value = vec![0; length];
        let length = rustix::fs::fgetxattr(file, &name, value.as_mut_slice())
            .map_err(|error| error.to_string())?;
        value.truncate(length);
        attributes.push((name, value));
    }
    attributes.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(attributes)
}

#[derive(Debug)]
pub(crate) enum SaveError {
    Changed(Snapshot),
    Failed(String),
}

impl From<String> for SaveError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// Compare against the exact version the user loaded or explicitly approved.
/// This is a last-moment conflict check, not a filesystem-wide compare-and-swap:
/// an uncooperative writer can still race the final check and rename.
pub(crate) fn save(path: &Path, expected: &Snapshot, bytes: &[u8]) -> Result<Snapshot, SaveError> {
    save_with_before_replace(path, expected, bytes, || {})
}

fn save_with_before_replace(
    path: &Path,
    expected: &Snapshot,
    bytes: &[u8],
    before_replace: impl FnOnce(),
) -> Result<Snapshot, SaveError> {
    let current = Snapshot::read(path)?;
    if &current != expected {
        return Err(SaveError::Changed(current));
    }
    if let Snapshot::File(file) = expected {
        if file.bytes.as_ref() == bytes {
            return Ok(current);
        }
        if file.identity.mode & 0o222 == 0 {
            return Err(SaveError::Failed(
                "file is read-only; check it out or change its permissions first".into(),
            ));
        }
        if file.identity.links != 1 {
            return Err(SaveError::Failed(
                "refusing to replace a hard-linked file and split its shared contents".into(),
            ));
        }
    }

    let parent = path
        .parent()
        .ok_or_else(|| "destination has no parent directory".to_owned())?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".yori-save-")
        .tempfile_in(parent)
        .map_err(|error| error.to_string())?;
    temporary
        .write_all(bytes)
        .map_err(|error| error.to_string())?;
    if let Snapshot::File(file) = expected {
        std::os::unix::fs::chown(
            temporary.path(),
            Some(file.identity.uid),
            Some(file.identity.gid),
        )
        .map_err(|error| format!("cannot preserve file ownership: {error}"))?;
        temporary
            .as_file()
            .set_permissions(Permissions::from_mode(file.identity.mode & 0o7777))
            .map_err(|error| error.to_string())?;
        for (name, value) in &file.attributes {
            rustix::fs::fsetxattr(temporary.as_file(), name, value, XattrFlags::empty())
                .map_err(|error| format!("cannot preserve file attributes: {error}"))?;
        }
    }
    // New files retain tempfile's restrictive 0600 permissions.
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    let written = Snapshot::read(temporary.path())?;

    before_replace();
    let current = Snapshot::read(path)?;
    if &current != expected {
        return Err(SaveError::Changed(current));
    }

    if expected.is_missing() {
        temporary
            .persist_noclobber(path)
            .map_err(|error| error.to_string())?;
    } else {
        temporary.persist(path).map_err(|error| error.to_string())?;
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("file replaced, but directory synchronization failed: {error}"))?;

    let actual = Snapshot::read(path)?;
    // Rename may update ctime/link metadata. Verify the contents and security
    // metadata rather than mistaking that for an external write.
    if let (Snapshot::File(written), Snapshot::File(actual_file)) = (&written, &actual)
        && written.bytes == actual_file.bytes
        && written.identity.mode == actual_file.identity.mode
        && written.identity.uid == actual_file.identity.uid
        && written.identity.gid == actual_file.identity.gid
        && written.attributes == actual_file.attributes
    {
        return Ok(actual);
    }

    Err(SaveError::Failed(
        "destination changed immediately after saving; your editor contents are still retained"
            .into(),
    ))
}
