//! NFT Program guest: one actor per run (`docs/actor-model/nft.md`).

lee_core::actor_entrypoint!(raw nft_program::handle_message);
