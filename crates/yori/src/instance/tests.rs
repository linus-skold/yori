//! Real D-Bus requests on isolated buses; never contact the user's desktop bus.

#[path = "../../tests/support/bus.rs"]
mod bus;

use super::*;
use bus::TestBus;
use std::{
    sync::{Arc, Barrier, mpsc},
    thread,
};

#[test]
fn secondary_waits_for_workspace_acknowledgment_and_preserves_path_bytes() {
    let bus = TestBus::new();
    let primary = Instance::establish(bus.builder(), &[]).unwrap().unwrap();
    let pairs = vec![(
        PathBuf::from(OsString::from_vec(
            b"/tmp/baseline with spaces\xff.rs".to_vec(),
        )),
        PathBuf::from("/tmp/local\nfile.rs"),
    )];
    let expected = pairs.clone();
    let address = bus.address.clone();
    let (finished, completion) = mpsc::channel();
    let client = thread::spawn(move || {
        let result = Instance::establish(Builder::address(address.as_str()).unwrap(), &pairs);
        finished
            .send(result.map(|instance| instance.is_none()))
            .unwrap();
    });

    let request = primary.requests.recv_blocking().unwrap();
    assert_eq!(request.pairs, expected);
    assert!(
        completion.recv_timeout(Duration::from_millis(30)).is_err(),
        "delivery alone must not release Perforce's temporary files"
    );
    request.complete(Ok(()));
    assert!(
        completion
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
    );
    client.join().unwrap();
}

#[test]
fn workspace_errors_are_returned_without_becoming_a_second_instance() {
    let bus = TestBus::new();
    let primary = Instance::establish(bus.builder(), &[]).unwrap().unwrap();
    let address = bus.address.clone();
    let client = thread::spawn(move || {
        Instance::establish(Builder::address(address.as_str()).unwrap(), &[])
            .err()
            .expect("handoff should fail")
    });

    let request = primary.requests.recv_blocking().unwrap();
    assert!(
        request.pairs.is_empty(),
        "an empty request activates the window"
    );
    request.complete(Err("cannot read temporary baseline".into()));
    assert!(
        client
            .join()
            .unwrap()
            .contains("cannot read temporary baseline")
    );
}

#[test]
fn concurrent_launches_elect_exactly_one_owner() {
    let bus = TestBus::new();
    let barrier = Arc::new(Barrier::new(4));
    let clients = (0..4)
        .map(|_| {
            let address = bus.address.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let instance =
                    Instance::establish(Builder::address(address.as_str()).unwrap(), &[]).unwrap();
                if let Some(primary) = &instance {
                    for _ in 0..3 {
                        primary.requests.recv_blocking().unwrap().complete(Ok(()));
                    }
                }

                // Keep the winning connection alive until all replies are delivered.
                instance
            })
        })
        .collect::<Vec<_>>();
    let outcomes = clients
        .into_iter()
        .map(|client| client.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|instance| instance.is_some())
            .count(),
        1
    );
}

#[test]
fn disconnect_releases_ownership_without_any_socket_cleanup() {
    let bus = TestBus::new();
    let primary = Instance::establish(bus.builder(), &[]).unwrap().unwrap();
    let Instance {
        _connection: connection,
        ..
    } = primary;
    connection.close().unwrap();

    let observer = bus.builder().build().unwrap();
    let proxy = zbus::blocking::fdo::DBusProxy::new(&observer).unwrap();
    let started = Instant::now();
    while proxy.name_has_owner(BUS_NAME.try_into().unwrap()).unwrap() {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }

    assert!(Instance::establish(bus.builder(), &[]).unwrap().is_some());
}

#[test]
fn invalid_and_oversized_requests_are_rejected_before_ui_dispatch() {
    for pairs in [
        vec![(b"relative.rs".to_vec(), b"/local".to_vec())],
        vec![(b"/base\0".to_vec(), b"/local".to_vec())],
        vec![(b"/base".to_vec(), b"/local".to_vec()); MAX_PAIRS + 1],
        vec![(vec![b'/'; MAX_PATH_BYTES], b"/local".to_vec())],
    ] {
        assert!(decode_pairs(pairs).is_err());
    }

    let bus = TestBus::new();
    let primary = Instance::establish(bus.builder(), &[]).unwrap().unwrap();
    let connection = bus.builder().build().unwrap();
    let result = connection.call_method(
        Some(BUS_NAME),
        OBJECT_PATH,
        Some(INTERFACE),
        "OpenComparisons",
        &(vec![(b"relative".to_vec(), b"/local".to_vec())],),
    );
    assert!(result.is_err());
    assert!(primary.requests.try_recv().is_err());
}
