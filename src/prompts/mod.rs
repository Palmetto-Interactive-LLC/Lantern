//! Pattern-specific setup-instruction prompt text.
//!
//! Kept out of `mcp/tools.rs` (which owns the team-pattern instructions) so
//! multiple in-flight pattern implementations don't collide on the same
//! function body — see `executor.rs`.

pub mod executor;
