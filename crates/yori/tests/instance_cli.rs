//! Exercise the real CLI as a secondary process, without a display server.

#[path = "support/bus.rs"]
mod bus;

use bus::TestBus;
use std::{
    ffi::OsString,
    io::Read,
    os::unix::ffi::{OsStrExt, OsStringExt},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

type Pairs = Vec<(Vec<u8>, Vec<u8>)>;
type Pending = (Pairs, async_channel::Sender<zbus::fdo::Result<()>>);

struct WorkspaceStub {
    requests: mpsc::Sender<Pending>,
}

#[zbus::interface(name = "io.github.trixnz.yori.Instance1")]
impl WorkspaceStub {
    async fn open_comparisons(&self, pairs: Pairs) -> zbus::fdo::Result<()> {
        let (reply, response) = async_channel::bounded(1);
        self.requests.send((pairs, reply)).unwrap();
        response.recv().await.unwrap()
    }
}

struct RunningCli(Child);

impl Drop for RunningCli {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn cli(bus: &TestBus, directory: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_yori"));
    command
        .current_dir(directory)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .env("XDG_RUNTIME_DIR", directory)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    command
}

#[test]
fn cli_forwards_absolute_path_bytes_and_exits_only_after_the_reply() {
    let bus = TestBus::new();
    let directory = tempfile::tempdir().unwrap();
    let (requests, incoming) = mpsc::channel();
    let _owner = bus
        .builder()
        .serve_at("/io/github/trixnz/yori", WorkspaceStub { requests })
        .unwrap()
        .name("io.github.trixnz.yori")
        .unwrap()
        .build()
        .unwrap();
    let local = OsString::from_vec(b"local\xff.rs".to_vec());

    for (focus_only, fail) in [(false, false), (true, false), (false, true)] {
        let mut command = cli(&bus, directory.path());
        if !focus_only {
            command.arg("baseline with spaces.rs").arg(&local);
        }
        let mut child = RunningCli(command.spawn().unwrap());
        let (pairs, reply) = incoming.recv_timeout(Duration::from_secs(5)).unwrap();
        let expected = if focus_only {
            Vec::new()
        } else {
            vec![(
                directory
                    .path()
                    .join("baseline with spaces.rs")
                    .as_os_str()
                    .as_bytes()
                    .to_vec(),
                directory
                    .path()
                    .join(&local)
                    .as_os_str()
                    .as_bytes()
                    .to_vec(),
            )]
        };
        assert_eq!(pairs, expected);
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "the CLI must wait until files have been consumed"
        );

        reply
            .send_blocking(if fail {
                Err(zbus::fdo::Error::Failed(
                    "temporary baseline unreadable".into(),
                ))
            } else {
                Ok(())
            })
            .unwrap();
        let status = child.0.wait().unwrap();
        let mut stderr = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert_eq!(status.success(), !fail, "{stderr}");
        if fail {
            assert!(stderr.contains("temporary baseline unreadable"));
        }
    }
}

#[test]
fn invalid_arguments_and_missing_bus_fail_without_starting_a_window() {
    let bus = TestBus::new();
    let directory = tempfile::tempdir().unwrap();
    let result = cli(&bus, directory.path())
        .arg("unpaired.rs")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("usage:"));

    let result = cli(&bus, directory.path())
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!(
                "unix:path={}",
                directory.path().join("missing-bus").display()
            ),
        )
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("session-bus connection"));
}
