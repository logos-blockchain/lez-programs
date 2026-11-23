//! The stablecoin through its real guest, against the real token program and LEZ's clock.
//!
//! The stablecoin is deployed at the account of a key the tests hold, because `Initialize` is a
//! root message at the program's own account, signed by it.

use clock_core::clock_actor;
use integration_tests::{account, assert_refused, deploy_at, key, submit, tick, with_clock};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use stablecoin_core::{
    compute_engine_pda, compute_stablecoin_definition_pda, compute_surplus_holding_pda, Engine,
    InitializeArgs, Message, Position, StablecoinState, FIXED_POINT_ONE,
};
use stablecoin_methods::STABLECOIN_ELF;
use token_core::{TokenDefinition, TokenHolding, TokenState};
use token_methods::TOKEN_ELF;
use twap_oracle_methods::TWAP_ORACLE_ELF;

const TOKEN: AccountId = AccountId::new([0xA0; 32]);
const ORACLE: AccountId = AccountId::new([0xA1; 32]);
/// The oracle price account the engine is pointed at. It does not exist: nothing here reads it.
const ORACLE_PRICE: AccountId = AccountId::new([0xC0; 32]);

const T0: u64 = 1_000_000;
const COLLATERAL_SUPPLY: u128 = 1_000_000_000;

struct World {
    state: V03State,
    stablecoin: AccountId,
    token: AccountId,
    oracle: AccountId,
    program_key: PrivateKey,
    admin: PrivateKey,
    freeze_authority: PrivateKey,
    collateral_definition: AccountId,
}

/// The key whose account holds the whole collateral supply.
fn collateral_holder() -> PrivateKey {
    key(5)
}

/// The token, the TWAP oracle and the stablecoin deployed, the clock at `T0`, and a collateral
/// token whose supply is held by [`collateral_holder`]. Not initialized.
fn world() -> World {
    let program_key = key(1);
    let stablecoin = account(&program_key);
    let state = deploy_at(&[
        (stablecoin, STABLECOIN_ELF),
        (TOKEN, TOKEN_ELF),
        (ORACLE, TWAP_ORACLE_ELF),
    ]);
    let mut state = with_clock(state, T0);

    let collateral_definition_key = key(4);
    let collateral_definition = account(&collateral_definition_key);
    let holder = collateral_holder();
    submit(
        &mut state,
        Actor::new(collateral_definition, TOKEN),
        &[
            Actor::new(collateral_definition, TOKEN),
            Actor::new(account(&holder), TOKEN),
        ],
        &[&collateral_definition_key, &holder],
        &token_core::Message::NewDefinition {
            name: "Collateral".to_owned(),
            total_supply: COLLATERAL_SUPPLY,
            mint_authority: None,
            holding: account(&holder),
        },
    )
    .expect("the collateral token is created");

    World {
        state,
        stablecoin,
        token: TOKEN,
        oracle: ORACLE,
        program_key,
        admin: key(2),
        freeze_authority: key(3),
        collateral_definition,
    }
}

fn stablecoin(world: &World, account_id: AccountId) -> Actor {
    Actor::new(account_id, world.stablecoin)
}

fn token(world: &World, account_id: AccountId) -> Actor {
    Actor::new(account_id, world.token)
}

fn engine_id(world: &World) -> AccountId {
    compute_engine_pda(world.stablecoin)
}

fn definition_id(world: &World) -> AccountId {
    compute_stablecoin_definition_pda(world.stablecoin)
}

fn surplus_id(world: &World) -> AccountId {
    compute_surplus_holding_pda(world.stablecoin)
}

fn initialize_args(world: &World) -> InitializeArgs {
    InitializeArgs {
        initial_admin_account_id: account(&world.admin),
        freeze_authority_account_id: account(&world.freeze_authority),
        token_program_id: world.token,
        collateral_definition_id: world.collateral_definition,
        market_price_oracle_id: ORACLE_PRICE,
        market_price_oracle_program_id: world.oracle,
        initial_stability_fee_per_millisecond: FIXED_POINT_ONE + 1,
        initial_controller_proportional_gain: 1,
        initial_controller_integral_gain: 2,
        initial_minimum_collateralization_ratio: FIXED_POINT_ONE * 3 / 2,
        minimum_milliseconds_between_rate_updates: 1_000,
        maximum_oracle_price_age_milliseconds: 10_000,
        initial_redemption_price: FIXED_POINT_ONE,
        stablecoin_name: "Stable".to_owned(),
        minimum_debt_amount: 10,
        debt_ceiling: 1_000_000_000,
    }
}

/// Every actor an `Initialize` reaches, past the root.
fn initialize_actors(world: &World) -> Vec<Actor> {
    vec![
        stablecoin(world, surplus_id(world)),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
        stablecoin(world, engine_id(world)),
        clock_actor(),
    ]
}

/// `Initialize` at the program account, signed by it.
fn try_initialize(world: &mut World) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, world.stablecoin);
    let mut declared = vec![root];
    declared.extend(initialize_actors(world));
    let message = Message::Initialize(initialize_args(world));
    submit(
        &mut world.state,
        root,
        &declared,
        &[&world.program_key],
        &message,
    )
}

fn initialize(world: &mut World) {
    try_initialize(world).expect("initialize succeeds");
}

/// The permissionless fee poke at the engine. It needs no signature.
fn accrue(world: &mut World) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, engine_id(world));
    let declared = [
        root,
        clock_actor(),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
    ];
    submit(
        &mut world.state,
        root,
        &declared,
        &[],
        &Message::AccrueStabilityFee,
    )
}

