//! AMM settlement and TWAP publication through the real AMM, token, Oracle, and clock programs.

use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_protocol_fee_pda, compute_vault_pda, event::Event, spot_price_q64_64,
    swap_exact_in_amounts, swap_exact_out_amounts, AmmConfig, AmmState, Message, PoolDefinition,
    MINIMUM_LIQUIDITY,
};
use clock_core::clock_actor;
use integration_tests::{
    account, assert_refused, deploy, deploy_at, key, submit, tick, with_clock,
};
use lee::{
    error::{InvalidProgramBehaviorError, LeeError},
    PrivateKey, V03State,
};
use lee_core::{
    account::{AccountId, Actor},
    execution_state::ExecutionError,
    program::TransactionEvent,
};
use token_core::{Message as TokenMessage, TokenState};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_price_observations_pda, price_to_tick,
    tick_to_oracle_price, CurrentTickState, Message as OracleMessage, OraclePrice, OracleState,
    PriceSource,
};
use twap_oracle_methods::TWAP_ORACLE_ELF;

const DEPOSIT_A: u128 = 1_000_000;
const DEPOSIT_B: u128 = 2_000_000;
const SUPPLY: u128 = 1_000_000_000;
const T0: u64 = 1_700_000_000_000;
const WINDOW: u64 = 2_048_000;

struct Chain {
    state: V03State,
    amm: AccountId,
    token: AccountId,
    oracle: AccountId,
    config: AccountId,
    pool: AccountId,
    a: AccountId,
    b: AccountId,
    user_a: AccountId,
    user_b: AccountId,
    user_lp: AccountId,
}

impl Chain {
    fn empty(protocol_fee_bps: u128) -> Self {
        let (state, ids) = deploy(&[
            amm_methods::AMM_ELF,
            token_methods::TOKEN_ELF,
            TWAP_ORACLE_ELF,
        ]);
        let [amm, token, oracle] = ids.as_slice() else {
            panic!("three guests deployed")
        };
        Self::setup(state, *amm, *token, *oracle, protocol_fee_bps)
    }

    fn setup(
        state: V03State,
        amm: AccountId,
        token: AccountId,
        oracle: AccountId,
        protocol_fee_bps: u128,
    ) -> Self {
        let config = compute_config_pda(amm, account(&key(1)), [0; 32]);
        let (a, b) = (account(&key(2)), account(&key(3)));
        let mut chain = Self {
            state: with_clock(state, T0),
            amm,
            token,
            oracle,
            config,
            pool: compute_pool_pda(amm, config, a, b),
            a,
            b,
            user_a: account(&key(4)),
            user_b: account(&key(5)),
            user_lp: account(&key(6)),
        };
        for (definition, holding, definition_key, holding_key) in [
            (a, chain.user_a, key(2), key(4)),
            (b, chain.user_b, key(3), key(5)),
        ] {
            chain
                .token_submit(
                    definition,
                    &[definition, holding],
                    &[&definition_key, &holding_key],
                    &TokenMessage::NewDefinition {
                        name: "Test".to_owned(),
                        total_supply: SUPPLY,
                        mint_authority: None,
                        holding,
                    },
                )
                .expect("token created");
        }
        chain
            .call(
                account(&key(1)),
                &[&key(1)],
                &Message::Initialize {
                    nonce: [0; 32],
                    config: AmmConfig {
                        token_program_id: token,
                        twap_oracle_program_id: oracle,
                        authority: account(&key(1)),
                        swap_fee_bps: 30,
                        protocol_fee_bps,
                    },
                },
            )
            .expect("AMM initialized");
        chain
    }

    fn new() -> Self {
        let mut chain = Self::empty(2_000);
        chain.create().expect("pool created");
        chain
    }

    fn create_message(&self) -> Message {
        Message::NewDefinition {
            config: self.config,
            definition_token_a_id: self.a,
            definition_token_b_id: self.b,
            user_holding_a: self.user_a,
            user_holding_b: self.user_b,
            user_holding_lp: self.user_lp,
            token_a_amount: DEPOSIT_A,
            token_b_amount: DEPOSIT_B,
            deadline: u64::MAX,
        }
    }

    fn create(&mut self) -> Result<Vec<TransactionEvent>, LeeError> {
        self.call(
            self.config,
            &[&key(4), &key(5), &key(6)],
            &self.create_message(),
        )
    }

