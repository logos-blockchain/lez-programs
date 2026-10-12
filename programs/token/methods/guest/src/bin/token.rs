#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, ShardData};
use spel_framework::prelude::*;
use token_core::{TokenDescriptor, TokenKind};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

// Two phases. Each `#[plan]` runs once with no account data in sight: it checks
// ids and authorization and declares one effect per shard it means to change.
// Each `#[apply]` then runs once per effect, sees exactly that shard's bytes, and
// returns their replacement — or `None` to leave them alone, which is how a guard
// reads a shard without writing it.
//
// Three v0.2.5 checks necessarily moved into the apply halves, because the plan
// phase can no longer see what they examined:
//   * `#[account(init)]` — whether a shard is empty.
//   * `program_owner == self` — now implied, since declaring an effect on a shard asserts the shard
//     is this program's.
//   * the stored mint authority, and anything else cross-checked between two accounts. Those become
//     declared values the owning shard's apply pins.
#[lez_program(instruction = "token_core::Instruction")]
mod token {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    // ── Transfer ────────────────────────────────────────────────────────────

    /// Transfer tokens from sender to recipient.
    ///
    /// `descriptor` names the asset. Neither side's apply can see the other's
    /// shard, so each asserts its own holding matches the descriptor; together
    /// that gives what the old single-function cross-check gave directly.
    #[plan]
    pub fn transfer(
        #[account(mut, signer)] sender: AccountMeta,
        #[account(mut)] recipient: AccountMeta,
        amount_to_transfer: u128,
        descriptor: TokenDescriptor,
    ) -> SpelResult {
        token_program::transfer::authorize(&sender);
        Ok(SpelOutput::empty()
            .effect(
                &sender,
                sender.program_account_id,
                &Effect::Withdraw(token_core::Withdraw {
                    descriptor,
                    amount: amount_to_transfer,
                }),
            )
            .effect(
                &recipient,
                recipient.program_account_id,
                &Effect::Deposit(token_core::Deposit {
                    descriptor,
                    amount: amount_to_transfer,
                }),
            ))
    }