fn stablecoin_state(world: &World, account_id: AccountId) -> Option<StablecoinState> {
    StablecoinState::decode(
        world
            .state
            .get_account_by_id(account_id)
            .data
            .actor_state(world.stablecoin),
    )
}

fn token_state(world: &World, account_id: AccountId) -> Option<TokenState> {
    TokenState::decode(
        world
            .state
            .get_account_by_id(account_id)
            .data
            .actor_state(world.token),
    )
}

fn engine(world: &World) -> Engine {
    match stablecoin_state(world, engine_id(world)) {
        Some(StablecoinState::Engine(engine)) => engine,
        other => panic!("expected the engine, got {other:?}"),
    }
}

fn position(world: &World, owner: AccountId) -> Option<Position> {
    match stablecoin_state(world, owner) {
        Some(StablecoinState::Position(position)) => Some(position),
        None => None,
        other => panic!("expected a position, got {other:?}"),
    }
}

fn holding(world: &World, account_id: AccountId) -> TokenHolding {
    match token_state(world, account_id) {
        Some(TokenState::Holding(holding)) => holding,
        other => panic!("expected a token holding, got {other:?}"),
    }
}

fn definition(world: &World, definition_id: AccountId) -> TokenDefinition {
    match token_state(world, definition_id) {
        Some(TokenState::Definition(definition)) => definition,
        other => panic!("expected a token definition, got {other:?}"),
    }
}

fn holding_balance(world: &World, account_id: AccountId) -> u128 {
    holding(world, account_id).balance
}

fn total_supply(world: &World, definition_id: AccountId) -> u128 {
    definition(world, definition_id).total_supply
}

// ── Initialize ───────────────────────────────────────────────────────────────────

#[test]
fn initialize_creates_the_engine_and_the_stablecoin_token() {
    let mut world = world();
    initialize(&mut world);
    let engine = engine(&world);
    assert_eq!(engine.pending, None);
    assert_eq!(engine.fee.last_accrued_at, 1_000_000);
    assert_eq!(engine.redemption.last_updated_at, 1_000_000);
    assert_eq!(engine.parameters.token_program_id, world.token);
    assert_eq!(engine.fee.accumulated_rate_at_last_accrual, FIXED_POINT_ONE);
    assert_eq!(engine.fee.total_normalized_debt, 0);
    let parameters = &engine.parameters;
    assert_eq!(parameters.admin_account_id, account(&world.admin));
    assert_eq!(
        parameters.freeze_authority_account_id,
        account(&world.freeze_authority)
    );
    assert_eq!(parameters.stablecoin_definition_id, definition_id(&world));
    assert_eq!(
        parameters.collateral_definition_id,
        world.collateral_definition
    );
    assert_eq!(parameters.market_price_oracle_id, ORACLE_PRICE);
    assert_eq!(parameters.market_price_oracle_program_id, world.oracle);
    assert_eq!(parameters.minimum_debt_amount, 10);
    assert_eq!(parameters.debt_ceiling, 1_000_000_000);
    assert!(!parameters.is_frozen);
    assert_eq!(
        total_supply(&world, compute_stablecoin_definition_pda(world.stablecoin)),
        0
    );
    assert_eq!(
        holding_balance(&world, compute_surplus_holding_pda(world.stablecoin)),
        0
    );
    // The stablecoin definition is its own mint authority, so the engine can mint through it.
    assert_eq!(
        definition(&world, definition_id(&world)),
        TokenDefinition {
            name: "Stable".to_owned(),
            total_supply: 0,
            authority: Some(definition_id(&world)),
        }
    );
    assert_eq!(
        holding(&world, surplus_id(&world)).definition_id,
        definition_id(&world)
    );
    assert_eq!(position(&world, account(&world.admin)), None);
}

#[test]
fn initialize_signed_by_anyone_else_is_refused() {
    let mut world = world();
    let other = key(9);
    let root = stablecoin(&world, account(&other));
    let mut declared = vec![root];
    declared.extend(initialize_actors(&world));
    let message = Message::Initialize(initialize_args(&world));

    let result = submit(&mut world.state, root, &declared, &[&other], &message);

    assert_refused(&result, "only the program's own account may initialize it");
    assert_eq!(stablecoin_state(&world, engine_id(&world)), None);
}

#[test]
fn initialize_twice_is_refused() {
    let mut world = world();
    initialize(&mut world);
    let before = engine(&world);
    let definition_before = definition(&world, definition_id(&world));

    let result = try_initialize(&mut world);

    // The program account's first message is the token relay, so the token's `NewDefinition`
    // at the existing definition fails before the engine sees `InitializeEngine`.
    assert_refused(&result, "definition account must be empty");
    assert_eq!(engine(&world), before);
    assert_eq!(definition(&world, definition_id(&world)), definition_before);
}

// ── AccrueStabilityFee ───────────────────────────────────────────────────────────

#[test]
fn anyone_can_accrue_and_the_engine_follows_the_clock() {
    let mut world = world();
    initialize(&mut world);
    tick(&mut world.state, 1_060_000);
    accrue(&mut world).unwrap();
    let engine = engine(&world);
    assert_eq!(engine.pending, None);
    assert_eq!(engine.fee.last_accrued_at, 1_060_000);
    // The fee is above 1.0 per millisecond, so 60_000 ms of the clock's time compound it.
    assert!(engine.fee.accumulated_rate_at_last_accrual > FIXED_POINT_ONE);
}
