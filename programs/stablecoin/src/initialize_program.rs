//! `Instruction::InitializeProgram` (spec §10.1).
//!
//! Wall-clock time comes from the system `CLOCK_01` account, declared as `now`
//! and pinned by a guard on the clock's own shard. `spel-framework`'s
//! `ProgramContext` exposes no clock, so — like the runtime's own time-locked
//! programs — the timestamp is read out of a clock account (see `clock_core`).

use lee_core::{
    account::{AccountId, ProgramShardSelector, ShardData},
    program::{AccountMeta, ChainedCall},
};
use stablecoin_core::{
    compute_stablecoin_definition_pda, compute_stablecoin_definition_pda_seed,
    compute_stablecoin_master_holding_pda, compute_stablecoin_master_holding_pda_seed,
    math::FIXED_POINT_ONE, CheckOraclePair, ProtocolParameters, RedemptionPriceState,
    StabilityFeeAccumulator,
};

/// Numerical parameters for [`plan_initialize_program`]. Bundled to keep the
/// argument count manageable; this is a parameter bag, not state.
#[derive(Debug)]
pub struct InitializeProgramParams<'a> {
    pub freeze_authority_account_id: AccountId,
    pub initial_stability_fee_per_millisecond: u128,
    pub initial_controller_proportional_gain: i128,
    pub initial_controller_integral_gain: i128,
    pub initial_minimum_collateralization_ratio: u128,
    pub minimum_milliseconds_between_rate_updates: u64,
    pub maximum_oracle_price_age_milliseconds: u64,
    pub initial_redemption_price: u128,
    pub stablecoin_name: &'a str,
}

/// The three global records this instruction writes, each into a shard that must
/// be empty.
#[derive(Debug)]
pub struct Globals {
    pub parameters: ProtocolParameters,
    pub accumulator: StabilityFeeAccumulator,
    pub redemption: RedemptionPriceState,
}

