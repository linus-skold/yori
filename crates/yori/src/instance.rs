//! Session-bus ownership and acknowledged file handoff, independent of GPUI.
//!
//! The first connection exclusively owns the application name. Later invocations
//! call its versioned interface and exit only after the workspace replies.

#[cfg(test)]
mod tests;

use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use zbus::{
    blocking::{Connection, connection::Builder},
    fdo,
};

use crate::comparison::ComparisonPaths;

const BUS_NAME: &str = "io.github.trixnz.yori";
const OBJECT_PATH: &str = "/io/github/trixnz/yori";
const INTERFACE: &str = "io.github.trixnz.yori.Instance2";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COMPARISONS: usize = 128;
const MAX_PATH_BYTES: usize = 1024 * 1024;

/// Path bytes avoid D-Bus string normalization and preserve non-UTF-8 filenames.
type WireComparisons = Vec<Vec<Vec<u8>>>;

pub(super) struct OpenRequest {
    pub comparisons: Vec<ComparisonPaths>,
    received: Instant,
    reply: Sender<Result<(), String>>,
}

impl OpenRequest {
    /// Don't apply a request which sat in the UI queue beyond the client timeout.
    pub fn expired(&self) -> bool {
        self.received.elapsed() >= REQUEST_TIMEOUT
    }

    pub fn complete(self, result: Result<(), String>) {
        let _ = self.reply.try_send(result);
    }
}

struct Endpoint {
    requests: Sender<OpenRequest>,
}

#[zbus::interface(name = "io.github.trixnz.yori.Instance2")]
impl Endpoint {
    async fn open_comparisons(&self, paths: WireComparisons) -> fdo::Result<()> {
        let comparisons = decode_comparisons(paths)?;
        let (reply, response) = async_channel::bounded(1);
        self.requests
            .try_send(OpenRequest {
                comparisons,
                received: Instant::now(),
                reply,
            })
            .map_err(|_| {
                fdo::Error::Failed("yori is busy or shutting down; retry the request".into())
            })?;

        // This is a workspace acknowledgment, not merely a transport receipt.
        // Perforce may delete its temporary baseline as soon as the sender exits.
        response
            .recv()
            .await
            .map_err(|_| fdo::Error::Failed("yori closed before opening the comparison".into()))?
            .map_err(fdo::Error::Failed)
    }
}

pub(super) struct Instance {
    // Retain ownership for the entire application lifetime.
    _connection: Connection,
    requests: Receiver<OpenRequest>,
}

impl Instance {
    /// Return the primary instance, or `None` after a successful handoff. Failure
    /// never falls back to a second window: a timed-out request may have started.
    pub fn start(comparisons: &[ComparisonPaths]) -> Result<Option<Self>, String> {
        let builder = Builder::session()
            .map_err(|error| format!("cannot connect to the desktop session bus: {error}"))?;
        Self::establish(builder, comparisons)
    }

    fn establish(
        builder: Builder<'_>,
        comparisons: &[ComparisonPaths],
    ) -> Result<Option<Self>, String> {
        let wire = encode_comparisons(comparisons);
        // Validate first launches too, before claiming the name or starting GPUI.
        decode_comparisons(wire.clone()).map_err(|error| error.to_string())?;
        let (requests, incoming) = async_channel::bounded(16);
        let connection = builder
            .method_timeout(REQUEST_TIMEOUT)
            .serve_at(OBJECT_PATH, Endpoint { requests })
            .and_then(Builder::build)
            .map_err(|error| format!("cannot initialize yori's session-bus connection: {error}"))?;

        // Register the interface before claiming the name so concurrent launches
        // can be queued safely even while the primary is initializing its window.
        match connection.request_name_with_flags(BUS_NAME, fdo::RequestNameFlags::DoNotQueue.into())
        {
            Ok(fdo::RequestNameReply::PrimaryOwner | fdo::RequestNameReply::AlreadyOwner) => {
                Ok(Some(Self {
                    _connection: connection,
                    requests: incoming,
                }))
            }
            Err(zbus::Error::NameTaken) | Ok(fdo::RequestNameReply::Exists) => {
                connection
                    .call_method(
                        Some(BUS_NAME),
                        OBJECT_PATH,
                        Some(INTERFACE),
                        "OpenComparisons",
                        &(wire,),
                    )
                    .map_err(|error| {
                        format!(
                            "handoff to running yori failed: {error}; no second window was started"
                        )
                    })?
                    .body()
                    .deserialize::<()>()
                    .map_err(|error| format!("invalid reply from running yori: {error}"))?;

                Ok(None)
            }
            Ok(fdo::RequestNameReply::InQueue) => {
                Err("unexpected queued instance ownership; refusing to start another window".into())
            }
            Err(error) => Err(format!("cannot claim yori's session-bus name: {error}")),
        }
    }

    pub async fn next(&self) -> Result<OpenRequest, async_channel::RecvError> {
        self.requests.recv().await
    }
}

fn encode_comparisons(comparisons: &[ComparisonPaths]) -> WireComparisons {
    comparisons
        .iter()
        .map(|comparison| {
            comparison
                .paths()
                .iter()
                .map(|path| path.as_os_str().as_bytes().to_vec())
                .collect()
        })
        .collect()
}

fn decode_comparisons(comparisons: WireComparisons) -> fdo::Result<Vec<ComparisonPaths>> {
    if comparisons.len() > MAX_COMPARISONS
        || comparisons.iter().flatten().map(Vec::len).sum::<usize>() > MAX_PATH_BYTES
        || comparisons
            .iter()
            .any(|paths| !matches!(paths.len(), 2 | 4))
    {
        return Err(fdo::Error::InvalidArgs(
            "expected two or four paths per comparison, within request size limits".into(),
        ));
    }

    let decode = |bytes: Vec<u8>| {
        if bytes.contains(&0) {
            return Err(fdo::Error::InvalidArgs(
                "file paths must not contain NUL bytes".into(),
            ));
        }

        let path = PathBuf::from(OsString::from_vec(bytes));
        if !path.is_absolute() {
            return Err(fdo::Error::InvalidArgs(
                "file paths must be absolute".into(),
            ));
        }

        Ok(path)
    };

    comparisons
        .into_iter()
        .map(|paths| {
            let paths = paths
                .into_iter()
                .map(decode)
                .collect::<fdo::Result<Vec<_>>>()?;
            ComparisonPaths::from_paths(&paths).map_err(fdo::Error::InvalidArgs)
        })
        .collect()
}