    fn actors(&self) -> Vec<Actor> {
        let mut actors = vec![
            Actor::new(self.config, self.amm),
            Actor::new(self.pool, self.amm),
            Actor::new(account(&key(1)), self.amm),
            self.tick_actor(),
            clock_actor(),
            Actor::new(self.observations_id(), self.oracle),
            Actor::new(self.price_id(), self.oracle),
        ];
        actors.extend(
            [
                self.a,
                self.b,
                self.user_a,
                self.user_b,
                self.user_lp,
                compute_vault_pda(self.amm, self.pool, self.a),
                compute_vault_pda(self.amm, self.pool, self.b),
                compute_liquidity_token_pda(self.amm, self.pool),
                compute_lp_lock_holding_pda(self.amm, self.pool),
                compute_protocol_fee_pda(self.amm, self.config, self.a),
                compute_protocol_fee_pda(self.amm, self.config, self.b),
            ]
            .into_iter()
            .map(|id| Actor::new(id, self.token)),
        );
        actors
    }

    fn call(
        &mut self,
        root: AccountId,
        signers: &[&PrivateKey],
        message: &Message,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let mut actors = self.actors();
        let root = Actor::new(root, self.amm);
        if !actors.contains(&root) {
            actors.push(root);
        }
        submit(&mut self.state, root, &actors, signers, message)
    }

    fn token_submit(
        &mut self,
        root: AccountId,
        accounts: &[AccountId],
        signers: &[&PrivateKey],
        message: &TokenMessage,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let actors: Vec<_> = accounts
            .iter()
            .map(|id| Actor::new(*id, self.token))
            .collect();
        submit(
            &mut self.state,
            Actor::new(root, self.token),
            &actors,
            signers,
            message,
        )
    }

    fn pool(&self) -> PoolDefinition {
        match AmmState::decode(
            self.state
                .get_account_by_id(self.pool)
                .data
                .actor_state(self.amm),
        ) {
            Some(AmmState::Pool(pool)) => pool,
            other => panic!("expected pool, got {other:?}"),
        }
    }

    fn balance(&self, id: AccountId) -> u128 {
        match TokenState::decode(
            self.state
                .get_account_by_id(id)
                .data
                .actor_state(self.token),
        ) {
            Some(TokenState::Holding(holding)) => holding.balance,
            other => panic!("expected holding, got {other:?}"),
        }
    }

    fn lp_supply(&self) -> u128 {
        match TokenState::decode(
            self.state
                .get_account_by_id(self.pool().liquidity_pool_id)
                .data
                .actor_state(self.token),
        ) {
            Some(TokenState::Definition(definition)) => definition.total_supply,
            other => panic!("expected LP definition, got {other:?}"),
        }
    }

    fn tick_actor(&self) -> Actor {
        let source = PriceSource::program(Actor::new(self.pool, self.amm));
        Actor::new(source.current_tick_account_id(), self.oracle)
    }

    fn observations_id(&self) -> AccountId {
        compute_price_observations_pda(self.oracle, self.tick_actor().account_id, WINDOW)
    }

    fn price_id(&self) -> AccountId {
        compute_oracle_price_account_pda(self.oracle, self.tick_actor().account_id, WINDOW)
    }

    fn oracle_state(&self, id: AccountId) -> Option<OracleState> {
        OracleState::decode(
            self.state
                .get_account_by_id(id)
                .data
                .actor_state(self.oracle),
        )
    }

    fn current_tick(&self) -> CurrentTickState {
        match self.oracle_state(self.tick_actor().account_id) {
            Some(OracleState::CurrentTick(state)) => state,
            other => panic!("expected current-tick actor, got {other:?}"),
        }
    }

    fn price(&self) -> OraclePrice {
        match self.oracle_state(self.price_id()) {
            Some(OracleState::Price(state)) => state,
            other => panic!("expected price actor, got {other:?}"),
        }
    }

    fn assert_reserves(&self) {
        let pool = self.pool();
        assert_eq!(self.balance(pool.vault_a_id), pool.reserve_a);
        assert_eq!(self.balance(pool.vault_b_id), pool.reserve_b);
        assert_eq!(self.lp_supply(), pool.liquidity_pool_supply);
        assert_eq!(
            self.current_tick().tick,
            price_to_tick(spot_price_q64_64(pool.reserve_a, pool.reserve_b))
        );
        assert_eq!(
            self.balance(compute_lp_lock_holding_pda(self.amm, self.pool)),
            MINIMUM_LIQUIDITY
        );
    }

    fn swap_in(&self, a_to_b: bool, amount: u128) -> Message {
        let (source, target) = if a_to_b {
            (self.user_a, self.user_b)
        } else {
            (self.user_b, self.user_a)
        };
        Message::SwapExactInput {
            user_input_holding: source,
            user_output_holding: target,
            swap_amount_in: amount,
            min_amount_out: 1,
            deadline: u64::MAX,
        }
    }

    fn snapshot(&self) -> Vec<(Actor, Vec<u8>)> {
        self.actors()
            .into_iter()
            .map(|actor| {
                (
                    actor,
                    self.state
                        .get_account_by_id(actor.account_id)
                        .data
                        .actor_state(actor.program_account_id)
                        .to_vec(),
                )
            })
            .collect()
    }
}

