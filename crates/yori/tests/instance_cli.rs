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

type Comparisons = Vec<Vec<Vec<u8>>>;
type Pending = (Comparisons, async_channel::Sender<zbus::fdo::Result<()>>);

struct WorkspaceStub {
    requests: mpsc::Sender<Pending>,
}

#[zbus::interface(name = "io.github.trixnz.yori.Instance2")]
impl WorkspaceStub {
    async fn open_comparisons(&self, paths: Comparisons) -> zbus::fdo::Result<()> {
        let (reply, response) = async_channel::bounded(1);
        self.requests.send((paths, reply)).unwrap();
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
fn cli_forwards_diff_and_merge_roles_and_exits_only_after_the_reply() {
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
    let paths = [
        OsString::from("base with spaces.rs"),
        OsString::from_vec(b"local\xff.rs".to_vec()),
        OsString::from("incoming\nfile.go"),
        OsString::from_vec(b"result\xfe.cpp".to_vec()),
    ];

    for (count, fail) in [(2, false), (0, false), (2, true), (4, false), (4, true)] {
        let mut command = cli(&bus, directory.path());
        command.args(&paths[..count]);
        let mut child = RunningCli(command.spawn().unwrap());
        let (comparisons, reply) = incoming.recv_timeout(Duration::from_secs(5)).unwrap();
        let expected = if count == 0 {
            Vec::new()
        } else {
            vec![
                paths[..count]
                    .iter()
                    .map(|path| directory.path().join(path).as_os_str().as_bytes().to_vec())
                    .collect::<Vec<_>>(),
            ]
        };
        assert_eq!(comparisons, expected);
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "the CLI must wait until all inputs have been consumed"
        );

        reply
            .send_blocking(if fail {
                Err(zbus::fdo::Error::Failed(
                    "temporary input unreadable".into(),
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
            assert!(stderr.contains("temporary input unreadable"));
        }
    }
}

#[test]
fn invalid_arguments_and_missing_bus_fail_without_starting_a_window() {
    let bus = TestBus::new();
    let directory = tempfile::tempdir().unwrap();
    for count in [1, 3, 5, 6] {
        let result = cli(&bus, directory.path())
            .args(vec!["file.rs"; count])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("usage:"));
    }

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
