//! Plan-phase checks and apply halves shared across the instruction set.
//!
//! Three families of check from the v0.2.5 version are not here, because what
//! they asked is no longer askable — or no longer worth asking:
//!
//! * `account.program_owner == stablecoin_program_id` on every global. A shard is keyed by the
//!   program that wrote it, so emitting an effect on `shards[self]` *is* that assertion.
//! * `account != Account::default()` on every global. Emptiness is visible only to an apply half,
//!   and each guard below gets it for free: an empty shard does not decode.
//! * `token_program_id = account.program_owner`. The plan can see
//!   `AccountMeta::program_account_id`, which is precisely "which program's shard this slot
//!   selects" — so routing a chained call through the same Token Program as the definition is still
//!   a plan-phase check, just a structural one.
//!
//! What stays in the plan is every *address* assertion. Under v0.3.0 those carry
//! more weight, not less: a guard proves a declared value matches *some* shard,
//! so the address check is what makes it the right one.

use clock_core::{clock_account_id, ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee_core::{
    account::{AccountId, ShardData},
    program::{AccountMeta, PdaSeed},
};
use stablecoin_core::{
    compute_protocol_parameters_pda, compute_redemption_price_state_pda,
    compute_stability_fee_accumulator_pda, verify_position_and_get_seed,
    verify_position_vault_and_get_seed, CheckHoldingBalance, CheckOraclePair, Position,
    PositionIdentity, ProtocolParameters, RedemptionPriceState, StabilityFeeAccumulator,
};
use token_core::{TokenDefinition, TokenHolding};
use twap_oracle_core::OraclePriceAccount;

// ─── plan half ──────────────────────────────────────────────────────────────

/// The clock slot must be the canonical 1-block LEZ clock.
///
/// Both halves of the identity matter. v0.3.0 keys a shard by the program that
/// wrote it, so the timestamp lives in `shards[clock_account_id()]` *at* the
/// `CLOCK_01` address; a selector naming that address under some other program
/// would hand [`check_clock`] bytes that program authored, and pinning `now`
/// against those is pinning it against a forgery.
///
/// # Panics
/// If `clock` is not [`CLOCK_01_PROGRAM_ACCOUNT_ID`], or does not select the
/// clock program's shard of it.
pub fn assert_canonical_clock(clock: &AccountMeta) {
    assert_eq!(
        clock.account_id, CLOCK_01_PROGRAM_ACCOUNT_ID,
        "Clock account must be the system CLOCK_01 account"
    );
    assert_eq!(
        clock.program_account_id,
        clock_account_id(),
        "Clock account must select the clock program's shard"
    );
}

/// A global's slot must sit at its canonical PDA.
///
/// Ownership is structural and emptiness belongs to the guard, so the address is
/// all that is left to check here — and it is the load-bearing part: without it,
/// any stablecoin-owned account that happened to decode as the record could
/// stand in for the global.
///
/// # Panics
/// If `slot` is not at `expected_id`.
fn assert_global_slot(slot: &AccountMeta, expected_id: AccountId, label: &str) {
    assert_eq!(
        slot.account_id, expected_id,
        "{label} account ID does not match expected PDA derivation"
    );
}

/// The `ProtocolParameters` slot must sit at its canonical PDA.
///
/// # Panics
/// If it does not.
pub fn assert_parameters_slot(parameters: &AccountMeta, stablecoin_program_id: AccountId) {
    assert_global_slot(
        parameters,
        compute_protocol_parameters_pda(stablecoin_program_id),
        "ProtocolParameters",
    );
}

/// The `StabilityFeeAccumulator` slot must sit at its canonical PDA.
///
/// # Panics
/// If it does not.
pub fn assert_accumulator_slot(accumulator: &AccountMeta, stablecoin_program_id: AccountId) {
    assert_global_slot(
        accumulator,
        compute_stability_fee_accumulator_pda(stablecoin_program_id),
        "StabilityFeeAccumulator",
    );
}

/// The `RedemptionPriceState` slot must sit at its canonical PDA.
///
/// # Panics
/// If it does not.
pub fn assert_redemption_slot(redemption: &AccountMeta, stablecoin_program_id: AccountId) {
    assert_global_slot(
        redemption,
        compute_redemption_price_state_pda(stablecoin_program_id),
        "RedemptionPriceState",
    );
}

/// The oracle slot must be the handle bound in the declared parameters.
///
/// The declaration is pinned by the parameters guard, so branching on it here is
/// sound — see [`stablecoin_core::CheckParameters`].
///
/// # Panics
/// If `oracle` is not `parameters.market_price_oracle_id`.
pub fn assert_oracle_slot(oracle: &AccountMeta, parameters: &ProtocolParameters) {
    assert_eq!(
        oracle.account_id, parameters.market_price_oracle_id,
        "Market price oracle account_id does not match ProtocolParameters.market_price_oracle_id"
    );
}

/// The position slot must sit at the `(owner, position_nonce)` PDA.
///
/// `position_nonce` is declared rather than read — every position instruction
/// except `OpenPosition` used to take it out of the stored record — and this
/// assertion is what checks it, since a wrong nonce derives a different address.
/// The stored `vault_account_id` is pinned separately, by
/// [`position_of`]'s identity comparison.
///
/// # Panics
/// If the address does not match its derivation.
pub fn assert_position_slot(
    position: &AccountMeta,
    owner: &AccountMeta,
    position_nonce: u64,
    stablecoin_program_id: AccountId,
) {
    verify_position_and_get_seed(position, owner, position_nonce, stablecoin_program_id);
}

/// As [`assert_position_slot`], and the vault slot must sit at the vault PDA
/// derived from the position. Returns the vault's seed, which is the whole of the
/// authority for any chained call that debits it.
///
/// Only the four instructions that actually touch the vault take it: `GenerateDebt`
/// and `RepayDebt` move stablecoins, never collateral.
///
/// # Panics
/// If either address does not match its derivation.
pub fn assert_position_and_vault_slots(
    position: &AccountMeta,
    vault: &AccountMeta,
    owner: &AccountMeta,
    position_nonce: u64,
    stablecoin_program_id: AccountId,
) -> PdaSeed {
    assert_position_slot(position, owner, position_nonce, stablecoin_program_id);
    verify_position_vault_and_get_seed(vault, position.account_id, stablecoin_program_id)
}

/// Every Token Program slot must select the shard of the *bound* Token Program.
///
/// This replaces v0.2.5's `definition.program_owner == holding.program_owner`,
/// and is strictly stronger than it. Then, the program was taken from whichever
/// account the caller supplied and the two were only required to agree; now it is
/// the one recorded at bootstrap, and a slot naming any other program's shard is
/// refused — see [`ProtocolParameters::token_program_id`] for what that bought.
///
/// # Panics
/// If `slot` does not select the bound Token Program's shard.
pub fn assert_token_program(slot: &AccountMeta, parameters: &ProtocolParameters, label: &str) {
    assert_eq!(
        slot.program_account_id, parameters.token_program_id,
        "{label} must select the protocol's bound Token Program shard"
    );
}

/// The oracle slot must be the handle the protocol has bound, address *and*
/// program.
///
/// # Panics
/// If either half differs.
pub fn assert_bound_oracle(oracle: &AccountMeta, parameters: &ProtocolParameters) {
    assert_oracle_slot(oracle, parameters);
    assert_eq!(
        oracle.program_account_id, parameters.market_price_oracle_program_id,
        "Market price oracle must select the bound producer's shard"
    );
}

// ─── apply half: guards ─────────────────────────────────────────────────────

/// Guard on the `CLOCK_01` shard: it must read exactly the declared time.
///
/// # Panics
/// If the shard is empty or disagrees with `now`.
pub fn check_clock(pre_data: &ShardData, now: u64) {
    assert!(!pre_data.is_empty(), "Clock account must be initialized");
    assert_eq!(
        ClockAccountData::from_bytes(pre_data.as_ref()).timestamp,
        now,
        "Declared timestamp does not match the system clock"
    );
}

/// Guard on the `ProtocolParameters` shard: every declared field must be stored.
///
/// Also the initialization gate — an empty or undecodable shard means the
/// protocol has not been bootstrapped.
///
/// # Panics
/// If the shard is not a valid record, or any field differs.
pub fn check_parameters(pre_data: &ShardData, declared: &ProtocolParameters) {
    let stored = ProtocolParameters::try_from(pre_data)
        .expect("ProtocolParameters account must be initialized and decode");
    assert_eq!(
        &stored, declared,
        "Declared protocol parameters do not match the ProtocolParameters account"
    );
}

/// Guard on the `StabilityFeeAccumulator` shard: its anchor must be the declared
/// one.
///
/// # Panics
/// If the shard is not a valid record, or the anchor differs.
pub fn check_accumulator(pre_data: &ShardData, declared: &StabilityFeeAccumulator) {
    let stored = StabilityFeeAccumulator::try_from(pre_data)
        .expect("StabilityFeeAccumulator account must be initialized and decode");
    assert_eq!(
        &stored, declared,
        "Declared fee accumulator does not match the StabilityFeeAccumulator account"
    );
}

/// Guard on the `RedemptionPriceState` shard: its anchor must be the declared
/// one.
///
/// # Panics
/// If the shard is not a valid record, or the anchor differs.
pub fn check_redemption(pre_data: &ShardData, declared: &RedemptionPriceState) {
    let stored = RedemptionPriceState::try_from(pre_data)
        .expect("RedemptionPriceState account must be initialized and decode");
    assert_eq!(
        &stored, declared,
        "Declared redemption state does not match the RedemptionPriceState account"
    );
}

/// Guard on the oracle's shard: the observation must be exactly the declared one.
///
/// # Panics
/// If the shard is not a valid `OraclePriceAccount`, or either field differs.
pub fn check_oracle(pre_data: &ShardData, price: u128, timestamp: u64) {
    let stored = OraclePriceAccount::try_from(pre_data)
        .expect("Market price oracle must be initialized and decode as OraclePriceAccount");
    assert_eq!(
        (stored.price, stored.timestamp),
        (price, timestamp),
        "Declared oracle observation does not match the market price oracle account"
    );
}

/// Guard on an oracle's shard: it must quote exactly the declared asset pair.
///
/// # Panics
/// If the shard is not a valid `OraclePriceAccount`, or the pair differs.
pub fn check_oracle_pair(pre_data: &ShardData, pair: &CheckOraclePair) {
    let stored = OraclePriceAccount::try_from(pre_data)
        .expect("Market price oracle must be initialized and decode as OraclePriceAccount");
    assert_eq!(
        stored.base_asset, pair.base_asset,
        "Oracle base_asset must equal the stablecoin definition's account_id"
    );
    assert_eq!(
        stored.quote_asset, pair.quote_asset,
        "Oracle quote_asset must equal the collateral definition's account_id"
    );
}

/// Guard on a Token Program definition shard: it must be a fungible definition.
///
/// # Panics
/// If the shard is not a valid `TokenDefinition`, or is non-fungible.
pub fn check_fungible_definition(pre_data: &ShardData) {
    let stored = TokenDefinition::try_from(pre_data)
        .expect("Collateral definition must be initialized and decode as a TokenDefinition");
    assert!(
        matches!(stored, TokenDefinition::Fungible { .. }),
        "Collateral definition must be Fungible"
    );
}

/// Guard on a Token Program holding shard: it must hold exactly the declared
/// balance of the declared asset.
///
/// # Panics
/// If the shard is not a fungible holding of `descriptor.definition_id`, or its
/// balance differs.
pub fn check_holding_balance(pre_data: &ShardData, guard: &CheckHoldingBalance) {
    let holding = TokenHolding::try_from(pre_data)
        .expect("Vault account must be initialized and hold a valid TokenHolding");
    let TokenHolding::Fungible {
        definition_id,
        balance,
    } = holding
    else {
        panic!("Vault must hold a fungible collateral balance")
    };
    assert_eq!(
        definition_id, guard.descriptor.definition_id,
        "Vault holding does not match the protocol's collateral definition"
    );
    assert_eq!(
        balance, guard.balance,
        "Declared vault balance does not match the vault holding"
    );
}

/// Guard on a Token Program holding shard: it must hold nothing yet.
///
/// # Panics
/// If the shard already holds data.
pub fn check_holding_absent(pre_data: &ShardData) {
    assert!(
        pre_data.is_empty(),
        "Position vault account must be uninitialized"
    );
}

// ─── apply half: shared writes ──────────────────────────────────────────────

/// Write a whole record into a shard that must be empty.
///
/// What `#[account(init)]` and the old `account == Account::default()` assert
/// become: emptiness is visible only here.
///
/// # Panics
/// If the shard already holds data.
#[must_use]
pub fn create(pre_data: &ShardData, data: ShardData) -> ShardData {
    assert!(
        pre_data.is_empty(),
        "Target account must not already hold data"
    );
    data
}

/// Decode a position and pin the declared identity to it.
///
/// Every effect on a position shard starts here, so no handler acts on a declared
/// id the position has not confirmed.
///
/// # Panics
/// If the shard is not a valid [`Position`], or the identity differs.
pub fn position_of(pre_data: &ShardData, expected: &PositionIdentity) -> Position {
    let position = Position::try_from(pre_data)
        .expect("Position account must be initialized and hold valid Position state");
    assert_eq!(
        &PositionIdentity::from(&position),
        expected,
        "Declared position identity does not match the Position account"
    );
    position
}

/// A fungible `Transfer` chained into the Token Program.
///
/// The descriptor is the declare-and-pin device the Token Program uses: each
/// side's apply asserts its own holding matches it, which is how this program's
/// bound collateral and stablecoin definitions become facts about the accounts a
/// caller supplied — the reads that used to establish that are gone.
#[must_use]
pub fn transfer_call(
    token_program_id: AccountId,
    from: AccountId,
    to: AccountId,
    descriptor: token_core::TokenDescriptor,
    amount: u128,
) -> lee_core::program::ChainedCall {
    lee_core::program::ChainedCall::new(
        token_program_id,
        vec![
            lee_core::account::ProgramShardSelector::new(from, token_program_id),
            lee_core::account::ProgramShardSelector::new(to, token_program_id),
        ],
        &token_core::Instruction::Transfer {
            amount_to_transfer: amount,
            descriptor,
        },
    )
}

/// The collateral [`token_core::TokenDescriptor`] for a set of declared
/// parameters.
#[must_use]
pub fn collateral_descriptor(parameters: &ProtocolParameters) -> token_core::TokenDescriptor {
    token_core::TokenDescriptor {
        definition_id: parameters.collateral_definition_id,
        kind: token_core::TokenKind::Fungible,
    }
}

/// The stablecoin [`token_core::TokenDescriptor`] for a set of declared
/// parameters.
#[must_use]
pub fn stablecoin_descriptor(parameters: &ProtocolParameters) -> token_core::TokenDescriptor {
    token_core::TokenDescriptor {
        definition_id: parameters.stablecoin_definition_id,
        kind: token_core::TokenKind::Fungible,
    }
}
