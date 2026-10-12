//! `Instruction::OpenPosition` (spec §10.4).

use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{Position, ProtocolParameters};
use token_core::{TokenDescriptor, TokenKind};

/// Plan half: derive the position and vault addresses, build the position
/// record, and chain the two token calls that materialize and fund the vault.
///
/// The position starts with no debt — spec §10.4 deliberately omits an
/// initial-debt parameter; borrowing is a separate `GenerateDebt`.
///
/// `!is_frozen` survives as a plan-phase check even though the plan cannot read
/// the flag: it is read off the declared parameters, which the parameters guard
/// pins to the shard. A declared value a guard pins is one the plan may branch
/// on, and that is what keeps the freeze gate where it belongs.
///
/// Three v0.2.5 reads are gone with nothing declared in their place, because the
/// chained calls already carry what they established. The vault's token is pinned
/// by `InitializeAccount`, which names the collateral definition; the user
/// holding's token is pinned by the `Transfer` descriptor; and the Token Program
/// is pinned by [`assert_token_program`](crate::shared::assert_token_program)
/// against the bound one.
///
/// # Panics
/// - `owner` or `user_collateral_holding` did not sign.
/// - `protocol_parameters` is not at its canonical PDA.
/// - `clock` is not the system `CLOCK_01` shard.
/// - the protocol is frozen.
/// - `collateral_definition` is not the bound collateral definition.
/// - any token slot does not select the bound Token Program's shard.
/// - `position` or `vault` is not at its PDA derivation.
#[expect(
    clippy::too_many_arguments,
    reason = "the account inputs mirror the spec §10.4 ABI, plus the declared parameters and clock"
)]
#[must_use]
pub fn plan_open_position(
    owner: &AccountMeta,
    position: &AccountMeta,
    vault: &AccountMeta,
    user_collateral_holding: &AccountMeta,
    collateral_definition: &AccountMeta,
    protocol_parameters: &AccountMeta,
    clock: &AccountMeta,
    parameters: &ProtocolParameters,
    now: u64,
    position_nonce: u64,
    initial_collateral_amount: u128,
    stablecoin_program_id: AccountId,
) -> (ShardData, Vec<ChainedCall>) {
    assert!(owner.is_authorized, "Owner authorization is missing");
    assert!(
        user_collateral_holding.is_authorized,
        "User collateral holding authorization is missing"
    );
    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_canonical_clock(clock);
    assert!(!parameters.is_frozen, "Protocol is frozen");
    assert_eq!(
        collateral_definition.account_id, parameters.collateral_definition_id,
        "Collateral definition does not match the one bound at initialize_program"
    );
    crate::shared::assert_token_program(collateral_definition, parameters, "Collateral definition");
    crate::shared::assert_token_program(
        user_collateral_holding,
        parameters,
        "User collateral holding",
    );
    crate::shared::assert_token_program(vault, parameters, "Position vault");

    let vault_seed = crate::shared::assert_position_and_vault_slots(
        position,
        vault,
        owner,
        position_nonce,
        stablecoin_program_id,
    );

    let position_data = ShardData::from(&Position {
        owner_account_id: owner.account_id,
        position_nonce,
        vault_account_id: vault.account_id,
        collateral_amount: initial_collateral_amount,
        normalized_debt_amount: 0,
        opened_at: now,
    });

    let token_program_id = parameters.token_program_id;
    let descriptor = TokenDescriptor {
        definition_id: parameters.collateral_definition_id,
        kind: TokenKind::Fungible,
    };

    // Materialize the vault as a Token holding for the collateral. The vault's
    // PDA seed is the whole of the authority — a chained call ships shard
    // selectors, not authorized accounts.
    let initialize_call = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(collateral_definition.account_id, token_program_id),
            ProgramShardSelector::new(vault.account_id, token_program_id),
        ],
        &token_core::Instruction::InitializeAccount {
            kind: TokenKind::Fungible,
        },
    )
    .with_pda_seeds(vec![vault_seed]);

    // Then fund it. The sender is the user's own holding, authorized by the
    // transaction's witness set, so this call carries no seed. The runtime
    // resolves the vault's post-`InitializeAccount` state from the transaction's
    // own diff, so nothing has to be hand-built.
    let transfer_call = crate::shared::transfer_call(
        token_program_id,
        user_collateral_holding.account_id,
        vault.account_id,
        descriptor,
        initial_collateral_amount,
    );

    (position_data, vec![initialize_call, transfer_call])
}
