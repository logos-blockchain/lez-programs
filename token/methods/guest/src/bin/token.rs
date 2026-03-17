#![no_main]

use spel_framework::prelude::*;
use nssa_core::account::AccountWithMetadata;

risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "token_core::Instruction")]
mod token {
    #[allow(unused_imports)]
    use super::*;

    /// Transfer tokens from sender to recipient.
    #[instruction]
    pub fn transfer(
        sender: AccountWithMetadata,
        recipient: AccountWithMetadata,
        amount_to_transfer: u128,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(token_program::transfer::transfer(
            sender,
            recipient,
            amount_to_transfer,
        )))
    }

    /// Create a new fungible token definition without metadata.
    #[instruction]
    pub fn new_fungible_definition(
        definition_target_account: AccountWithMetadata,
        holding_target_account: AccountWithMetadata,
        name: String,
        total_supply: u128,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(
            token_program::new_definition::new_fungible_definition(
                definition_target_account,
                holding_target_account,
                name,
                total_supply,
            ),
        ))
    }

    /// Create a new fungible or non-fungible token definition with metadata.
    #[instruction]
    pub fn new_definition_with_metadata(
        definition_target_account: AccountWithMetadata,
        holding_target_account: AccountWithMetadata,
        metadata_target_account: AccountWithMetadata,
        new_definition: token_core::NewTokenDefinition,
        metadata: Box<token_core::NewTokenMetadata>,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(
            token_program::new_definition::new_definition_with_metadata(
                definition_target_account,
                holding_target_account,
                metadata_target_account,
                new_definition,
                *metadata,
            ),
        ))
    }

    /// Initialize a token holding account for a given token definition.
    #[instruction]
    pub fn initialize_account(
        definition_account: AccountWithMetadata,
        account_to_initialize: AccountWithMetadata,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(
            token_program::initialize::initialize_account(
                definition_account,
                account_to_initialize,
            ),
        ))
    }

    /// Burn tokens from the holder's account.
    #[instruction]
    pub fn burn(
        definition_account: AccountWithMetadata,
        user_holding_account: AccountWithMetadata,
        amount_to_burn: u128,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(token_program::burn::burn(
            definition_account,
            user_holding_account,
            amount_to_burn,
        )))
    }

    /// Mint new tokens to the holder's account.
    #[instruction]
    pub fn mint(
        definition_account: AccountWithMetadata,
        user_holding_account: AccountWithMetadata,
        amount_to_mint: u128,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(token_program::mint::mint(
            definition_account,
            user_holding_account,
            amount_to_mint,
        )))
    }

    /// Print a new NFT from the master copy.
    #[instruction]
    pub fn print_nft(
        master_account: AccountWithMetadata,
        printed_account: AccountWithMetadata,
    ) -> SpelResult {
        Ok(SpelOutput::states_only(token_program::print_nft::print_nft(
            master_account,
            printed_account,
        )))
    }
}
