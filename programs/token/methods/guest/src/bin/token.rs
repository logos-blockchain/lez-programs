//! Token Program guest: one actor per run (`docs/actor-model/token.md`).

lee_core::actor_entrypoint!(raw token_program::handle_message);
