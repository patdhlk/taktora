//! UDS and TCP transports to a co-located `dlt-daemon`. REQ_0807.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use thiserror::Error;

/// What kind of socket to open. UDS is the documented default
/// (REQ_0807).
#[derive(Debug, Clone)]
pub enum TransportConfig {
    /// Connect to `dlt-daemon` over a Unix-domain socket at `path`.
    Uds(PathBuf),
    /// Connect to `dlt-daemon` over TCP at `addr` (e.g. `"127.0.0.1:3490"`).
    Tcp(String),
}

/// An opened transport connection to `dlt-daemon`.
#[derive(Debug)]
pub enum Transport {
    /// Unix-domain socket connection.
    Uds(UnixStream),
    /// TCP connection.
    Tcp(TcpStream),
}

/// Errors returned by [`Transport::connect`], [`Transport::write_all`], and
/// [`Transport::read`].
#[derive(Debug, Error)]
pub enum TransportError {
    /// Wraps an underlying I/O failure.
    #[error("transport I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl Transport {
    /// Open a connection per `cfg`.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] if the underlying socket call fails.
    pub fn connect(cfg: &TransportConfig) -> Result<Self, TransportError> {
        Ok(match cfg {
            TransportConfig::Uds(path) => Transport::Uds(UnixStream::connect(path)?),
            TransportConfig::Tcp(addr) => Transport::Tcp(TcpStream::connect(addr)?),
        })
    }

    /// Write every byte in `buf` to the transport. Short writes are retried
    /// internally by [`std::io::Write::write_all`].
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] on any I/O failure.
    pub fn write_all(&mut self, buf: &[u8]) -> Result<(), TransportError> {
        match self {
            Transport::Uds(s) => s.write_all(buf)?,
            Transport::Tcp(s) => s.write_all(buf)?,
        }
        Ok(())
    }

    /// Read up to `buf.len()` bytes from the transport. Returns the number of
    /// bytes read; `0` indicates an orderly shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Io`] on any I/O failure.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        Ok(match self {
            Transport::Uds(s) => s.read(buf)?,
            Transport::Tcp(s) => s.read(buf)?,
        })
    }
}
