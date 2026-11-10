//! Test-only oracle guest.

#[path = "../../../src/oracle.rs"]
mod oracle;

lee_core::actor_entrypoint!(raw oracle::handle_message);
