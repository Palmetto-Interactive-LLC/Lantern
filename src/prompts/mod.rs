//! Pattern-aware `devorch_get_setup_instructions` content, one module per
//! non-team `LaunchPattern` slug (`DEVORCH_PATTERN`). The team pattern's
//! instructions stay inline in `mcp/tools.rs::handle_get_setup_instructions`
//! (unchanged, byte-identical to pre-pattern behavior).

pub mod simple;
