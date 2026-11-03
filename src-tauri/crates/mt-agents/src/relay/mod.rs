//! Canal IPC local via Unix domain socket (`RelayServer`) e protocolo do `mt-agent-relay`.

pub mod protocol;
pub mod server;

pub use protocol::{
    MAX_RELAY_MESSAGE_BYTES, RELAY_PROTOCOL_VERSION, RelayAck, RelayEnvelope, RelayMessage,
};
pub use server::{AuthenticatedRelayEvent, RelayServer, RelayToken};