#[test]
fn creation_initializes_vaults_lp_lock_and_authenticated_oracle() {
    let mut chain = Chain::empty(2_000);
    let events = chain.create().expect("create succeeds");
    let pool = chain.pool();
    assert_eq!((pool.reserve_a, pool.reserve_b), (DEPOSIT_A, DEPOSIT_B));
    assert_eq!(
        chain.balance(chain.user_lp),
        pool.liquidity_pool_supply
            .checked_sub(MINIMUM_LIQUIDITY)
            .expect("unlocked supply")
    );
    assert_eq!(
        chain.current_tick().source,
        PriceSource::program(Actor::new(chain.pool, chain.amm)),
    );
    let tick_actor = chain.tick_actor();
    assert_refused(
        &submit(
            &mut chain.state,
            tick_actor,
            &[tick_actor, clock_actor()],
            &[],
            &OracleMessage::UpdateCurrentTick { price: 1 << 64 },
        ),
        "sender is not the price source",
    );
    assert!(events.iter().any(|event| event.account_id == chain.amm
        && matches!(
            borsh::from_slice::<Event>(&event.event.data),
            Ok(Event::PoolCreated { .. })
        )));
    chain.assert_reserves();
    let before = chain.snapshot();
    assert_refused(&chain.create(), "pool already exists");
    assert_eq!(chain.snapshot(), before);
}

#[test]
fn exact_input_and_output_in_both_directions_preserve_reserves_and_protocol_fees() {
    let mut chain = Chain::new();
    for a_to_b in [true, false] {
        let pool = chain.pool();
        let (reserve_in, reserve_out, source, target, definition, signer) = if a_to_b {
            (
                pool.reserve_a,
                pool.reserve_b,
                chain.user_a,
                chain.user_b,
                chain.a,
                key(4),
            )
        } else {
            (
                pool.reserve_b,
                pool.reserve_a,
                chain.user_b,
                chain.user_a,
                chain.b,
                key(5),
            )
        };
        let amount = 10_000;
        let (effective, out) = swap_exact_in_amounts(amount, reserve_in, reserve_out, 30);
        let before_in = chain.balance(source);
        let before_out = chain.balance(target);
        chain
            .call(chain.pool, &[&signer], &chain.swap_in(a_to_b, amount))
            .expect("exact input succeeds");
        assert_eq!(
            chain.balance(source),
            before_in.checked_sub(amount).expect("debit")
        );
        assert_eq!(
            chain.balance(target),
            before_out.checked_add(out).expect("credit")
        );
        let fee = amm_core::protocol_fee_amount(amount.checked_sub(effective).expect("fee"), 2_000);
        let fee_id = compute_protocol_fee_pda(chain.amm, chain.config, definition);
        assert_eq!(chain.balance(fee_id), fee);
        chain.assert_reserves();
        let pool = chain.pool();
        let (rin, rout) = if a_to_b {
            (pool.reserve_a, pool.reserve_b)
        } else {
            (pool.reserve_b, pool.reserve_a)
        };
        let (_, required) = swap_exact_out_amounts(500, rin, rout, 30).expect("quote");
        let before = chain.balance(target);
        chain
            .call(
                chain.pool,
                &[&signer],
                &Message::SwapExactOutput {
                    user_input_holding: source,
                    user_output_holding: target,
                    exact_amount_out: 500,
                    max_amount_in: required,
                    deadline: u64::MAX,
                },
            )
            .expect("exact output succeeds");
        assert_eq!(
            chain.balance(target),
            before.checked_add(500).expect("credit")
        );
        chain.assert_reserves();
    }
}

#[test]
fn add_and_remove_liquidity_preserve_the_permanent_lock() {
    let mut chain = Chain::new();
    let initial_lp = chain.balance(chain.user_lp);
    chain
        .call(
            chain.pool,
            &[&key(4), &key(5)],
            &Message::AddLiquidity {
                user_holding_a: chain.user_a,
                user_holding_b: chain.user_b,
                user_holding_lp: chain.user_lp,
                min_amount_liquidity: 1,
                max_amount_to_add_token_a: 10_000,
                max_amount_to_add_token_b: 20_000,
                deadline: u64::MAX,
            },
        )
        .expect("add succeeds");
    assert!(chain.balance(chain.user_lp) > initial_lp);
    chain.assert_reserves();
    let liquidity = chain.balance(chain.user_lp);
    chain
        .call(
            chain.pool,
            &[&key(6)],
            &Message::RemoveLiquidity {
                user_holding_a: chain.user_a,
                user_holding_b: chain.user_b,
                user_holding_lp: chain.user_lp,
                remove_liquidity_amount: liquidity,
                min_amount_to_remove_token_a: 1,
                min_amount_to_remove_token_b: 1,
                deadline: u64::MAX,
            },
        )
        .expect("remove all unlocked liquidity");
    assert_eq!(chain.pool().liquidity_pool_supply, MINIMUM_LIQUIDITY);
    assert_eq!(chain.balance(chain.user_lp), 0);
    chain.assert_reserves();
}

