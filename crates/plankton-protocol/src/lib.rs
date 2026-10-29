pub mod acp;
pub mod daemon;
pub mod error;
pub mod exposure;
pub mod password_changes;
pub mod passwords;
pub mod resources;

// Version 6 replaces raw credential delivery with daemon-owned consumer execution.
pub const PROTOCOL_VERSION: u16 = 6;
