//! SPIKE shell: the reader was PROMOTED to `descent_core::reader` (session 4,
//! after 10/10 token-identity vs the oracle lexer). This crate remains as the
//! home of the spike's probes/metrics binaries (main.rs token dump with
//! warnings, bin/comment_audit) and NOTES.md (mismatch classes, bridge
//! metrics, normalization evidence). The re-export keeps those binaries and
//! any external references working.

pub use descent_core::reader::*;
