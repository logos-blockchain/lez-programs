#![no_main]

use spel_framework::prelude::*;
use spel_framework::context::ProgramContext;
use nssa_core::{account::AccountWithMetadata, program::ProgramId};

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "ata_core::Instruction")]
mod ata {
    #[allow(unused_imports)]
    use super::*;

    /// Create the Associated Token Account for (token program, owner, definition).
    /// Idempotent: no-op if the account already exists.
    /// The token program is selected explicitly by `token_program_id`; the token definition and
    /// any existing ATA occupant must be owned by that program.
    #[instruction]
    pub fn create(
        ctx: ProgramContext,
        owner: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        ata_account: AccountWithMetadata,
        token_program_id: ProgramId,
    ) -> SpelResult {
        let (post_states, chained_calls) = ata_program::create::create_associated_token_account(
            owner,
            token_definition,
            ata_account,
            ctx.self_program_id,
            token_program_id,
        );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }

    /// Transfer tokens FROM owner's ATA to a recipient token holding account.
    /// The token program is selected explicitly by `token_program_id`; the sender ATA and recipient
    /// holding must be owned by that program.
    /// The recipient holding must already be initialized, be owned by the same token program
    /// as the sender ATA, and point at the same token definition as the sender.
    #[instruction]
    pub fn transfer(
        ctx: ProgramContext,
        owner: AccountWithMetadata,
        sender_ata: AccountWithMetadata,
        recipient: AccountWithMetadata,
        token_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            ata_program::transfer::transfer_from_associated_token_account(
                owner,
                sender_ata,
                recipient,
                ctx.self_program_id,
                token_program_id,
                amount,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }

    /// Burn tokens FROM owner's ATA.
    /// The token program is selected explicitly by `token_program_id`; the holder ATA and token
    /// definition must be owned by that program.
    #[instruction]
    pub fn burn(
        ctx: ProgramContext,
        owner: AccountWithMetadata,
        holder_ata: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        token_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            ata_program::burn::burn_from_associated_token_account(
                owner,
                holder_ata,
                token_definition,
                ctx.self_program_id,
                token_program_id,
                amount,
            );
        Ok(spel_framework::SpelOutput::execute(post_states, chained_calls))
    }
}
