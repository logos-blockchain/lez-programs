#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, ShardData};
use spel_framework::{context::ProgramContext, prelude::*};
use stablecoin_core::{
    AccrueFee, AdvanceRedemption, CheckHoldingBalance, CheckOracle, CheckOraclePair,
    CheckParameters, CollateralizationInputs, FeeProjection, PositionIdentity, ProtocolParameters,
    RedemptionPriceState, StabilityFeeAccumulator,
};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

// Two phases. Each `#[plan]` runs once with no account data in sight: it checks
// ids and authorization, derives addresses, enforces the §8 parameter bands, and
// declares one effect per shard it means to touch. Each `#[apply]` then runs once
// per effect, sees exactly that shard's bytes, and returns their replacement — or
// `None` to leave them alone, which is how a guard reads a shard without writing
// it.
//
// Fourteen of these nineteen instructions read `ProtocolParameters` and most read
// a second or third global besides. None of them can any more, so each record
// became a declaration its own shard's guard pins: `parameters` against
// `CheckParameters`, the two anchors against `CheckAccumulator` and
// `CheckRedemption`, the market observation against `CheckOracle`, and the
// timestamp against `CheckClock`. A declared value a guard pins is one the *plan*
// may branch on, which is how `!is_frozen` and the bound-definition checks stay
// where they belong.
//
// No shard ever carries both a guard and a write. Where a record is being
// rewritten the write's own apply does the pinning — the admin setters compare
// the stored admin, `UpdateParameters` compares the fields a change depends on —
// so there is nothing for a second effect to add.
//
// The order of the `#[plan]` functions below is load-bearing and must match the
// `Instruction` enum's variant order: the IDL carries no discriminant, so
// `spel-cli` uses an instruction's *position* in the IDL — which is this source
// order — as the Borsh variant tag. That is why `withdraw_collateral` and
// `repay_debt` sit after the admin setters rather than beside the rest of the
// position lifecycle.
//
// The checks that simply disappeared are worth naming. `program_owner == self` on
// every global is structural now: a shard is keyed by the program that wrote it,
// so emitting an effect on `shards[self]` *is* that assertion. `account !=
// Account::default()` is gone the same way — an empty shard does not decode, so
// every guard is also the initialization gate. And one check got *stronger*:
// "which Token Program" used to be read off whichever account the caller
// supplied, and is now the one recorded at bootstrap.
#[lez_program(instruction = "stablecoin_core::Instruction")]
mod stablecoin {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    // ── Bootstrap ───────────────────────────────────────────────────────────

