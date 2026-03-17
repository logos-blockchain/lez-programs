#![no_main]

use spel_framework::prelude::*;
use nssa_core::{account::AccountWithMetadata, program::ProgramId};

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "ata_core::Instruction")]
mod ata {
    #[allow(unused_imports)]
    use super::*;

    /// Create the Associated Token Account for (owner, definition).
    /// Idempotent: no-op if the account already exists.
    #[instruction]
    pub fn create(
        owner: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        ata_account: AccountWithMetadata,
        ata_program_id: ProgramId,
    ) -> SpelResult {
        let (post_states, chained_calls) = ata_program::create::create_associated_token_account(
            owner,
            token_definition,
            ata_account,
            ata_program_id,
        );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Transfer tokens FROM owner's ATA to a recipient holding account.
    #[instruction]
    pub fn transfer(
        owner: AccountWithMetadata,
        sender_ata: AccountWithMetadata,
        recipient: AccountWithMetadata,
        ata_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            ata_program::transfer::transfer_from_associated_token_account(
                owner,
                sender_ata,
                recipient,
                ata_program_id,
                amount,
            );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }

    /// Burn tokens FROM owner's ATA.
    #[instruction]
    pub fn burn(
        owner: AccountWithMetadata,
        holder_ata: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        ata_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let (post_states, chained_calls) =
            ata_program::burn::burn_from_associated_token_account(
                owner,
                holder_ata,
                token_definition,
                ata_program_id,
                amount,
            );
        Ok(SpelOutput::with_chained_calls(post_states, chained_calls))
    }
}
