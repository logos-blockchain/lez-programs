#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, AccountWithMetadata};
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

#[lez_program(instruction = "token_core::Instruction")]
mod token {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    /// Transfer tokens from sender to recipient.
    /// Fresh public recipients must be explicitly authorized in the same transaction.
    #[instruction]
    pub fn transfer(
        #[account(mut, signer)]
        sender: AccountWithMetadata,
        #[account(mut)]
        recipient: AccountWithMetadata,
        amount_to_transfer: u128,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::transfer::transfer(sender, recipient, amount_to_transfer),
            vec![],
        ))
    }

    /// Create a new fungible token definition without metadata.
    /// Definition and holding targets must be uninitialized and authorized.
    /// `mint_authority` is `Some(id)` for a mintable token or `None` for fixed supply.
    #[instruction]
    pub fn new_fungible_definition(
        #[account(init, signer)]
        definition_target_account: AccountWithMetadata,
        #[account(init, signer)]
        holding_target_account: AccountWithMetadata,
        name: String,
        total_supply: u128,
        mint_authority: Option<AccountId>,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::new_definition::new_fungible_definition(
                definition_target_account,
                holding_target_account,
                name,
                total_supply,
                mint_authority,
            ),
            vec![],
        ))
    }

    /// Create a new fungible or non-fungible token definition with metadata.
    /// Definition, holding, and metadata targets must be uninitialized and authorized.
    #[expect(
        clippy::boxed_local,
        reason = "boxed metadata keeps the instruction argument size bounded on the stack"
    )]
    #[instruction]
    pub fn new_definition_with_metadata(
        #[account(init, signer)]
        definition_target_account: AccountWithMetadata,
        #[account(init, signer)]
        holding_target_account: AccountWithMetadata,
        #[account(init, signer)]
        metadata_target_account: AccountWithMetadata,
        new_definition: token_core::NewTokenDefinition,
        metadata: Box<token_core::NewTokenMetadata>,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::new_definition::new_definition_with_metadata(
                definition_target_account,
                holding_target_account,
                metadata_target_account,
                new_definition,
                *metadata,
            ),
            vec![],
        ))
    }

    /// Initialize a token holding account for a given token definition.
    /// The holding target must be uninitialized and authorized.
    #[instruction]
    pub fn initialize_account(
        ctx: ProgramContext,
        definition_account: AccountWithMetadata,
        #[account(init, signer)]
        account_to_initialize: AccountWithMetadata,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::initialize::initialize_account(
                definition_account,
                account_to_initialize,
                ctx.self_program_id,
            ),
            vec![],
        ))
    }

    /// Burn tokens from the holder's account.
    #[instruction]
    pub fn burn(
        #[account(mut)]
        definition_account: AccountWithMetadata,
        #[account(mut, signer)]
        user_holding_account: AccountWithMetadata,
        amount_to_burn: u128,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::burn::burn(definition_account, user_holding_account, amount_to_burn),
            vec![],
        ))
    }

    /// Mint new tokens to the holder's account.
    /// The definition account must be authorized as the current mint authority.
    /// Fresh public holders must be explicitly authorized in the same transaction.
    #[instruction]
    pub fn mint(
        ctx: ProgramContext,
        #[account(mut, signer)]
        definition_account: AccountWithMetadata,
        user_holding_account: AccountWithMetadata,
        authority_accounts: Vec<AccountWithMetadata>,
        amount_to_mint: u128,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::mint::mint(
                definition_account,
                user_holding_account,
                amount_to_mint,
                authority_accounts,
                ctx.self_program_id,
            ),
            vec![],
        ))
    }

    /// Rotate or renounce the mint authority for a fungible token definition.
    /// Pass `new_authority: None` to permanently renounce minting (fixed supply).
    /// The definition account must be authorized as the current mint authority.
    #[instruction]
    pub fn set_authority(
        ctx: ProgramContext,
        definition_account: AccountWithMetadata,
        authority_accounts: Vec<AccountWithMetadata>,
        new_authority: Option<AccountId>,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::set_authority::set_authority(
                definition_account,
                new_authority,
                authority_accounts,
                ctx.self_program_id,
            ),
            vec![],
        ))
    }

    /// Print a new NFT from the master copy.
    /// The printed copy target must be uninitialized and authorized.
    #[instruction]
    pub fn print_nft(
        #[account(mut, signer)]
        master_account: AccountWithMetadata,
        #[account(init, signer)]
        printed_account: AccountWithMetadata,
    ) -> SpelResult {
        Ok(spel_framework::SpelOutput::execute(
            token_program::print_nft::print_nft(master_account, printed_account),
            vec![],
        ))
    }
}