#[test]
fn direct_lp_burns_reduce_the_supply_used_to_mint_new_liquidity() {
    let mut chain = Chain::new();
    let pool = chain.pool();
    let burned = pool.liquidity_pool_supply / 2;
    chain
        .token_submit(
            chain.user_lp,
            &[chain.user_lp, pool.liquidity_pool_id],
            &[&key(6)],
            &TokenMessage::Burn { amount: burned },
        )
        .expect("LP holder can burn directly through token");
    let actual_supply = chain.lp_supply();
    assert_eq!(
        actual_supply,
        pool.liquidity_pool_supply
            .checked_sub(burned)
            .expect("burned supply")
    );
    let quote = amm_ffi::api::add_liquidity_quote(
        serde_json::from_value(serde_json::json!({
            "tokenAId": hex::encode(pool.definition_token_a_id.value()),
            "tokenBId": hex::encode(pool.definition_token_b_id.value()),
            "maxAmountA": pool.reserve_a.to_string(),
            "maxAmountB": pool.reserve_b.to_string(),
            "slippageBps": 0,
            "poolData": hex::encode(AmmState::Pool(pool.clone()).encode()),
            "lpSupply": actual_supply.to_string(),
        }))
        .expect("quote request"),
    )
    .expect("add quote");
    assert_eq!(quote["minimumLp"], actual_supply.to_string());
    let before_lp = chain.balance(chain.user_lp);
    // Doubling reserves should mint exactly the current outstanding supply,
    // preserving the donation from the direct burn for existing LP holders.
    chain
        .call(
            chain.pool,
            &[&key(4), &key(5)],
            &Message::AddLiquidity {
                user_holding_a: chain.user_a,
                user_holding_b: chain.user_b,
                user_holding_lp: chain.user_lp,
                min_amount_liquidity: actual_supply,
                max_amount_to_add_token_a: pool.reserve_a,
                max_amount_to_add_token_b: pool.reserve_b,
                deadline: u64::MAX,
            },
        )
        .expect("add uses the post-burn supply");
    assert_eq!(
        chain
            .balance(chain.user_lp)
            .checked_sub(before_lp)
            .expect("minted LP"),
        actual_supply
    );
    chain.assert_reserves();
}

#[test]
fn direct_lp_burns_donate_to_remaining_liquidity_holders() {
    let mut chain = Chain::new();
    let pool = chain.pool();
    chain
        .token_submit(
            chain.user_lp,
            &[chain.user_lp, pool.liquidity_pool_id],
            &[&key(6)],
            &TokenMessage::Burn {
                amount: pool.liquidity_pool_supply / 2,
            },
        )
        .expect("direct LP burn succeeds");
    let liquidity = chain.balance(chain.user_lp);
    let supply = chain.lp_supply();
    let expected_a = amm_core::mul_div_floor(pool.reserve_a, liquidity, supply);
    let expected_b = amm_core::mul_div_floor(pool.reserve_b, liquidity, supply);
    let quote = amm_ffi::api::remove_liquidity_quote(
        serde_json::from_value(serde_json::json!({
            "tokenAId": hex::encode(pool.definition_token_a_id.value()),
            "tokenBId": hex::encode(pool.definition_token_b_id.value()),
            "lpAmount": liquidity.to_string(),
            "slippageBps": 0,
            "poolData": hex::encode(AmmState::Pool(pool.clone()).encode()),
            "lpSupply": supply.to_string(),
        }))
        .expect("quote request"),
    )
    .expect("remove quote");
    assert_eq!(quote["minimumAmountA"], expected_a.to_string());
    assert_eq!(quote["minimumAmountB"], expected_b.to_string());
    let (before_a, before_b) = (chain.balance(chain.user_a), chain.balance(chain.user_b));
    chain
        .call(
            chain.pool,
            &[&key(6)],
            &Message::RemoveLiquidity {
                user_holding_a: chain.user_a,
                user_holding_b: chain.user_b,
                user_holding_lp: chain.user_lp,
                remove_liquidity_amount: liquidity,
                min_amount_to_remove_token_a: expected_a,
                min_amount_to_remove_token_b: expected_b,
                deadline: u64::MAX,
            },
        )
        .expect("remaining holder withdraws the donated share");
    assert_eq!(
        chain.balance(chain.user_a),
        before_a.checked_add(expected_a).expect("A credit")
    );
    assert_eq!(
        chain.balance(chain.user_b),
        before_b.checked_add(expected_b).expect("B credit")
    );
    assert_eq!(chain.balance(chain.user_lp), 0);
    assert_eq!(chain.pool().liquidity_pool_supply, MINIMUM_LIQUIDITY);
    chain.assert_reserves();
}

