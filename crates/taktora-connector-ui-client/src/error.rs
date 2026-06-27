//! The client error type.

use taktora_connector_core::ConnectorError;

/// Errors raised by the UI client.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// An iceoryx2 node / service-registry operation failed.
    Iox(String),
    /// A transport (channel open / send / receive) operation failed.
    Transport(ConnectorError),
    /// A JSON payload (manifest or ack) failed to parse / encode.
    Codec(serde_json::Error),
    /// No manifest could be read for the requested instance within the timeout
    /// (the connector is absent or has not yet published).
    ManifestUnavailable {
        /// The manifest service name that was polled.
        service: String,
    },
    /// The requested ViewModel name is not in the bound manifest.
    UnknownViewModel(String),
    /// The requested command name is not in the bound manifest.
    UnknownCommand(String),
    /// A command was attempted while the client is in read-only mode (the
    /// contract hash did not match — commands are disabled, REQ_0876).
    ReadOnly,
    /// A command invocation exhausted its retry budget without an ack.
    CommandTimeout {
        /// The command name.
        command: String,
        /// The number of attempts made.
        attempts: u32,
    },
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ClientError::Iox(m) => write!(f, "iceoryx2 error: {m}"),
            ClientError::Transport(e) => write!(f, "transport error: {e}"),
            ClientError::Codec(e) => write!(f, "json error: {e}"),
            ClientError::ManifestUnavailable { service } => {
                write!(f, "no manifest available on service '{service}'")
            }
            ClientError::UnknownViewModel(n) => write!(f, "unknown view model '{n}'"),
            ClientError::UnknownCommand(n) => write!(f, "unknown command '{n}'"),
            ClientError::ReadOnly => {
                write!(f, "client is in read-only mode; commands are disabled")
            }
            ClientError::CommandTimeout { command, attempts } => {
                write!(f, "command '{command}' timed out after {attempts} attempts")
            }
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Transport(e) => Some(e),
            ClientError::Codec(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ConnectorError> for ClientError {
    fn from(e: ConnectorError) -> Self {
        ClientError::Transport(e)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::Codec(e)
    }
}
