#![cfg_attr(not(test), no_main)]

use nssa_core::account::ShardData;
use spel_framework::{context::ProgramContext, prelude::*};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "token_mint_authority_core::Instruction")]
mod token_mint_authority {
    #[allow(unused_imports)]
    use super::*;

    /// Mint a fixed faucet grant to the calling recipient, rate-limited to once
    /// per 24h per `(recipient, token definition)`.
    ///
    /// A delegating proxy: `user -> token-mint-authority -> token`. One chained
    /// `Token::MintWithAuthority`, authorized by this program's mint-authority PDA
    /// seed — this program holds no key of its own.
    ///
    /// It owns exactly one shard, the allowance, and that is the only effect it
    /// writes. The other two effects are guards on shards belonging to other
    /// programs: the clock, and the Token Program's definition.
    ///
    /// # Errors
    /// Returns the host program's panic-converted error if any precondition
    /// fails — see the host fn for the full list.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit recipient, allowance, holding, definition, authority, and clock accounts"
    )]
    #[plan]
    pub fn faucet_mint(
        ctx: ProgramContext,
        #[account(signer)] recipient: AccountMeta,
        #[account(mut)] mint_allowance: AccountMeta,
        #[account(mut)] user_holding: AccountMeta,
        #[account(mut)] token_definition: AccountMeta,
        mint_authority: AccountMeta,
        clock: AccountMeta,
        now: u64,
    ) -> SpelResult {
        let mint_call = token_mint_authority_program::faucet_mint::plan_faucet_mint(
            &recipient,
            &mint_allowance,
            &user_holding,
            &token_definition,
            &mint_authority,
            &clock,
            ctx.self_account_id,
        );
        let token_program_id = token_definition.program_account_id;

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(token_mint_authority_core::CheckClock { now }),
            )
            .guard(
                &token_definition,
                token_program_id,
                &Effect::CheckFaucetAuthority(token_mint_authority_core::CheckFaucetAuthority {
                    mint_authority_id: mint_authority.account_id,
                }),
            )
            .effect(
                &mint_allowance,
                mint_allowance.program_account_id,
                &Effect::RecordMint(token_mint_authority_core::RecordMint {
                    recipient_id: recipient.account_id,
                    definition_id: token_definition.account_id,
                    now,
                }),
            )
            .call(mint_call))
    }

    #[apply]
    pub fn check_clock(
        effect: token_mint_authority_core::CheckClock,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        token_mint_authority_program::faucet_mint::check_clock(pre_data, effect.now);
        None
    }

    #[apply]
    pub fn check_faucet_authority(
        effect: token_mint_authority_core::CheckFaucetAuthority,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        token_mint_authority_program::faucet_mint::check_faucet_authority(
            pre_data,
            effect.mint_authority_id,
        );
        None
    }

    #[apply]
    pub fn record_mint(
        effect: token_mint_authority_core::RecordMint,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(token_mint_authority_program::faucet_mint::record_mint(
            pre_data,
            effect.recipient_id,
            effect.definition_id,
            effect.now,
        ))
    }
}