#[test]
fn swaps_refuse_missing_signatures_slippage_wrong_tokens_and_undeclared_vaults_atomically() {
    let mut chain = Chain::new();
    let before = chain.snapshot();
    assert_refused(
        &chain.call(chain.pool, &[], &chain.swap_in(true, 10_000)),
        "sender holding must authorize the transfer",
    );
    let mut excessive_min = chain.swap_in(true, 10_000);
    if let Message::SwapExactInput { min_amount_out, .. } = &mut excessive_min {
        *min_amount_out = u128::MAX;
    }
    assert_refused(
        &chain.call(chain.pool, &[&key(4)], &excessive_min),
        "output below minimum",
    );
    let mut wrong_target = chain.swap_in(true, 10_000);
    if let Message::SwapExactInput {
        user_output_holding,
        ..
    } = &mut wrong_target
    {
        *user_output_holding = chain.user_a;
    }
    assert_refused(
        &chain.call(chain.pool, &[&key(4)], &wrong_target),
        "holding has the wrong token definition",
    );
    let mut actors = chain.actors();
    actors.retain(|a| *a != Actor::new(chain.pool().vault_a_id, chain.token));
    let message = chain.swap_in(true, 10_000);
    let result = submit(
        &mut chain.state,
        Actor::new(chain.pool, chain.amm),
        &actors,
        &[&key(4)],
        &message,
    );
    assert!(
        result.is_err(),
        "vault must be declared to guarantee atomic credit"
    );
    assert_eq!(chain.snapshot(), before);
}

#[test]
fn failed_creation_rolls_back_earlier_token_initialization_and_namespace_is_signed() {
    let mut chain = Chain::empty(0);
    let before = chain.snapshot();
    // A and LP sign, B does not. The guest reaches B only after creating the LP and vaults.
    assert_refused(
        &chain.call(chain.config, &[&key(4), &key(6)], &chain.create_message()),
        "sender holding must authorize the transfer",
    );
    assert_eq!(chain.snapshot(), before);
    let result = chain.call(
        account(&key(1)),
        &[],
        &Message::Initialize {
            nonce: [1; 32],
            config: AmmConfig {
                token_program_id: chain.token,
                twap_oracle_program_id: chain.oracle,
                authority: account(&key(1)),
                swap_fee_bps: 30,
                protocol_fee_bps: 0,
            },
        },
    );
    assert_refused(&result, "AMM owner or authority must authorize");
    chain
        .create()
        .expect("failed creation left no pending state");
    chain.assert_reserves();
}

#[test]
fn donations_are_synchronized_and_oracle_feeds_use_pool_prices() {
    let mut chain = Chain::new();
    let vault = chain.pool().vault_a_id;
    chain
        .token_submit(
            chain.user_a,
            &[chain.user_a, vault],
            &[&key(4)],
            &TokenMessage::Transfer {
                to: vault,
                amount: 123,
            },
        )
        .expect("donation");
    assert_eq!(chain.pool().reserve_a, DEPOSIT_A);
    chain
        .call(chain.pool, &[], &Message::SyncReserves)
        .expect("sync");
    assert_eq!(
        chain.pool().reserve_a,
        DEPOSIT_A.checked_add(123).expect("donation")
    );
    chain.assert_reserves();
    chain
        .call(
            chain.pool,
            &[],
            &Message::CreatePriceObservations {
                window_duration: WINDOW,
            },
        )
        .expect("open observations and price actors");
    let tick_actor = chain.tick_actor();
    let initial_tick = chain.current_tick().tick;
    let Some(OracleState::PriceObservations(observations)) =
        chain.oracle_state(chain.observations_id())
    else {
        panic!("expected observations actor")
    };
    assert_eq!(observations.price_source_id, chain.pool);
    assert_eq!(observations.current_tick, tick_actor.account_id);
    assert_eq!(observations.last_recorded_tick, initial_tick);
    assert_eq!(observations.total_entries, 1);
    let initial_price = chain.price();
    assert_eq!(
        (initial_price.base_asset, initial_price.quote_asset),
        (chain.a, chain.b)
    );
    assert_eq!(initial_price.source_id, chain.pool);
    assert_eq!(initial_price.price, tick_to_oracle_price(initial_tick));
    assert_eq!(initial_price.timestamp, T0);

    let before = chain.snapshot();
    assert_refused(
        &chain.call(
            chain.pool,
            &[],
            &Message::CreateOraclePriceAccount {
                window_duration: WINDOW,
            },
        ),
        "observations actor already exists",
    );
    assert_eq!(chain.snapshot(), before);

    // A keeper records a changed pool tick, then publishes the price on the same block.
    tick(&mut chain.state, T0 + 1_000);
    chain
        .call(chain.pool, &[&key(4)], &chain.swap_in(true, 10_000))
        .expect("swap updates the tick");
    chain.assert_reserves();
    let updated = chain.current_tick();
    assert_eq!(updated.last_updated, T0 + 1_000);
    assert_ne!(initial_tick, updated.tick);
    let declared = chain.actors();
    for operation in [
        OracleMessage::RecordTick {
            window_duration: WINDOW,
        },
        OracleMessage::PublishPrice {
            window_duration: WINDOW,
        },
    ] {
        submit(&mut chain.state, tick_actor, &declared, &[], &operation).expect("keeper succeeds");
    }
    let published = chain.price();
    assert_eq!(published.timestamp, T0 + 1_000);
    assert_eq!(published.price, tick_to_oracle_price(updated.tick));
}

