//! Ports for virtualised simulation components.
//!
//! These traits are the seam through which virtualised hardware is offered to
//! subjects: clocks, networks, and storage. They are deliberately
//! substrate-agnostic — the application and Lua adapters only ever talk to
//! these traits, never to a concrete substrate. Each substrate provides its
//! own implementations (e.g. the Docker substrate uses libfaketime for
//! clocks), and substrates that cannot virtualise a component inherit the
//! default method bodies, which report the operation as unsupported.

pub mod clock;
pub mod network;
pub mod storage;

pub use clock::{ClockControl, ClockState};
pub use network::{Direction, LinkId, NetworkControl, PartitionMode};
pub use storage::{StorageControl, StorageOpts};