/// Plan half: bootstrap a fresh protocol instance.
///
/// The returned [`CheckOraclePair`] is what the oracle's own guard asserts, and
/// it is the only one of the old reads that needs a payload: the pair it compares
/// against is built from account ids the plan can see.
///
/// The §8 bands are asserted here rather than in an apply, because every input
/// they bound is an instruction field — nothing has to be read to check them.
///
/// Two values are recorded that v0.2.5 did not keep: `token_program_id` and
/// `market_price_oracle_program_id`. Under v0.3.0 a slot names *which program's
/// shard* it selects, so without them a caller could route a chained `Burn`
/// through a program of their own choosing and have a position's debt fall
/// against bytes they authored — see [`ProtocolParameters`].
///
/// # Panics
/// - `admin` did not sign.
/// - any of the five PDAs is not at its derivation, or the clock is not `CLOCK_01`.
/// - any numerical parameter is outside its §8 band, `initial_redemption_price` is zero, or
///   `stablecoin_name` is empty.
#[expect(
    clippy::too_many_arguments,
    reason = "the nine account inputs, program id, clock and parameter bag mirror the spec §10.1 ABI"
)]
#[must_use]
pub fn plan_initialize_program(
    admin: &AccountMeta,
    protocol_parameters: &AccountMeta,
    stability_fee_accumulator: &AccountMeta,
    redemption_price_state: &AccountMeta,
    stablecoin_definition: &AccountMeta,
    stablecoin_master_holding: &AccountMeta,
    collateral_definition: &AccountMeta,
    market_price_oracle: &AccountMeta,
    clock: &AccountMeta,
    now: u64,
    params: &InitializeProgramParams<'_>,
    stablecoin_program_id: AccountId,
) -> (Globals, CheckOraclePair, ChainedCall) {
    assert!(admin.is_authorized, "Admin authorization is missing");

    crate::shared::assert_parameters_slot(protocol_parameters, stablecoin_program_id);
    crate::shared::assert_accumulator_slot(stability_fee_accumulator, stablecoin_program_id);
    crate::shared::assert_redemption_slot(redemption_price_state, stablecoin_program_id);
    assert_eq!(
        stablecoin_definition.account_id,
        compute_stablecoin_definition_pda(stablecoin_program_id),
        "StablecoinDefinition account ID does not match expected PDA derivation"
    );
    assert_eq!(
        stablecoin_master_holding.account_id,
        compute_stablecoin_master_holding_pda(stablecoin_program_id),
        "StablecoinMasterHolding account ID does not match expected PDA derivation"
    );
    crate::shared::assert_canonical_clock(clock);

    crate::checks::assert_stability_fee_in_band(
        params.initial_stability_fee_per_millisecond,
        "initial_stability_fee_per_millisecond",
    );
    crate::checks::assert_collateralization_ratio_in_band(
        params.initial_minimum_collateralization_ratio,
        "initial_minimum_collateralization_ratio",
    );
    crate::checks::assert_controller_gains_in_band(
        params.initial_controller_proportional_gain,
        params.initial_controller_integral_gain,
    );
    crate::checks::assert_timing_milliseconds_in_band(
        params.minimum_milliseconds_between_rate_updates,
        "minimum_milliseconds_between_rate_updates",
    );
    crate::checks::assert_timing_milliseconds_in_band(
        params.maximum_oracle_price_age_milliseconds,
        "maximum_oracle_price_age_milliseconds",
    );
    assert!(
        params.initial_redemption_price > 0,
        "initial_redemption_price must be positive"
    );
    assert!(
        !params.stablecoin_name.is_empty(),
        "stablecoin_name must be non-empty"
    );

    // The Token Program and the oracle's producer are taken from the slots that
    // carry them, which is the one moment this is sound: a caller naming the
    // wrong program here binds a protocol nobody else's tokens can enter, and
    // every later instruction is checked against what is recorded now.
    let token_program_id = collateral_definition.program_account_id;
    let globals = Globals {
        parameters: ProtocolParameters {
            admin_account_id: admin.account_id,
            freeze_authority_account_id: params.freeze_authority_account_id,
            stablecoin_definition_id: stablecoin_definition.account_id,
            collateral_definition_id: collateral_definition.account_id,
            market_price_oracle_id: market_price_oracle.account_id,
            token_program_id,
            market_price_oracle_program_id: market_price_oracle.program_account_id,
            stability_fee_per_millisecond: params.initial_stability_fee_per_millisecond,
            controller_proportional_gain: params.initial_controller_proportional_gain,
            controller_integral_gain: params.initial_controller_integral_gain,
            minimum_collateralization_ratio: params.initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates: params
                .minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds: params.maximum_oracle_price_age_milliseconds,
            is_frozen: false,
        },
        accumulator: StabilityFeeAccumulator {
            accumulated_rate_at_last_accrual: FIXED_POINT_ONE,
            last_accrued_at: now,
        },
        redemption: RedemptionPriceState {
            redemption_price_at_last_update: params.initial_redemption_price,
            redemption_rate_per_millisecond: FIXED_POINT_ONE,
            controller_integral_term: 0,
            last_updated_at: now,
        },
    };

    let oracle_pair = CheckOraclePair {
        base_asset: stablecoin_definition.account_id,
        quote_asset: collateral_definition.account_id,
    };

    // The chained call carries shard selectors; the two PDA seeds are what
    // authorize the Token Program over the definition and its master holding.
    let new_definition_call = ChainedCall::new(
        token_program_id,
        vec![
            ProgramShardSelector::new(stablecoin_definition.account_id, token_program_id),
            ProgramShardSelector::new(stablecoin_master_holding.account_id, token_program_id),
        ],
        &token_core::Instruction::NewFungibleDefinition {
            name: params.stablecoin_name.to_owned(),
            total_supply: 0,
            // Self/PDA authority: the definition account is its own mint
            // authority, so `generate_debt` and `repay_debt` can mint and burn by
            // presenting the stablecoin definition's PDA seed.
            mint_authority: Some(stablecoin_definition.account_id),
        },
    )
    .with_pda_seeds(vec![
        compute_stablecoin_definition_pda_seed(),
        compute_stablecoin_master_holding_pda_seed(),
    ]);

    (globals, oracle_pair, new_definition_call)
}

impl Globals {
    /// The three records as the [`ShardData`] each shard's `create` writes.
    #[must_use]
    pub fn shard_data(&self) -> [ShardData; 3] {
        [
            ShardData::from(&self.parameters),
            ShardData::from(&self.accumulator),
            ShardData::from(&self.redemption),
        ]
    }
}