#[test]
fn protocol_fee_withdrawal_follows_rotated_authority() {
    let mut chain = Chain::new();
    chain
        .call(chain.pool, &[&key(4)], &chain.swap_in(true, 10_000))
        .expect("swap");
    let fees = compute_protocol_fee_pda(chain.amm, chain.config, chain.a);
    let amount = chain.balance(fees);
    let withdraw = Message::WithdrawProtocolFees {
        config: chain.config,
        token: chain.a,
        destination: chain.user_a,
        amount,
    };
    assert_refused(
        &chain.call(account(&key(7)), &[&key(7)], &withdraw),
        "sender is not the AMM admin authority",
    );
    chain
        .call(
            account(&key(1)),
            &[&key(1)],
            &Message::UpdateConfig {
                config: chain.config,
                new_authority: account(&key(7)),
            },
        )
        .expect("rotate");
    assert_refused(
        &chain.call(account(&key(1)), &[&key(1)], &withdraw),
        "sender is not the AMM admin authority",
    );
    let before = chain.balance(chain.user_a);
    chain
        .call(account(&key(7)), &[&key(7)], &withdraw)
        .expect("new authority withdraws");
    assert_eq!(chain.balance(fees), 0);
    assert_eq!(
        chain.balance(chain.user_a),
        before.checked_add(amount).expect("credit")
    );
    chain.assert_reserves();
}

#[test]
fn non_image_program_addresses_are_used_for_pdas_and_dispatch() {
    let (amm, token, oracle) = (
        AccountId::new([81; 32]),
        AccountId::new([82; 32]),
        AccountId::new([83; 32]),
    );
    let state = deploy_at(&[
        (amm, amm_methods::AMM_ELF),
        (token, token_methods::TOKEN_ELF),
        (oracle, TWAP_ORACLE_ELF),
    ]);
    let mut chain = Chain::setup(state, amm, token, oracle, 0);
    chain.create().expect("create at named deployments");
    chain
        .call(chain.pool, &[&key(5)], &chain.swap_in(false, 10_000))
        .expect("swap at named deployments");
    chain.assert_reserves();
}

#[test]
fn oracle_failure_rolls_back_transfers_fees_and_pool_state() {
    let mut chain = Chain::new();
    let before = chain.snapshot();
    let mut actors = chain.actors();
    actors.retain(|actor| *actor != clock_actor());
    let swap = chain.swap_in(true, 10_000);
    let result = submit(
        &mut chain.state,
        Actor::new(chain.pool, chain.amm),
        &actors,
        &[&key(4)],
        &swap,
    );
    // Settlement calls UpdateCurrentTick after the token transfers. Its clock read fails.
    assert!(
        matches!(result, Err(LeeError::InvalidProgramBehavior(InvalidProgramBehaviorError::Execution(
            ExecutionError::UndeclaredActor { actor }
        ))) if actor == clock_actor()),
        "expected the Oracle's clock read to fail, got {result:?}"
    );
    assert_eq!(chain.snapshot(), before);
}

#[test]
fn permanent_lock_cannot_be_spent_by_removal_or_direct_token_messages() {
    let mut chain = Chain::new();
    let lock = compute_lp_lock_holding_pda(chain.amm, chain.pool);
    let before = chain.snapshot();
    let remove = Message::RemoveLiquidity {
        user_holding_a: chain.user_a,
        user_holding_b: chain.user_b,
        user_holding_lp: lock,
        remove_liquidity_amount: MINIMUM_LIQUIDITY,
        min_amount_to_remove_token_a: 1,
        min_amount_to_remove_token_b: 1,
        deadline: u64::MAX,
    };
    assert_refused(
        &chain.call(chain.pool, &[&key(1)], &remove),
        "holding must authorize the burn",
    );
    assert_refused(
        &chain.token_submit(
            lock,
            &[lock, chain.user_lp],
            &[&key(1)],
            &TokenMessage::Transfer {
                to: chain.user_lp,
                amount: 1,
            },
        ),
        "sender holding must authorize the transfer",
    );
    assert_eq!(chain.snapshot(), before);
}

