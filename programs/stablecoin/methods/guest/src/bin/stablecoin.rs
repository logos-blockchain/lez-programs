//! Stablecoin guest: one actor per run.

lee_core::actor_entrypoint!(raw stablecoin_program::handle_message);
