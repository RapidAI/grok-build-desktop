//! ACP-over-stdio transport and client used by the desktop host.

mod client;
mod framing;

pub use client::{
    AcpClient, AcpEvent, PermissionDecision, PermissionRequest, SessionInfo, StreamText,
};
pub use framing::{decode_line, encode_line};
