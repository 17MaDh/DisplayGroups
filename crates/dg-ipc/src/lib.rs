//! Agent ↔ lock-screen service IPC (ARCHITECTURE.md §8.3).
//!
//! * [`protocol`]: versioned messages. Only display geometry and chords ever reach
//!   the service: no audio, no actions, no paths.
//! * [`framing`]: 4-byte length prefix + JSON, capped at [`MAX_MESSAGE_BYTES`].
//! * [`pipe`]: named pipes with an explicit DACL, remote clients rejected, and
//!   caller identification (session id and user SID).

pub mod framing;
#[cfg(windows)]
pub mod pipe;
pub mod protocol;

pub use framing::{read_message, write_message, IpcError, MAX_MESSAGE_BYTES};
pub use protocol::*;