    /// Bootstraps the protocol: the three global records, the stablecoin's own
    /// Token definition, and the bindings every later instruction is checked
    /// against (spec §10.1).
    ///
    /// The two bindings that are new in v0.3.0 are the Token Program's and the
    /// oracle producer's shard keys. They are taken from the slots that carry
    /// them, which is sound at exactly this one moment: a caller naming the wrong
    /// program binds a protocol nobody else's tokens can enter, and every later
    /// instruction is checked against what is recorded here.
    ///
    /// Expected accounts:
    /// 1. `admin` — signs; becomes `ProtocolParameters.admin_account_id`.
    /// 2. `protocol_parameters` — the PDA, shard still empty.
    /// 3. `stability_fee_accumulator` — the PDA, shard still empty.
    /// 4. `redemption_price_state` — the PDA, shard still empty.
    /// 5. `stablecoin_definition` — the PDA; claimed by the chained Token call.
    /// 6. `stablecoin_master_holding` — the PDA; also claimed by that call.
    /// 7. `collateral_definition` — guarded as a fungible Token definition.
    /// 8. `market_price_oracle` — guarded as quoting the right asset pair.
    /// 9. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires nine explicit accounts plus the §8 parameters"
    )]
    #[plan]
    pub fn initialize_program(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(init)] protocol_parameters: AccountMeta,
        #[account(init)] stability_fee_accumulator: AccountMeta,
        #[account(init)] redemption_price_state: AccountMeta,
        #[account(init)] stablecoin_definition: AccountMeta,
        #[account(init)] stablecoin_master_holding: AccountMeta,
        collateral_definition: AccountMeta,
        market_price_oracle: AccountMeta,
        clock: AccountMeta,
        freeze_authority_account_id: AccountId,
        initial_stability_fee_per_millisecond: u128,
        initial_controller_proportional_gain: i128,
        initial_controller_integral_gain: i128,
        initial_minimum_collateralization_ratio: u128,
        minimum_milliseconds_between_rate_updates: u64,
        maximum_oracle_price_age_milliseconds: u64,
        initial_redemption_price: u128,
        stablecoin_name: String,
        now: u64,
    ) -> SpelResult {
        let params = stablecoin_program::initialize_program::InitializeProgramParams {
            freeze_authority_account_id,
            initial_stability_fee_per_millisecond,
            initial_controller_proportional_gain,
            initial_controller_integral_gain,
            initial_minimum_collateralization_ratio,
            minimum_milliseconds_between_rate_updates,
            maximum_oracle_price_age_milliseconds,
            initial_redemption_price,
            stablecoin_name: &stablecoin_name,
        };
        let (globals, oracle_pair, call) =
            stablecoin_program::initialize_program::plan_initialize_program(
                &admin,
                &protocol_parameters,
                &stability_fee_accumulator,
                &redemption_price_state,
                &stablecoin_definition,
                &stablecoin_master_holding,
                &collateral_definition,
                &market_price_oracle,
                &clock,
                now,
                &params,
                ctx.self_account_id,
            );
        let [parameters_data, accumulator_data, redemption_data] = globals.shard_data();
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::Create(stablecoin_core::Create {
                    data: parameters_data,
                }),
            )
            .effect(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::Create(stablecoin_core::Create {
                    data: accumulator_data,
                }),
            )
            .effect(
                &redemption_price_state,
                redemption_price_state.program_account_id,
                &Effect::Create(stablecoin_core::Create {
                    data: redemption_data,
                }),
            )
            .guard(
                &collateral_definition,
                collateral_definition.program_account_id,
                &Effect::CheckFungibleDefinition(stablecoin_core::CheckFungibleDefinition),
            )
            .guard(
                &market_price_oracle,
                market_price_oracle.program_account_id,
                &Effect::CheckOraclePair(oracle_pair),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    // ── Permissionless pokes ────────────────────────────────────────────────

    /// Advances the global stability-fee accumulator to the clock's timestamp
    /// (spec §10.2).
    ///
    /// Idempotent and unthrottled: the read-side projection keeps every position
    /// current regardless of cadence, so a redundant call is a no-op rather than
    /// an error. Allowed while frozen.
    ///
    /// Expected accounts:
    /// 1. `caller` — signs; not retained anywhere.
    /// 2. `protocol_parameters` — guarded; supplies the fee rate.
    /// 3. `stability_fee_accumulator` — advanced.
    /// 4. `clock` — the system `CLOCK_01` account.
    #[plan]
    pub fn accrue_stability_fee(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountMeta,
        protocol_parameters: AccountMeta,
        #[account(mut)] stability_fee_accumulator: AccountMeta,
        clock: AccountMeta,
        parameters: ProtocolParameters,
        now: u64,
    ) -> SpelResult {
        stablecoin_program::accrue_stability_fee::plan_accrue_stability_fee(
            &caller,
            &protocol_parameters,
            &stability_fee_accumulator,
            &clock,
            ctx.self_account_id,
        );
        let accrue = stablecoin_program::accrue_stability_fee::accrue_fee_effect(
            parameters.stability_fee_per_millisecond,
            now,
        );
        Ok(SpelOutput::empty()
            .effect(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::AccrueFee(accrue),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Runs one tick of the redemption-rate controller and re-anchors the
    /// redemption price (spec §10.3).
    ///
    /// Strict: the three gates — interval not yet due, stale observation, zero
    /// price — are failures here and skips in `refresh_globals`. Two of them could
    /// be checked in the plan, but the interval cannot: it needs `last_updated_at`,
    /// which lives in the shard being rewritten. Keeping all three in one place is
    /// what stops the two instructions drifting apart. Allowed while frozen.
    ///
    /// Expected accounts:
    /// 1. `caller` — signs; not retained anywhere.
    /// 2. `protocol_parameters` — guarded; supplies the gains and timing bounds.
    /// 3. `redemption_price_state` — ticked.
    /// 4. `market_price_oracle` — guarded; the bound handle, address and program.
    /// 5. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires five explicit accounts plus the declared globals"
    )]
    #[plan]
    pub fn update_redemption_rate(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountMeta,
        protocol_parameters: AccountMeta,
        #[account(mut)] redemption_price_state: AccountMeta,
        market_price_oracle: AccountMeta,
        clock: AccountMeta,
        parameters: ProtocolParameters,
        oracle_price: u128,
        oracle_timestamp: u64,
        now: u64,
    ) -> SpelResult {
        stablecoin_program::update_redemption_rate::plan_update_redemption_rate(
            &caller,
            &protocol_parameters,
            &redemption_price_state,
            &market_price_oracle,
            &clock,
            &parameters,
            ctx.self_account_id,
        );
        let advance = stablecoin_program::update_redemption_rate::advance_redemption_effect(
            &parameters,
            oracle_price,
            oracle_timestamp,
            now,
        );
        Ok(SpelOutput::empty()
            .effect(
                &redemption_price_state,
                redemption_price_state.program_account_id,
                &Effect::AdvanceRedemption(advance),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &market_price_oracle,
                market_price_oracle.program_account_id,
                &Effect::CheckOracle(CheckOracle {
                    price: oracle_price,
                    timestamp: oracle_timestamp,
                }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Best-effort combined poke: advances both globals in one transaction (spec
    /// §10.3a).
    ///
    /// A LEZ transaction carries exactly one instruction, so this is the only way
    /// to refresh the fee accumulator and the redemption rate together. The fee
    /// accrual always happens; the controller tick happens only if its interval is
    /// due and the observation is fresh and non-zero.
    ///
    /// The "always versus best-effort" split is expressed by the apply halves
    /// rather than decided here: both effects are emitted unconditionally and
    /// `try_advance_redemption` returns `None` — literally "unchanged" — when a
    /// gate blocks. v0.2.5 had to make that decision in the handler and emit an
    /// unchanged diff to report the skip, which it could only do because handlers
    /// still saw account data.
    ///
    /// Expected accounts: the union of the two standalone pokes' (6).
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires six explicit accounts plus the declared globals"
    )]
    #[plan]
    pub fn refresh_globals(
        ctx: ProgramContext,
        #[account(signer)] caller: AccountMeta,
        protocol_parameters: AccountMeta,
        #[account(mut)] stability_fee_accumulator: AccountMeta,
        #[account(mut)] redemption_price_state: AccountMeta,
        market_price_oracle: AccountMeta,
        clock: AccountMeta,
        parameters: ProtocolParameters,
        oracle_price: u128,
        oracle_timestamp: u64,
        now: u64,
    ) -> SpelResult {
        stablecoin_program::refresh_globals::plan_refresh_globals(
            &caller,
            &protocol_parameters,
            &stability_fee_accumulator,
            &redemption_price_state,
            &market_price_oracle,
            &clock,
            &parameters,
            ctx.self_account_id,
        );
        let accrue = stablecoin_program::accrue_stability_fee::accrue_fee_effect(
            parameters.stability_fee_per_millisecond,
            now,
        );
        let advance = stablecoin_program::update_redemption_rate::advance_redemption_effect(
            &parameters,
            oracle_price,
            oracle_timestamp,
            now,
        );
        Ok(SpelOutput::empty()
            .effect(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::AccrueFee(accrue),
            )
            .effect(
                &redemption_price_state,
                redemption_price_state.program_account_id,
                &Effect::TryAdvanceRedemption(advance),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &market_price_oracle,
                market_price_oracle.program_account_id,
                &Effect::CheckOracle(CheckOracle {
                    price: oracle_price,
                    timestamp: oracle_timestamp,
                }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    // ── Position lifecycle ──────────────────────────────────────────────────

    /// Opens a new collateral-only position for the calling owner (spec §10.4).
    ///
    /// The position starts debt-free — §10.4 deliberately omits an initial-debt
    /// parameter; borrowing is a separate `generate_debt`. Blocked while frozen.
    ///
    /// Three v0.2.5 reads are gone with nothing declared in their place, because
    /// the chained calls carry what they established: the vault's token by the
    /// `InitializeAccount`'s own definition slot, the user holding's token by the
    /// `Transfer` descriptor, and the Token Program by the bootstrap binding.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs; becomes `Position.owner_account_id`.
    /// 2. `position` — the `(owner, nonce)` PDA, shard still empty.
    /// 3. `vault` — the vault PDA; guarded as an empty Token holding, then materialized and funded
    ///    by the chained calls.
    /// 4. `user_collateral_holding` — signs; the source of the collateral.
    /// 5. `collateral_definition` — the bound collateral definition.
    /// 6. `protocol_parameters` — guarded; supplies the binding and freeze flag.
    /// 7. `clock` — the system `CLOCK_01` account; stamps `opened_at`.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires seven explicit accounts plus the declared parameters and clock"
    )]
    #[plan]
    pub fn open_position(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(init)] position: AccountMeta,
        #[account(mut)] vault: AccountMeta,
        #[account(mut, signer)] user_collateral_holding: AccountMeta,
        collateral_definition: AccountMeta,
        protocol_parameters: AccountMeta,
        clock: AccountMeta,
        position_nonce: u64,
        initial_collateral_amount: u128,
        parameters: ProtocolParameters,
        now: u64,
    ) -> SpelResult {
        let (data, calls) = stablecoin_program::open_position::plan_open_position(
            &owner,
            &position,
            &vault,
            &user_collateral_holding,
            &collateral_definition,
            &protocol_parameters,
            &clock,
            &parameters,
            now,
            position_nonce,
            initial_collateral_amount,
            ctx.self_account_id,
        );
        Ok(SpelOutput::plan(Vec::new(), calls)
            .effect(
                &position,
                position.program_account_id,
                &Effect::Create(stablecoin_core::Create { data }),
            )
            .guard(
                &vault,
                vault.program_account_id,
                &Effect::CheckHoldingAbsent(stablecoin_core::CheckHoldingAbsent),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Deposits additional collateral into an existing position (spec §10.5).
    ///
    /// Allowed while frozen and with no collateralization check: a deposit can
    /// only improve the position, so §7 keeps this path open in emergencies.
    ///
    /// The position's collateral is *set* to the vault's pre-transfer balance plus
    /// `amount`, not incremented — anyone can transfer straight into a vault, and
    /// incrementing would leave the position permanently below the vault and
    /// strand the difference. `amount = 0` is the recovery path for that case,
    /// which is why the vault's balance is guarded rather than ignored.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs.
    /// 2. `position` — the `(owner, nonce)` PDA; collateral reconciled.
    /// 3. `vault` — guarded at `vault_balance`, then credited by the chained call.
    /// 4. `user_collateral_holding` — signs; the source.
    /// 5. `protocol_parameters` — guarded; supplies the binding.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires five explicit accounts plus the declared parameters and vault balance"
    )]
    #[plan]
    pub fn deposit_collateral(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(mut)] position: AccountMeta,
        #[account(mut)] vault: AccountMeta,
        #[account(mut, signer)] user_collateral_holding: AccountMeta,
        protocol_parameters: AccountMeta,
        amount: u128,
        expected: PositionIdentity,
        vault_balance: u128,
        parameters: ProtocolParameters,
    ) -> SpelResult {
        let call = stablecoin_program::deposit_collateral::plan_deposit_collateral(
            &owner,
            &position,
            &vault,
            &user_collateral_holding,
            &protocol_parameters,
            &parameters,
            expected.position_nonce,
            amount,
            ctx.self_account_id,
        );
        let descriptor = stablecoin_program::shared::collateral_descriptor(&parameters);
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &position,
                position.program_account_id,
                &Effect::PositionAfterDeposit(stablecoin_core::DepositCollateral {
                    expected,
                    vault_balance,
                    amount,
                }),
            )
            .guard(
                &vault,
                vault.program_account_id,
                &Effect::CheckHoldingBalance(CheckHoldingBalance {
                    descriptor,
                    balance: vault_balance,
                }),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            ))
    }

    /// Mints stablecoins against a position, increasing its debt (spec §10.7).
    ///
    /// Blocked while frozen. The normalized-debt delta is rounded **up** (§6.3)
    /// and the §6.2 invariant is re-checked after the mint, against the fee
    /// accumulator and redemption price both projected to `now` (§5.3). The
    /// oracle is a liveness gate only — §10.7 never consumes its price — and that
    /// gate moves into the plan, where it now belongs: both halves of the
    /// comparison are declared and pinned elsewhere.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs.
    /// 2. `position` — the `(owner, nonce)` PDA; debt grown, then §6.2 re-checked.
    /// 3. `stablecoin_definition` — the bound definition; minted via the chained call, authorized
    ///    by its own PDA seed.
    /// 4. `user_stablecoin_holding` — the mint destination; needs no signature.
    /// 5. `stability_fee_accumulator` — guarded; the fee anchor.
    /// 6. `redemption_price_state` — guarded; the redemption anchor.
    /// 7. `market_price_oracle` — guarded; the liveness gate.
    /// 8. `protocol_parameters` — guarded.
    /// 9. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires nine explicit accounts plus the declared globals and clock"
    )]
    #[plan]
    pub fn generate_debt(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(mut)] position: AccountMeta,
        #[account(mut)] stablecoin_definition: AccountMeta,
        #[account(mut)] user_stablecoin_holding: AccountMeta,
        stability_fee_accumulator: AccountMeta,
        redemption_price_state: AccountMeta,
        market_price_oracle: AccountMeta,
        protocol_parameters: AccountMeta,
        clock: AccountMeta,
        amount: u128,
        expected: PositionIdentity,
        parameters: ProtocolParameters,
        accumulator: StabilityFeeAccumulator,
        redemption: RedemptionPriceState,
        oracle_price: u128,
        oracle_timestamp: u64,
        now: u64,
    ) -> SpelResult {
        let call = stablecoin_program::generate_debt::plan_generate_debt(
            &owner,
            &position,
            &stablecoin_definition,
            &user_stablecoin_holding,
            &stability_fee_accumulator,
            &redemption_price_state,
            &market_price_oracle,
            &protocol_parameters,
            &clock,
            &parameters,
            oracle_timestamp,
            now,
            expected.position_nonce,
            amount,
            ctx.self_account_id,
        );
        let inputs = collateralization_inputs(&parameters, accumulator, redemption, now);
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &position,
                position.program_account_id,
                &Effect::PositionAfterGenerate(stablecoin_core::GenerateDebt {
                    expected,
                    amount,
                    inputs,
                }),
            )
            .guard(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::CheckAccumulator(stablecoin_core::CheckAccumulator { accumulator }),
            )
            .guard(
                &redemption_price_state,
                redemption_price_state.program_account_id,
                &Effect::CheckRedemption(stablecoin_core::CheckRedemption { state: redemption }),
            )
            .guard(
                &market_price_oracle,
                market_price_oracle.program_account_id,
                &Effect::CheckOracle(CheckOracle {
                    price: oracle_price,
                    timestamp: oracle_timestamp,
                }),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Clears a fully-settled position's shard (spec §10.9).
    ///
    /// v0.2.5 could only zero the *data*: ownership was not in a diff, the runtime
    /// derived it, and the account lingered stablecoin-owned — which is why
    /// §10.9's `Account::default()` was unexpressible and the `(owner, nonce)` pair
    /// could never be reused. v0.3.0 removes ownership from an address entirely,
    /// so clearing the shard *is* the close, and the pair becomes reusable:
    /// `open_position`'s write asks only that the shard be empty.
    ///
    /// The vault still lingers at `balance = 0` — the Token Program has no
    /// `CloseHolding` (§12) — and its emptiness is guarded on its own shard rather
    /// than read here, which catches the case where position and vault disagree.
    /// Allowed while frozen.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs.
    /// 2. `position` — the `(owner, nonce)` PDA; shard cleared.
    /// 3. `vault` — guarded at a zero balance.
    /// 4. `protocol_parameters` — guarded; supplies the binding.
    #[plan]
    pub fn close_position(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(mut)] position: AccountMeta,
        vault: AccountMeta,
        protocol_parameters: AccountMeta,
        expected: PositionIdentity,
        parameters: ProtocolParameters,
    ) -> SpelResult {
        stablecoin_program::close_position::plan_close_position(
            &owner,
            &position,
            &vault,
            &protocol_parameters,
            &parameters,
            expected.position_nonce,
            ctx.self_account_id,
        );
        let descriptor = stablecoin_program::shared::collateral_descriptor(&parameters);
        Ok(SpelOutput::empty()
            .effect(
                &position,
                position.program_account_id,
                &Effect::PositionAfterClose(stablecoin_core::ClosePosition { expected }),
            )
            .guard(
                &vault,
                vault.program_account_id,
                &Effect::CheckHoldingBalance(CheckHoldingBalance {
                    descriptor,
                    balance: 0,
                }),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            ))
    }

    // ── Admin setters ───────────────────────────────────────────────────────
    //
    // Every setter emits exactly one effect, on the `ProtocolParameters` shard it
    // is rewriting. Whether the signer is the *real* admin is stored in that shard
    // and nowhere else, so the plan reports which account claims the role and the
    // apply compares it — which also means no setter needs a `CheckParameters`
    // guard: the write's own apply is already reading the record.

    /// Retunes the minimum collateralization ratio (spec §10.11).
    ///
    /// Tightening leaves existing positions retroactively under-collateralized:
    /// they can still deposit or repay to recover, but cannot withdraw or borrow
    /// until back above the ratio.
    ///
    /// Expected accounts:
    /// 1. `admin` — signs; compared against the stored admin by the apply.
    /// 2. `protocol_parameters` — rewritten.
    #[plan]
    pub fn set_minimum_collateralization_ratio(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_ratio: u128,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_minimum_collateralization_ratio(
            &admin,
            &protocol_parameters,
            new_ratio,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::UpdateParameters(effect),
        ))
    }

    /// Retunes both PI controller gains (spec §10.12).
    ///
    /// Bundled, and deliberately does not reset the controller's integral term.
    ///
    /// Expected accounts: as `set_minimum_collateralization_ratio`.
    #[plan]
    pub fn set_controller_gains(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_proportional_gain: i128,
        new_integral_gain: i128,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_controller_gains(
            &admin,
            &protocol_parameters,
            new_proportional_gain,
            new_integral_gain,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::UpdateParameters(effect),
        ))
    }

    /// Rotates the market-price oracle (spec §10.13).
    ///
    /// The pin chain here runs through two shards and is the clearest example of
    /// the pattern: the asset pair the replacement must quote is declared, pinned
    /// against the stored definitions by the parameters apply, and pinned against
    /// the replacement's own fields by a guard on the oracle shard. Neither apply
    /// sees both accounts, and between them they establish exactly what the single
    /// v0.2.5 function established directly.
    ///
    /// The producing program stays unconstrained — any well-formed
    /// `OraclePriceAccount` works — but it is now *bound* by the admin here
    /// instead of chosen per call later.
    ///
    /// Expected accounts:
    /// 1. `admin` — signs.
    /// 2. `protocol_parameters` — rewritten.
    /// 3. `new_oracle` — guarded as quoting the declared asset pair.
    #[plan]
    pub fn set_market_price_oracle(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_oracle: AccountMeta,
        expected_base_asset: AccountId,
        expected_quote_asset: AccountId,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_market_price_oracle(
            &admin,
            &protocol_parameters,
            &new_oracle,
            expected_base_asset,
            expected_quote_asset,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty()
            .effect(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::UpdateParameters(effect),
            )
            .guard(
                &new_oracle,
                new_oracle.program_account_id,
                &Effect::CheckOraclePair(CheckOraclePair {
                    base_asset: expected_base_asset,
                    quote_asset: expected_quote_asset,
                }),
            ))
    }

    /// Retunes both timing parameters (spec §10.14). Bundled.
    ///
    /// Expected accounts: as `set_minimum_collateralization_ratio`.
    #[plan]
    pub fn set_timing_parameters(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_minimum_milliseconds_between_rate_updates: u64,
        new_maximum_oracle_price_age_milliseconds: u64,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_timing_parameters(
            &admin,
            &protocol_parameters,
            new_minimum_milliseconds_between_rate_updates,
            new_maximum_oracle_price_age_milliseconds,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::UpdateParameters(effect),
        ))
    }

    /// Rotates the admin handle (spec §10.15).
    ///
    /// One-step and effective immediately, since every admin check reads the
    /// stored handle. A wrong id locks the admin out permanently.
    ///
    /// Expected accounts: as `set_minimum_collateralization_ratio`.
    #[plan]
    pub fn set_admin(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_admin_account_id: AccountId,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_admin(
            &admin,
            &protocol_parameters,
            new_admin_account_id,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::UpdateParameters(effect),
        ))
    }

    /// Rotates the freeze-authority handle (spec §10.16).
    ///
    /// Set by the **admin**, not by the freeze authority, so a compromised freeze
    /// authority cannot entrench itself.
    ///
    /// Expected accounts: as `set_minimum_collateralization_ratio`.
    #[plan]
    pub fn set_freeze_authority(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        new_freeze_authority_account_id: AccountId,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_freeze_authority(
            &admin,
            &protocol_parameters,
            new_freeze_authority_account_id,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::UpdateParameters(effect),
        ))
    }

    /// Retunes the stability fee (spec §10.10).
    ///
    /// The only setter that touches a second shard, because the accumulator must
    /// first be accrued forward at the **old** rate so the new one is never
    /// retroactive. That accrual happens in the accumulator's own shard, which
    /// cannot see the parameters — so the old rate travels as a declared value and
    /// `ParameterChange::StabilityFee::old_rate` is what pins it against the
    /// record being overwritten.
    ///
    /// Expected accounts:
    /// 1. `admin` — signs.
    /// 2. `protocol_parameters` — rewritten; also pins `old_rate`.
    /// 3. `stability_fee_accumulator` — accrued at the old rate.
    /// 4. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires four explicit accounts plus both rates and the clock"
    )]
    #[plan]
    pub fn set_stability_fee_per_millisecond(
        ctx: ProgramContext,
        #[account(signer)] admin: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
        #[account(mut)] stability_fee_accumulator: AccountMeta,
        clock: AccountMeta,
        new_rate: u128,
        old_rate: u128,
        now: u64,
    ) -> SpelResult {
        let effect = stablecoin_program::admin::plan_set_stability_fee_per_millisecond(
            &admin,
            &protocol_parameters,
            &stability_fee_accumulator,
            &clock,
            old_rate,
            new_rate,
            ctx.self_account_id,
        );
        let accrue = stablecoin_program::accrue_stability_fee::accrue_fee_effect(old_rate, now);
        Ok(SpelOutput::empty()
            .effect(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::UpdateParameters(effect),
            )
            .effect(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::AccrueFee(accrue),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Emergency kill switch: halts the risk-increasing instructions (spec §10.17).
    ///
    /// While frozen, `open_position`, `withdraw_collateral` and `generate_debt`
    /// panic; `deposit_collateral`, `repay_debt`, `close_position` and the
    /// permissionless pokes keep working (§7). Idempotent.
    ///
    /// Expected accounts:
    /// 1. `freeze_authority` — signs; compared against the stored handle by the apply.
    /// 2. `protocol_parameters` — rewritten.
    #[plan]
    pub fn freeze(
        ctx: ProgramContext,
        #[account(signer)] freeze_authority: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
    ) -> SpelResult {
        let effect = stablecoin_program::freeze::plan_set_frozen(
            &freeze_authority,
            &protocol_parameters,
            true,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::SetFrozen(effect),
        ))
    }

    /// Resumes normal operation (spec §10.18). Idempotent; same accounts and
    /// preconditions as `freeze`.
    #[plan]
    pub fn unfreeze(
        ctx: ProgramContext,
        #[account(signer)] freeze_authority: AccountMeta,
        #[account(mut)] protocol_parameters: AccountMeta,
    ) -> SpelResult {
        let effect = stablecoin_program::freeze::plan_set_frozen(
            &freeze_authority,
            &protocol_parameters,
            false,
            ctx.self_account_id,
        );
        Ok(SpelOutput::empty().effect(
            &protocol_parameters,
            protocol_parameters.program_account_id,
            &Effect::SetFrozen(effect),
        ))
    }

    /// Withdraws collateral from a position back to a user-controlled holding
    /// (spec §10.6).
    ///
    /// Blocked while frozen — this is the one collateral move that can only worsen
    /// the protocol's position. §6.2 is re-checked after the decrement, unless
    /// `amount` or the position's debt is zero, in which case the new state is
    /// trivially safe and requiring a live redemption price would only make the
    /// instruction fail on a stale oracle.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs.
    /// 2. `position` — the `(owner, nonce)` PDA; collateral shrunk.
    /// 3. `vault` — debited by the chained call, authorized by its PDA seed.
    /// 4. `user_collateral_holding` — the destination; needs no signature.
    /// 5. `stability_fee_accumulator` — guarded; the fee anchor.
    /// 6. `redemption_price_state` — guarded; the redemption anchor.
    /// 7. `protocol_parameters` — guarded.
    /// 8. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires eight explicit accounts plus the declared globals and clock"
    )]
    #[plan]
    pub fn withdraw_collateral(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(mut)] position: AccountMeta,
        #[account(mut)] vault: AccountMeta,
        #[account(mut)] user_collateral_holding: AccountMeta,
        stability_fee_accumulator: AccountMeta,
        redemption_price_state: AccountMeta,
        protocol_parameters: AccountMeta,
        clock: AccountMeta,
        amount: u128,
        expected: PositionIdentity,
        parameters: ProtocolParameters,
        accumulator: StabilityFeeAccumulator,
        redemption: RedemptionPriceState,
        now: u64,
    ) -> SpelResult {
        let call = stablecoin_program::withdraw_collateral::plan_withdraw_collateral(
            &owner,
            &position,
            &vault,
            &user_collateral_holding,
            &stability_fee_accumulator,
            &redemption_price_state,
            &protocol_parameters,
            &clock,
            &parameters,
            expected.position_nonce,
            amount,
            ctx.self_account_id,
        );
        let inputs = collateralization_inputs(&parameters, accumulator, redemption, now);
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &position,
                position.program_account_id,
                &Effect::PositionAfterWithdraw(stablecoin_core::WithdrawCollateral {
                    expected,
                    amount,
                    inputs,
                }),
            )
            .guard(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::CheckAccumulator(stablecoin_core::CheckAccumulator { accumulator }),
            )
            .guard(
                &redemption_price_state,
                redemption_price_state.program_account_id,
                &Effect::CheckRedemption(stablecoin_core::CheckRedemption { state: redemption }),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    /// Repays outstanding stablecoin debt against a position (spec §10.8).
    ///
    /// Allowed while frozen — repaying only improves the protocol's position (§7)
    /// — and with no collateralization check, for the same reason. The
    /// normalized-debt decrement is rounded **down** (§6.3), so debt shrinks by at
    /// most what was burned and the remainder is fee credit for the protocol.
    ///
    /// The redemption anchor is deliberately not declared: nothing here reads it,
    /// and pinning it would make every repayment fail whenever the controller
    /// ticked — a failure mode bought for nothing.
    ///
    /// Expected accounts:
    /// 1. `owner` — signs.
    /// 2. `position` — the `(owner, nonce)` PDA; debt shrunk.
    /// 3. `stablecoin_definition` — the bound definition; supply burned.
    /// 4. `user_stablecoin_holding` — signs; the tokens are burned from here.
    /// 5. `stability_fee_accumulator` — guarded; the fee anchor.
    /// 6. `protocol_parameters` — guarded.
    /// 7. `clock` — the system `CLOCK_01` account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires seven explicit accounts plus the declared globals and clock"
    )]
    #[plan]
    pub fn repay_debt(
        ctx: ProgramContext,
        #[account(signer)] owner: AccountMeta,
        #[account(mut)] position: AccountMeta,
        #[account(mut)] stablecoin_definition: AccountMeta,
        #[account(mut, signer)] user_stablecoin_holding: AccountMeta,
        stability_fee_accumulator: AccountMeta,
        protocol_parameters: AccountMeta,
        clock: AccountMeta,
        amount: u128,
        expected: PositionIdentity,
        parameters: ProtocolParameters,
        accumulator: StabilityFeeAccumulator,
        now: u64,
    ) -> SpelResult {
        let call = stablecoin_program::repay_debt::plan_repay_debt(
            &owner,
            &position,
            &stablecoin_definition,
            &user_stablecoin_holding,
            &stability_fee_accumulator,
            &protocol_parameters,
            &clock,
            &parameters,
            expected.position_nonce,
            amount,
            ctx.self_account_id,
        );
        let fee = FeeProjection {
            accumulator,
            stability_fee_per_millisecond: parameters.stability_fee_per_millisecond,
            now,
        };
        Ok(SpelOutput::plan(Vec::new(), vec![call])
            .effect(
                &position,
                position.program_account_id,
                &Effect::PositionAfterRepay(stablecoin_core::RepayDebt {
                    expected,
                    amount,
                    fee,
                }),
            )
            .guard(
                &stability_fee_accumulator,
                stability_fee_accumulator.program_account_id,
                &Effect::CheckAccumulator(stablecoin_core::CheckAccumulator { accumulator }),
            )
            .guard(
                &protocol_parameters,
                protocol_parameters.program_account_id,
                &Effect::CheckParameters(CheckParameters { parameters }),
            )
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(stablecoin_core::CheckClock { now }),
            ))
    }

    // ── Writes ──────────────────────────────────────────────────────────────

    #[apply]
    pub fn create(effect: stablecoin_core::Create, pre_data: &ShardData) -> Option<ShardData> {
        Some(stablecoin_program::shared::create(pre_data, effect.data))
    }

    #[apply]
    pub fn update_parameters(
        effect: stablecoin_core::UpdateParameters,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::admin::update_parameters(
            pre_data, &effect,
        ))
    }

    #[apply]
    pub fn set_frozen(
        effect: stablecoin_core::SetFrozen,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::freeze::set_frozen(pre_data, &effect))
    }

    #[apply]
    pub fn accrue_fee(effect: AccrueFee, pre_data: &ShardData) -> Option<ShardData> {
        Some(stablecoin_program::accrue_stability_fee::accrue_fee(
            pre_data, &effect,
        ))
    }

    #[apply]
    pub fn advance_redemption(
        effect: AdvanceRedemption,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::update_redemption_rate::advance_redemption(pre_data, &effect))
    }

    /// The best-effort twin of `advance_redemption`: `None` here means "skipped",
    /// which is the whole of `refresh_globals`' soft-gate behaviour.
    #[apply]
    pub fn try_advance_redemption(
        effect: AdvanceRedemption,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::update_redemption_rate::try_advance_redemption(pre_data, &effect)
    }

    #[apply]
    pub fn position_after_deposit(
        effect: stablecoin_core::DepositCollateral,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::deposit_collateral::position_after_deposit(pre_data, &effect))
    }

    #[apply]
    pub fn position_after_generate(
        effect: stablecoin_core::GenerateDebt,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::generate_debt::position_after_generate(
            pre_data, &effect,
        ))
    }

    #[apply]
    pub fn position_after_withdraw(
        effect: stablecoin_core::WithdrawCollateral,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::withdraw_collateral::position_after_withdraw(pre_data, &effect))
    }

    #[apply]
    pub fn position_after_repay(
        effect: stablecoin_core::RepayDebt,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::repay_debt::position_after_repay(
            pre_data, &effect,
        ))
    }

    #[apply]
    pub fn position_after_close(
        effect: stablecoin_core::ClosePosition,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(stablecoin_program::close_position::position_after_close(
            pre_data, &effect,
        ))
    }

    // ── Guards ──────────────────────────────────────────────────────────────
    //
    // Each returns `None`. For the four that read another program's shard — the
    // clock, the oracle, a Token definition, a Token holding — that is enforced,
    // since the runtime rejects a foreign write. For the three on this program's
    // own globals it is not, so a guard that accidentally wrote would be a silent
    // hole.

    #[apply]
    pub fn check_clock(
        effect: stablecoin_core::CheckClock,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_clock(pre_data, effect.now);
        None
    }

    #[apply]
    pub fn check_parameters(effect: CheckParameters, pre_data: &ShardData) -> Option<ShardData> {
        stablecoin_program::shared::check_parameters(pre_data, &effect.parameters);
        None
    }

    #[apply]
    pub fn check_accumulator(
        effect: stablecoin_core::CheckAccumulator,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_accumulator(pre_data, &effect.accumulator);
        None
    }

    #[apply]
    pub fn check_redemption(
        effect: stablecoin_core::CheckRedemption,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_redemption(pre_data, &effect.state);
        None
    }

    #[apply]
    pub fn check_oracle(effect: CheckOracle, pre_data: &ShardData) -> Option<ShardData> {
        stablecoin_program::shared::check_oracle(pre_data, effect.price, effect.timestamp);
        None
    }

    #[apply]
    pub fn check_oracle_pair(effect: CheckOraclePair, pre_data: &ShardData) -> Option<ShardData> {
        stablecoin_program::shared::check_oracle_pair(pre_data, &effect);
        None
    }

    #[apply]
    pub fn check_fungible_definition(
        _effect: stablecoin_core::CheckFungibleDefinition,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_fungible_definition(pre_data);
        None
    }

    #[apply]
    pub fn check_holding_balance(
        effect: CheckHoldingBalance,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_holding_balance(pre_data, &effect);
        None
    }

    #[apply]
    pub fn check_holding_absent(
        _effect: stablecoin_core::CheckHoldingAbsent,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        stablecoin_program::shared::check_holding_absent(pre_data);
        None
    }
}

/// The §6.2 inputs `generate_debt` and `withdraw_collateral` both assemble from
/// the same three declared records.
fn collateralization_inputs(
    parameters: &ProtocolParameters,
    accumulator: StabilityFeeAccumulator,
    redemption: RedemptionPriceState,
    now: u64,
) -> CollateralizationInputs {
    CollateralizationInputs {
        fee: FeeProjection {
            accumulator,
            stability_fee_per_millisecond: parameters.stability_fee_per_millisecond,
            now,
        },
        redemption,
        minimum_collateralization_ratio: parameters.minimum_collateralization_ratio,
    }
}
