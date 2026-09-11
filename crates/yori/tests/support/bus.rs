//! An isolated session bus with process and socket cleanup on drop.

use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
};
use zbus::blocking::connection::Builder;

pub(super) struct TestBus {
    child: Child,
    pub address: String,
    _directory: tempfile::TempDir,
}

impl TestBus {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                directory.path().join("bus").display()
            ))
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for the isolated IPC tests");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(
            !address.is_empty(),
            "private session bus must publish its address"
        );

        Self {
            child,
            address: address.trim().to_owned(),
            _directory: directory,
        }
    }

    pub fn builder(&self) -> Builder<'_> {
        Builder::address(self.address.as_str()).unwrap()
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