#[test]
fn expired_swaps_roll_back_and_the_deadline_itself_is_valid() {
    use lee::{
        public_transaction::{Message as TransactionMessage, WitnessSet},
        PublicTransaction,
    };
    let mut chain = Chain::new();
    let mut message = chain.swap_in(true, 10_000);
    if let Message::SwapExactInput { deadline, .. } = &mut message {
        *deadline = 50;
    }
    let signer = key(4);
    let before = chain.snapshot();
    for timestamp in [51, 50] {
        let tx = TransactionMessage::try_new(
            Actor::new(chain.pool, chain.amm),
            chain.actors(),
            [(
                account(&signer),
                chain.state.get_account_by_id(account(&signer)).nonce,
            )]
            .into(),
            &message,
        )
        .expect("transaction encodes");
        let witness = WitnessSet::for_message(&tx, &[&signer]);
        let result = chain.state.transition_from_public_transaction(
            &PublicTransaction::new(tx, witness),
            1,
            timestamp,
        );
        if timestamp == 51 {
            assert!(result.is_err(), "expired transaction must fail");
            assert_eq!(chain.snapshot(), before);
        } else {
            result.expect("deadline is inclusive");
        }
    }
    chain.assert_reserves();
}

#[test]
fn namespaces_keep_distinct_pools_and_reject_cross_config_creation() {
    let mut chain = Chain::new();
    let first_pool = chain.pool;
    let first_config = chain.config;
    let first_pool_state = chain.pool();
    let nonce = [1; 32];
    chain.config = compute_config_pda(chain.amm, account(&key(1)), nonce);
    chain.pool = compute_pool_pda(chain.amm, chain.config, chain.a, chain.b);
    chain.user_lp = account(&key(8));
    chain
        .call(
            account(&key(1)),
            &[&key(1)],
            &Message::Initialize {
                nonce,
                config: AmmConfig {
                    token_program_id: chain.token,
                    twap_oracle_program_id: chain.oracle,
                    authority: account(&key(1)),
                    swap_fee_bps: 5,
                    protocol_fee_bps: 0,
                },
            },
        )
        .expect("second namespace");
    let mut wrong_config = chain.create_message();
    if let Message::NewDefinition { config, .. } = &mut wrong_config {
        *config = first_config;
    }
    assert_refused(
        &chain.call(chain.config, &[&key(4), &key(5), &key(8)], &wrong_config),
        "pool config does not match receiver",
    );
    chain
        .call(
            chain.config,
            &[&key(4), &key(5), &key(8)],
            &chain.create_message(),
        )
        .expect("second pool");
    assert_ne!(chain.pool, first_pool);
    assert_ne!(chain.pool().vault_a_id, first_pool_state.vault_a_id);
    chain
        .call(chain.pool, &[&key(4)], &chain.swap_in(true, 10_000))
        .expect("second pool swap");
    assert_eq!(
        AmmState::decode(
            chain
                .state
                .get_account_by_id(first_pool)
                .data
                .actor_state(chain.amm)
        ),
        Some(AmmState::Pool(first_pool_state))
    );
    chain.assert_reserves();
}

#[test]
fn creation_checks_token_id_arguments_and_the_initial_lp_signature() {
    let mut chain = Chain::empty(0);
    let before = chain.snapshot();
    let result = chain.call(chain.config, &[&key(4), &key(5)], &chain.create_message());
    assert!(
        result.is_err(),
        "the initial LP holding did not authorize: {result:?}"
    );
    let mut wrong_pair = chain.create_message();
    // Swap the supplied definitions while keeping the holdings in their original order.
    // The sorted pair derives the same pool, so only authenticated holding reads catch it.
    if let Message::NewDefinition {
        definition_token_a_id,
        definition_token_b_id,
        ..
    } = &mut wrong_pair
    {
        std::mem::swap(definition_token_a_id, definition_token_b_id);
    }
    assert_refused(
        &chain.call(chain.config, &[&key(4), &key(5), &key(6)], &wrong_pair),
        "token A definition mismatch",
    );
    assert_eq!(chain.snapshot(), before);
}