    #[apply]
    pub fn withdraw(effect: token_core::Withdraw, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::transfer::withdraw(
            pre_data,
            &effect.descriptor,
            effect.amount,
        ))
    }

    #[apply]
    pub fn deposit(effect: token_core::Deposit, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::transfer::deposit(
            pre_data,
            &effect.descriptor,
            effect.amount,
        ))
    }

    // ── Creation ────────────────────────────────────────────────────────────

    /// Create a new fungible token definition without metadata.
    ///
    /// Reads nothing, so the plan builds both records outright. Emptiness of each
    /// target is asserted by `create` in the apply phase.
    #[plan]
    pub fn new_fungible_definition(
        #[account(init, signer)] definition_target_account: AccountMeta,
        #[account(init, signer)] holding_target_account: AccountMeta,
        name: String,
        total_supply: u128,
        mint_authority: Option<AccountId>,
    ) -> SpelResult {
        token_program::new_definition::authorize_targets(&[
            &definition_target_account,
            &holding_target_account,
        ]);
        let (definition, holding) = token_program::new_definition::fungible_records(
            definition_target_account.account_id,
            name,
            total_supply,
            mint_authority,
        );
        Ok(SpelOutput::empty()
            .effect(
                &definition_target_account,
                definition_target_account.program_account_id,
                &Effect::Create(token_core::Create { data: definition }),
            )
            .effect(
                &holding_target_account,
                holding_target_account.program_account_id,
                &Effect::Create(token_core::Create { data: holding }),
            ))
    }

    /// Create a new fungible or non-fungible token definition with metadata.
    #[expect(
        clippy::boxed_local,
        reason = "boxed metadata keeps the instruction argument size bounded on the stack"
    )]
    #[plan]
    pub fn new_definition_with_metadata(
        #[account(init, signer)] definition_target_account: AccountMeta,
        #[account(init, signer)] holding_target_account: AccountMeta,
        #[account(init, signer)] metadata_target_account: AccountMeta,
        new_definition: token_core::NewTokenDefinition,
        metadata: Box<token_core::NewTokenMetadata>,
    ) -> SpelResult {
        token_program::new_definition::authorize_targets(&[
            &definition_target_account,
            &holding_target_account,
            &metadata_target_account,
        ]);
        let (definition, holding, meta) = token_program::new_definition::records_with_metadata(
            definition_target_account.account_id,
            metadata_target_account.account_id,
            new_definition,
            *metadata,
        );
        Ok(SpelOutput::empty()
            .effect(
                &definition_target_account,
                definition_target_account.program_account_id,
                &Effect::Create(token_core::Create { data: definition }),
            )
            .effect(
                &holding_target_account,
                holding_target_account.program_account_id,
                &Effect::Create(token_core::Create { data: holding }),
            )
            .effect(
                &metadata_target_account,
                metadata_target_account.program_account_id,
                &Effect::Create(token_core::Create { data: meta }),
            ))
    }

    #[apply]
    pub fn create(effect: token_core::Create, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::new_definition::create(pre_data, effect.data))
    }

    // ── InitializeAccount ───────────────────────────────────────────────────

    /// Initialize a token holding account for a given token definition.
    ///
    /// `kind` is the definition's, but the holding's apply never sees the
    /// definition — so the definition's own apply guards the claim and writes
    /// nothing.
    #[plan]
    pub fn initialize_account(
        definition_account: AccountMeta,
        #[account(init)] account_to_initialize: AccountMeta,
        kind: TokenKind,
    ) -> SpelResult {
        Ok(SpelOutput::empty()
            .guard(
                &definition_account,
                definition_account.program_account_id,
                &Effect::CheckHoldingKind(token_core::CheckHoldingKind { kind }),
            )
            .effect(
                &account_to_initialize,
                account_to_initialize.program_account_id,
                &Effect::InitializeHolding(token_core::InitializeHolding {
                    descriptor: TokenDescriptor {
                        definition_id: definition_account.account_id,
                        kind,
                    },
                    is_authorized: account_to_initialize.is_authorized,
                }),
            ))
    }

    #[apply]
    pub fn check_holding_kind(
        effect: token_core::CheckHoldingKind,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        token_program::initialize::check_holding_kind(pre_data, effect.kind);
        None
    }

    #[apply]
    pub fn initialize_holding(
        effect: token_core::InitializeHolding,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(token_program::initialize::initialize_holding(
            pre_data,
            &effect.descriptor,
            effect.is_authorized,
        ))
    }

    // ── Burn ────────────────────────────────────────────────────────────────

    /// Burn tokens from the holder's account.
    #[plan]
    pub fn burn(
        #[account(mut)] definition_account: AccountMeta,
        #[account(mut, signer)] user_holding_account: AccountMeta,
        amount_to_burn: u128,
        kind: TokenKind,
    ) -> SpelResult {
        token_program::burn::authorize(&user_holding_account);
        Ok(SpelOutput::empty()
            .effect(
                &definition_account,
                definition_account.program_account_id,
                &Effect::BurnSupply(token_core::BurnSupply {
                    kind,
                    amount: amount_to_burn,
                }),
            )
            .effect(
                &user_holding_account,
                user_holding_account.program_account_id,
                &Effect::BurnHolding(token_core::BurnHolding {
                    descriptor: TokenDescriptor {
                        definition_id: definition_account.account_id,
                        kind,
                    },
                    amount: amount_to_burn,
                }),
            ))
    }

    #[apply]
    pub fn burn_supply(effect: token_core::BurnSupply, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::burn::burn_supply(
            pre_data,
            effect.kind,
            effect.amount,
        ))
    }

    #[apply]
    pub fn burn_holding(
        effect: token_core::BurnHolding,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(token_program::burn::burn_holding(
            pre_data,
            &effect.descriptor,
            effect.amount,
        ))
    }

    // ── Mint ────────────────────────────────────────────────────────────────

    /// Mint new tokens under self/PDA authority: the definition account itself is
    /// the current mint authority and signs (or is PDA-authorized, e.g. the AMM
    /// minting its own LP token).
    #[plan]
    pub fn mint(
        #[account(mut, signer)] definition_account: AccountMeta,
        #[account(mut)] user_holding_account: AccountMeta,
        amount_to_mint: u128,
    ) -> SpelResult {
        Ok(mint_effects(
            &definition_account,
            &user_holding_account,
            &definition_account,
            amount_to_mint,
        ))
    }

    /// Mint new tokens under an external authority: a distinct `authority_account`
    /// (the account the mint authority was rotated to) signs, while the definition
    /// account is mutated but does not sign.
    #[plan]
    pub fn mint_with_authority(
        #[account(mut)] definition_account: AccountMeta,
        #[account(mut)] user_holding_account: AccountMeta,
        #[account(signer)] authority_account: AccountMeta,
        amount_to_mint: u128,
    ) -> SpelResult {
        Ok(mint_effects(
            &definition_account,
            &user_holding_account,
            &authority_account,
            amount_to_mint,
        ))
    }

    /// Shared effect set for both mint authority modes.
    ///
    /// The plan can only report *which* account claims authority and whether it
    /// authorized; whether that is the real authority is stored in the definition's
    /// shard, so the comparison travels into `mint_supply`.
    fn mint_effects(
        definition_account: &AccountMeta,
        user_holding_account: &AccountMeta,
        authority: &AccountMeta,
        amount_to_mint: u128,
    ) -> SpelOutput {
        SpelOutput::empty()
            .effect(
                definition_account,
                definition_account.program_account_id,
                &Effect::MintSupply(token_core::MintSupply {
                    amount: amount_to_mint,
                    authority_id: authority.account_id,
                    authority_is_authorized: authority.is_authorized,
                }),
            )
            .effect(
                user_holding_account,
                user_holding_account.program_account_id,
                &Effect::Deposit(token_core::Deposit {
                    descriptor: TokenDescriptor {
                        definition_id: definition_account.account_id,
                        kind: TokenKind::Fungible,
                    },
                    amount: amount_to_mint,
                }),
            )
    }

    #[apply]
    pub fn mint_supply(effect: token_core::MintSupply, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::mint::mint_supply(
            pre_data,
            effect.amount,
            effect.authority_id,
            effect.authority_is_authorized,
        ))
    }

    // ── PrintNft ────────────────────────────────────────────────────────────

    /// Print a new NFT from the master copy.
    #[plan]
    pub fn print_nft(
        #[account(mut, signer)] master_account: AccountMeta,
        #[account(init)] printed_account: AccountMeta,
        definition_id: AccountId,
    ) -> SpelResult {
        token_program::print_nft::authorize(&master_account);
        let copy = ShardData::from(&token_core::TokenHolding::NftPrintedCopy {
            definition_id,
            owned: true,
        });
        Ok(SpelOutput::empty()
            .effect(
                &master_account,
                master_account.program_account_id,
                &Effect::PrintCopy(token_core::PrintCopy { definition_id }),
            )
            .effect(
                &printed_account,
                printed_account.program_account_id,
                &Effect::Create(token_core::Create { data: copy }),
            ))
    }

    #[apply]
    pub fn print_copy(effect: token_core::PrintCopy, pre_data: &ShardData) -> Option<ShardData> {
        Some(token_program::print_nft::print_copy(
            pre_data,
            effect.definition_id,
        ))
    }

    // ── SetAuthority ────────────────────────────────────────────────────────

    /// Rotate or renounce the mint authority under self/PDA authority.
    /// Pass `new_authority: None` to permanently renounce minting (fixed supply).
    #[plan]
    pub fn set_authority(
        #[account(mut, signer)] definition_account: AccountMeta,
        new_authority: Option<AccountId>,
    ) -> SpelResult {
        Ok(set_authority_effect(
            &definition_account,
            &definition_account,
            new_authority,
        ))
    }

    /// Rotate or renounce the mint authority under an external authority.
    #[plan]
    pub fn set_authority_with_authority(
        #[account(mut)] definition_account: AccountMeta,
        #[account(signer)] authority_account: AccountMeta,
        new_authority: Option<AccountId>,
    ) -> SpelResult {
        Ok(set_authority_effect(
            &definition_account,
            &authority_account,
            new_authority,
        ))
    }

    /// Shared effect for both authority modes; see [`mint_effects`] for why the
    /// authority comparison cannot happen here.
    fn set_authority_effect(
        definition_account: &AccountMeta,
        authority: &AccountMeta,
        new_authority: Option<AccountId>,
    ) -> SpelOutput {
        SpelOutput::empty().effect(
            definition_account,
            definition_account.program_account_id,
            &Effect::SetMintAuthority(token_core::SetMintAuthority {
                new_authority,
                authority_id: authority.account_id,
                authority_is_authorized: authority.is_authorized,
            }),
        )
    }

    #[apply]
    pub fn set_mint_authority(
        effect: token_core::SetMintAuthority,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(token_program::set_authority::set_mint_authority(
            pre_data,
            effect.new_authority,
            effect.authority_id,
            effect.authority_is_authorized,
        ))
    }
}
