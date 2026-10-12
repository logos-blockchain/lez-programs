#![cfg_attr(not(test), no_main)]

use nssa_core::account::{AccountId, ShardData};
use spel_framework::{context::ProgramContext, prelude::*};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

// Every instruction here used to read the clock, and two of them read a second
// oracle shard as well. Under v0.3.0 a plan reads nothing and an apply sees one
// shard, so each of those reads became a declared instruction field plus a guard
// on the shard it came from:
//
//   now                       -> guard on CLOCK_01            (exact)
//   current_tick              -> guard on CurrentTickAccount   (exact)
//   current_tick_last_updated -> same guard
//   twap_price                -> guard on PriceObservations, which recomputes it
//
// `PublishPrice` is the interesting one. Its average needs the ring buffer, the
// live tick and the wall clock at once, and nothing sees all three. So the caller
// computes it, the observations guard recomputes it from the buffer it *can* see
// plus the three scalars above, and those scalars are each pinned by their own
// guard. A lie about any input fails either the recomputation or the guard
// holding that input to its shard.
#[lez_program(instruction = "twap_oracle_core::Instruction")]
mod twap_oracle {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    /// Creates and initialises a price observations account for a price source
    /// and time window.
    #[plan]
    pub fn create_price_observations(
        ctx: ProgramContext,
        #[account(init)] price_observations: AccountMeta,
        #[account(signer)] price_source: AccountMeta,
        clock: AccountMeta,
        initial_tick: i32,
        window_duration: u64,
        now: u64,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "CreatePriceObservations");
        let data = twap_oracle_program::create_price_observations::plan_create_price_observations(
            &price_observations,
            &price_source,
            initial_tick,
            window_duration,
            now,
            ctx.self_account_id,
        );

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .effect(
                &price_observations,
                price_observations.program_account_id,
                &Effect::Create(twap_oracle_core::Create { data }),
            ))
    }

    /// Creates and initialises a canonical oracle price account.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit accounts plus the asset pair, seed price, window, and declared clock"
    )]
    #[plan]
    pub fn create_oracle_price_account(
        ctx: ProgramContext,
        #[account(init)] oracle_price_account: AccountMeta,
        #[account(signer)] price_source: AccountMeta,
        clock: AccountMeta,
        base_asset: AccountId,
        quote_asset: AccountId,
        initial_price: u128,
        window_duration: u64,
        now: u64,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "CreateOraclePriceAccount");
        let data =
            twap_oracle_program::create_oracle_price_account::plan_create_oracle_price_account(
                &oracle_price_account,
                &price_source,
                base_asset,
                quote_asset,
                initial_price,
                window_duration,
                now,
                ctx.self_account_id,
            );

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .effect(
                &oracle_price_account,
                oracle_price_account.program_account_id,
                &Effect::Create(twap_oracle_core::Create { data }),
            ))
    }

    /// Creates and initialises the live tick account for a price source.
    #[plan]
    pub fn create_current_tick_account(
        ctx: ProgramContext,
        #[account(init)] current_tick_account: AccountMeta,
        #[account(signer)] price_source: AccountMeta,
        clock: AccountMeta,
        initial_price: u128,
        now: u64,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "CreateCurrentTickAccount");
        let data =
            twap_oracle_program::create_current_tick_account::plan_create_current_tick_account(
                &current_tick_account,
                &price_source,
                initial_price,
                now,
                ctx.self_account_id,
            );

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .effect(
                &current_tick_account,
                current_tick_account.program_account_id,
                &Effect::Create(twap_oracle_core::Create { data }),
            ))
    }

    /// Updates the tick stored in an existing current tick account from a new
    /// spot price.
    #[plan]
    pub fn update_current_tick(
        ctx: ProgramContext,
        #[account(mut)] current_tick_account: AccountMeta,
        #[account(signer)] price_source: AccountMeta,
        clock: AccountMeta,
        price: u128,
        now: u64,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "UpdateCurrentTick");
        let tick = twap_oracle_program::update_current_tick::plan_update_current_tick(
            &current_tick_account,
            &price_source,
            price,
            ctx.self_account_id,
        );

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .effect(
                &current_tick_account,
                current_tick_account.program_account_id,
                &Effect::WriteCurrentTick(twap_oracle_core::WriteCurrentTick {
                    tick,
                    last_updated: now,
                }),
            ))
    }

    // The order of the `#[plan]` functions here is load-bearing and must match the
    // `Instruction` enum's variant order: the IDL carries no discriminant, so
    // `spel-cli` uses an instruction's *position* in the IDL — which is this
    // source order — as the Borsh variant tag. `publish_price` therefore precedes
    // `record_tick`, as the enum has it.

    /// Computes the TWAP and writes it to the oracle price account.
    ///
    /// The price effect is emitted only when `twap_price` is `Some`. A `None`
    /// claim still carries its guard, so "there was nothing to publish" is
    /// checked as strictly as a published figure.
    #[expect(
        clippy::too_many_arguments,
        reason = "three account slots plus the pinned scalars the TWAP is recomputed from"
    )]
    #[plan]
    pub fn publish_price(
        ctx: ProgramContext,
        price_observations: AccountMeta,
        #[account(mut)] oracle_price_account: AccountMeta,
        current_tick_account: AccountMeta,
        clock: AccountMeta,
        price_source_id: AccountId,
        window_duration: u64,
        now: u64,
        current_tick: i32,
        current_tick_last_updated: u64,
        twap_price: Option<u128>,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "PublishPrice");
        twap_oracle_program::publish_price::plan_publish_price(
            &price_observations,
            &oracle_price_account,
            &current_tick_account,
            price_source_id,
            window_duration,
            ctx.self_account_id,
        );

        let mut output = SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .guard(
                &current_tick_account,
                current_tick_account.program_account_id,
                &Effect::CheckCurrentTick(twap_oracle_core::CheckCurrentTick {
                    tick: current_tick,
                    last_updated: Some(current_tick_last_updated),
                }),
            )
            .guard(
                &price_observations,
                price_observations.program_account_id,
                &Effect::CheckTwap(twap_oracle_core::CheckTwap {
                    tick: current_tick,
                    last_updated: current_tick_last_updated,
                    now,
                    twap_price,
                }),
            );

        if let Some(price) = twap_price {
            output = output.effect(
                &oracle_price_account,
                oracle_price_account.program_account_id,
                &Effect::PublishTwap(twap_oracle_core::PublishTwap { price, now }),
            );
        }

        Ok(output)
    }

    /// Records the current tick into the price observations ring buffer.
    #[expect(
        clippy::too_many_arguments,
        reason = "instruction interface requires explicit accounts plus the declared clock and current tick"
    )]
    #[plan]
    pub fn record_tick(
        ctx: ProgramContext,
        #[account(mut)] price_observations: AccountMeta,
        current_tick_account: AccountMeta,
        clock: AccountMeta,
        price_source_id: AccountId,
        window_duration: u64,
        now: u64,
        current_tick: i32,
    ) -> SpelResult {
        twap_oracle_program::shared::assert_canonical_clock(&clock, "RecordTick");
        twap_oracle_program::record_tick::plan_record_tick(
            &price_observations,
            &current_tick_account,
            price_source_id,
            window_duration,
            ctx.self_account_id,
        );

        Ok(SpelOutput::empty()
            .guard(
                &clock,
                clock.program_account_id,
                &Effect::CheckClock(twap_oracle_core::CheckClock { now }),
            )
            .guard(
                &current_tick_account,
                current_tick_account.program_account_id,
                &Effect::CheckCurrentTick(twap_oracle_core::CheckCurrentTick {
                    tick: current_tick,
                    last_updated: None,
                }),
            )
            .effect(
                &price_observations,
                price_observations.program_account_id,
                &Effect::AppendObservation(twap_oracle_core::AppendObservation {
                    tick: current_tick,
                    now,
                    window_duration,
                }),
            ))
    }

    // ── apply halves ────────────────────────────────────────────────────────

    #[apply]
    pub fn check_clock(
        effect: twap_oracle_core::CheckClock,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        twap_oracle_program::shared::check_clock(pre_data, effect.now);
        None
    }

    #[apply]
    pub fn create(effect: twap_oracle_core::Create, pre_data: &ShardData) -> Option<ShardData> {
        Some(twap_oracle_program::shared::create(pre_data, effect.data))
    }

    #[apply]
    pub fn write_current_tick(
        effect: twap_oracle_core::WriteCurrentTick,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(
            twap_oracle_program::update_current_tick::write_current_tick(
                pre_data,
                effect.tick,
                effect.last_updated,
            ),
        )
    }

    #[apply]
    pub fn check_current_tick(
        effect: twap_oracle_core::CheckCurrentTick,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        twap_oracle_program::update_current_tick::check_current_tick(
            pre_data,
            effect.tick,
            effect.last_updated,
        );
        None
    }

    #[apply]
    pub fn append_observation(
        effect: twap_oracle_core::AppendObservation,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        twap_oracle_program::record_tick::append_observation(
            pre_data,
            effect.tick,
            effect.now,
            effect.window_duration,
        )
    }

    #[apply]
    pub fn check_twap(
        effect: twap_oracle_core::CheckTwap,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        twap_oracle_program::publish_price::check_twap(
            pre_data,
            effect.tick,
            effect.last_updated,
            effect.now,
            effect.twap_price,
        );
        None
    }

    #[apply]
    pub fn publish_twap(
        effect: twap_oracle_core::PublishTwap,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        Some(twap_oracle_program::publish_price::publish_twap(
            pre_data,
            effect.price,
            effect.now,
        ))
    }
}