#[test]
fn module_plans_execute_with_only_their_declared_actors_and_signers() {
    use amm_ffi::api;
    use serde_json::{json, Value};
    let mut chain = Chain::empty(2_000);
    let hex_id = |id: AccountId| hex::encode(id.value());
    let config = chain.state.get_account_by_id(chain.config);
    let config_read = json!({
        "id": hex_id(chain.config), "programId": hex_id(chain.amm), "status": "ok",
        "account": {
            "nonce": hex::encode(config.nonce.0.to_le_bytes()),
            "actor_states": [{"program": hex_id(chain.amm), "data": hex::encode(config.data.actor_state(chain.amm))}]
        }
    });
    let mut request = json!({
        "ammProgramId": hex_id(chain.amm), "config": config_read,
        "tokenAId": hex_id(chain.a), "tokenBId": hex_id(chain.b),
        "userHoldingAId": hex_id(chain.user_a), "userHoldingBId": hex_id(chain.user_b),
        "userHoldingLpId": hex_id(chain.user_lp),
        "amountA": DEPOSIT_A.to_string(), "amountB": DEPOSIT_B.to_string(),
        "deadlineMs": u64::MAX.to_string(),
    });
    let execute = |chain: &mut Chain, plan: Value| {
        let ids: Vec<String> = serde_json::from_value(plan["accountIds"].clone()).expect("ids");
        let programs: Vec<String> =
            serde_json::from_value(plan["actorProgramIds"].clone()).expect("programs");
        let flags: Vec<bool> =
            serde_json::from_value(plan["signingRequirements"].clone()).expect("signers");
        let parse =
            |id: &str| AccountId::new(hex::decode(id).expect("hex").try_into().expect("32 bytes"));
        let actors: Vec<_> = ids
            .iter()
            .zip(&programs)
            .map(|(id, p)| Actor::new(parse(id), parse(p)))
            .collect();
        let keys: Vec<_> = (1..=6).map(key).collect();
        let signers: Vec<_> = keys
            .iter()
            .filter(|k| {
                ids.iter()
                    .zip(&flags)
                    .any(|(id, sign)| *sign && parse(id) == account(k))
            })
            .collect();
        let bytes: Vec<u8> =
            serde_json::from_value(plan["message"].clone()).expect("message bytes");
        let message: Message = borsh::from_slice(&bytes).expect("actor message");
        let root = usize::try_from(plan["rootMention"].as_u64().expect("root")).expect("index");
        submit(
            &mut chain.state,
            *actors.get(root).expect("root actor"),
            &actors,
            &signers,
            &message,
        )
        .expect("module plan settles");
    };
    execute(
        &mut chain,
        api::create_pool_plan(serde_json::from_value(request.clone()).expect("create request"))
            .expect("create plan"),
    );
    let pool = chain.pool();
    request["poolData"] = json!(hex::encode(AmmState::Pool(pool).encode()));
    request["maxAmountA"] = json!("10000");
    request["maxAmountB"] = json!("20000");
    request["minLp"] = json!("1");
    execute(
        &mut chain,
        api::add_liquidity_plan(serde_json::from_value(request.clone()).expect("add request"))
            .expect("add plan"),
    );
    request["tokenInId"] = json!(hex_id(chain.a));
    request["tokenOutId"] = json!(hex_id(chain.b));
    request["userInputHoldingId"] = json!(hex_id(chain.user_a));
    request["userOutputHoldingId"] = json!(hex_id(chain.user_b));
    request["amountIn"] = json!("10000");
    request["minOut"] = json!("1");
    execute(
        &mut chain,
        api::swap_exact_in_plan(serde_json::from_value(request.clone()).expect("swap request"))
            .expect("swap plan"),
    );
    request["amountOut"] = json!("100");
    request["maxIn"] = json!("10000");
    execute(
        &mut chain,
        api::swap_exact_out_plan(serde_json::from_value(request.clone()).expect("swap request"))
            .expect("swap plan"),
    );
    request["lpAmount"] = json!("1000");
    request["minAmountA"] = json!("1");
    request["minAmountB"] = json!("1");
    execute(
        &mut chain,
        api::remove_liquidity_plan(
            serde_json::from_value(request.clone()).expect("remove request"),
        )
        .expect("remove plan"),
    );
    execute(
        &mut chain,
        api::sync_reserves_plan(serde_json::from_value(request.clone()).expect("sync request"))
            .expect("sync plan"),
    );
    request["windowDurationMs"] = json!(3600000);
    execute(
        &mut chain,
        api::create_price_observations_plan(
            serde_json::from_value(request.clone()).expect("oracle request"),
        )
        .expect("oracle plan"),
    );
    execute(
        &mut chain,
        api::create_oracle_price_account_plan(
            serde_json::from_value(request.clone()).expect("oracle request"),
        )
        .expect("oracle plan"),
    );
    request["tokenDefinitionId"] = json!(hex_id(chain.a));
    request["destinationId"] = json!(hex_id(chain.user_a));
    request["amount"] = json!("1");
    execute(
        &mut chain,
        api::withdraw_protocol_fees_plan(
            serde_json::from_value(request.clone()).expect("fee request"),
        )
        .expect("fee plan"),
    );
    request["newAuthorityId"] = json!(hex_id(account(&key(6))));
    execute(
        &mut chain,
        api::transfer_ownership_plan(serde_json::from_value(request).expect("admin request"))
            .expect("admin plan"),
    );
}
