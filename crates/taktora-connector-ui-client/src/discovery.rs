//! Service-registry discovery and manifest reading (REQ_0877, REQ_0872).
//!
//! [`discover`] scans the iceoryx2 service registry for every service whose name
//! ends in the well-known manifest suffix (`.manifest`) and reads each
//! [`Manifest`]. This is the *only* place a service name is derived by convention
//! — it is the bootstrap needed to find the manifest, which then becomes the sole
//! source of every other service name (REQ_0873). [`Client::connect`] uses the
//! same machinery to bind one named instance.
//!
//! [`Client::connect`]: crate::Client::connect

use std::time::{Duration, Instant};

use iceoryx2::node::Node;
use iceoryx2::prelude::{CallbackProgression, NodeBuilder, ServiceDetails, ipc};
use iceoryx2::service::Service as _;
use taktora_connector_transport_iox::{RawChannelReader, ServiceFactory};
use taktora_connector_ui_contract::{MANIFEST_SERVICE_SUFFIX, Manifest};

use crate::ENVELOPE_CAPACITY;
use crate::error::ClientError;

/// The default time [`discover`] / [`Client::connect`] waits for a manifest's
/// first (history-redelivered) sample.
///
/// [`Client::connect`]: crate::Client::connect
pub(crate) const DEFAULT_MANIFEST_TIMEOUT: Duration = Duration::from_secs(2);

/// The manifest service name for `instance` (the one bootstrap convention).
///
/// Delegates to [`taktora_connector_ui_contract::manifest_service_name`] so the
/// client and server share one definition of this bootstrap name.
#[must_use]
pub fn manifest_service_name(instance: &str) -> String {
    taktora_connector_ui_contract::manifest_service_name(instance)
}

/// Create a fresh iceoryx2 client node bound to the default (same-host) config.
pub(crate) fn create_node() -> Result<Node<ipc::Service>, ClientError> {
    NodeBuilder::new()
        .create::<ipc::Service>()
        .map_err(|e| ClientError::Iox(format!("node create: {e:?}")))
}

/// List every service in the registry whose name ends in `.manifest`.
///
/// # Errors
///
/// Returns [`ClientError::Iox`] if the service-registry enumeration fails.
pub fn list_manifest_services(node: &Node<ipc::Service>) -> Result<Vec<String>, ClientError> {
    let mut names = Vec::new();
    ipc::Service::list(node.config(), |service: ServiceDetails<ipc::Service>| {
        let name = service.static_details.name().as_str();
        if name.ends_with(MANIFEST_SERVICE_SUFFIX) {
            names.push(name.to_owned());
        }
        CallbackProgression::Continue
    })
    .map_err(|e| ClientError::Iox(format!("service list: {e:?}")))?;
    Ok(names)
}

/// Read the latest [`Manifest`] from an already-opened manifest reader, polling
/// until a sample arrives or `timeout` elapses.
///
/// The manifest service is `history_size(1)` and the server republishes it every
/// tick, so a fresh subscriber receives the current manifest on the publisher's
/// next send — within one UI cadence, no handshake (REQ_0881).
pub(crate) fn read_manifest_blocking(
    reader: &RawChannelReader<ENVELOPE_CAPACITY>,
    service: &str,
    timeout: Duration,
) -> Result<Manifest, ClientError> {
    let deadline = Instant::now() + timeout;
    let mut scratch = vec![0u8; ENVELOPE_CAPACITY];
    loop {
        if let Some((bytes, _)) = drain_latest(reader, &mut scratch)? {
            let manifest: Manifest = serde_json::from_slice(&bytes)?;
            return Ok(manifest);
        }
        if Instant::now() >= deadline {
            return Err(ClientError::ManifestUnavailable {
                service: service.to_owned(),
            });
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Drain a reader to its newest envelope, returning the latest payload bytes (a
/// fresh `Vec`) and the envelope sample metadata, or `None` if nothing was
/// queued.
///
/// `scratch` must be at least `ENVELOPE_CAPACITY` bytes; it is reused across
/// envelopes so only the final (newest) payload is copied out.
pub(crate) fn drain_latest(
    reader: &RawChannelReader<ENVELOPE_CAPACITY>,
    scratch: &mut [u8],
) -> Result<Option<(Vec<u8>, taktora_connector_transport_iox::RawSample)>, ClientError> {
    let mut latest: Option<(Vec<u8>, taktora_connector_transport_iox::RawSample)> = None;
    while let Some(sample) = reader.try_recv_into(scratch)? {
        latest = Some((scratch[..sample.payload_len].to_vec(), sample));
    }
    Ok(latest)
}

/// Discover every connector instance on the host, returning each one's current
/// [`Manifest`] (REQ_0877), using the default manifest-read timeout.
///
/// Best-effort: a manifest service that fails to yield a sample within the
/// timeout is skipped rather than failing the whole scan, so one stalled
/// connector does not hide the others.
///
/// # Errors
///
/// Returns [`ClientError::Iox`] only if the node cannot be created or the
/// registry enumeration itself fails.
pub fn discover() -> Result<Vec<Manifest>, ClientError> {
    discover_with(DEFAULT_MANIFEST_TIMEOUT)
}

/// [`discover`] with an explicit per-manifest read `timeout`.
///
/// # Errors
///
/// As [`discover`].
pub fn discover_with(timeout: Duration) -> Result<Vec<Manifest>, ClientError> {
    let node = create_node()?;
    let services = list_manifest_services(&node)?;
    let factory = ServiceFactory::new(&node);
    let mut manifests = Vec::new();
    for service in services {
        let Ok(reader) = factory.create_raw_reader_named::<ENVELOPE_CAPACITY>(&service) else {
            continue;
        };
        if let Ok(manifest) = read_manifest_blocking(&reader, &service, timeout) {
            manifests.push(manifest);
        }
    }
    Ok(manifests)
}
