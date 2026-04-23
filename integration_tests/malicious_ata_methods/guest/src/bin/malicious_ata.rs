#![no_main]

use nssa_core::{
    account::AccountWithMetadata,
    program::{AccountPostState, ProgramId},
};
use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

fn unchanged_post_state(account: AccountWithMetadata) -> AccountPostState {
    AccountPostState::new(account.account)
}

#[lez_program(instruction = "ata_core::Instruction")]
mod malicious_ata {
    #[allow(unused_imports)]
    use super::*;

    /// Intentionally malicious test helper. It acknowledges ATA creation without creating
    /// anything, proving callers must not trust a caller-supplied ATA program ID.
    #[instruction]
    pub fn create(
        owner: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        ata_account: AccountWithMetadata,
        ata_program_id: ProgramId,
    ) -> SpelResult {
        let _ = ata_program_id;
        assert!(owner.is_authorized, "Owner authorization is missing");

        Ok(SpelOutput::states_only(vec![
            unchanged_post_state(owner),
            unchanged_post_state(token_definition),
            unchanged_post_state(ata_account),
        ]))
    }

    /// Intentionally malicious test helper. It returns success without debiting the sender
    /// or crediting the recipient.
    #[instruction]
    pub fn transfer(
        owner: AccountWithMetadata,
        sender_ata: AccountWithMetadata,
        recipient: AccountWithMetadata,
        ata_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let _ = (ata_program_id, amount);
        assert!(owner.is_authorized, "Owner authorization is missing");

        Ok(SpelOutput::states_only(vec![
            unchanged_post_state(owner),
            unchanged_post_state(sender_ata),
            unchanged_post_state(recipient),
        ]))
    }

    /// Intentionally malicious test helper. It returns success without burning the LP position
    /// or decreasing the LP definition supply.
    #[instruction]
    pub fn burn(
        owner: AccountWithMetadata,
        holder_ata: AccountWithMetadata,
        token_definition: AccountWithMetadata,
        ata_program_id: ProgramId,
        amount: u128,
    ) -> SpelResult {
        let _ = (ata_program_id, amount);
        assert!(owner.is_authorized, "Owner authorization is missing");

        Ok(SpelOutput::states_only(vec![
            unchanged_post_state(owner),
            unchanged_post_state(holder_ata),
            unchanged_post_state(token_definition),
        ]))
    }
}
