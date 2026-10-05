//! Sessions: one folder per recording, an append-only event log, and recovery.

pub mod levels;
pub mod log;
pub mod model;
pub mod paths;
pub mod recovery;
pub mod store;
