#![cfg_attr(not(test), no_main)]

use nssa_core::account::AccountWithMetadata;
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "token_mint_authority_core::Instruction")]
mod token_mint_authority {
    #[allow(unused_imports)]
    use super::*;

    /// Mint a fixed faucet grant to the calling recipient, rate-limited to once
    /// per 24h per `(recipient, token definition)` (host fn
    /// `token_mint_authority_program::faucet_mint::faucet_mint`).
    ///
    /// Pure delegating proxy: `user -> token-mint-authority -> token`. Emits one chained
    /// `Token::MintWithAuthority` authorized by this program's mint-authority PDA
    /// seed — the Token-Mint-Authority holds no key of its own. Wall-clock time is read
    /// from the system `CLOCK_01` account passed as the 6th input, since the
    /// pinned `ProgramContext` exposes no clock.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails — see the host fn for the full list.
    #[instruction]
    pub fn faucet_mint(
        ctx: ProgramContext,
        #[account(signer)]
        recipient: AccountWithMetadata,
        #[account(mut)]
        mint_allowance: AccountWithMetadata,
        #[account(mut)]
        user_holding: AccountWithMetadata,
        #[account(mut)]
        token_definition: AccountWithMetadata,
        #[account(mut)]
        mint_authority: AccountWithMetadata,
        clock: AccountWithMetadata,
    ) -> SpelResult {
        let (post_states, chained_calls) = token_mint_authority_program::faucet_mint::faucet_mint(
            recipient,
            mint_allowance,
            user_holding,
            token_definition,
            mint_authority,
            clock,
            ctx.self_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(
            post_states,
            chained_calls,
        ))
    }
}
