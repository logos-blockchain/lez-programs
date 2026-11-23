//! The stablecoin's world, shared by the end-to-end tests (`tests/stablecoin.rs`) and the cycle
//! measurements (`tests/stablecoin_cycles.rs`): the stablecoin, the token program, the TWAP
//! oracle and a price source deployed on a `V03State` with LEZ's clock, and one helper per
//! transaction, declaring the actors it reaches and signed by the accounts it needs.
//!
//! The stablecoin is deployed at the account of a key the tests hold, because `Initialize` is a
//! root message at the program's own account, signed by it.
//!
//! Every stablecoin transaction goes through [`send`], which checks after each one, successful
//! or refused, that the engine is not left waiting for a reply.

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};
use clock_core::clock_actor;
use integration_test_methods::PRICE_SOURCE_ELF;
use integration_tests::{account, deploy_at, key, submit, with_clock};
use lee::{
    error::LeeError,
    public_transaction::{Message as TransactionMessage, WitnessSet},
    PrivateKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{AccountId, Actor},
    program::{PdaSeed, TransactionEvent},
};
use stablecoin_core::{
    compute_engine_pda, compute_position_vault_pda, compute_stablecoin_definition_pda,
    compute_surplus_holding_pda,
    event::{selector, StablecoinEvent},
    Engine, InitializeArgs, Message, Position, StablecoinState, FIXED_POINT_ONE,
    MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
};
use stablecoin_methods::STABLECOIN_ELF;
use token_core::{TokenDefinition, TokenHolding, TokenState};
use token_methods::TOKEN_ELF;
use twap_oracle_core::{
    compute_current_tick_account_pda_seed, compute_oracle_price_account_pda,
    compute_price_observations_pda, OraclePrice, OracleState, PriceSource,
};
use twap_oracle_methods::TWAP_ORACLE_ELF;

pub const TOKEN: AccountId = AccountId::new([0xA0; 32]);
pub const ORACLE: AccountId = AccountId::new([0xA1; 32]);
/// The price source guest (`methods/guest/src/bin/price_source.rs`), standing in for the AMM.
pub const SOURCE: AccountId = AccountId::new([0xA2; 32]);
/// The oracle window the engine reads.
pub const WINDOW: u64 = 2_048_000;

pub const T0: u64 = 1_000_000;
pub const COLLATERAL_SUPPLY: u128 = 1_000_000_000;
/// Q64.64 one, the oracle's price unit.
pub const ORACLE_ONE: u128 = 1 << 64;

/// Called with the state and every stablecoin transaction just before it is applied. The cycle
/// measurements use it to read what each transaction is charged.
pub type Observer = fn(&V03State, &PublicTransaction);

pub struct World {
    pub state: V03State,
    pub stablecoin: AccountId,
    pub token: AccountId,
    pub oracle: AccountId,
    pub program_key: PrivateKey,
    pub admin: PrivateKey,
    pub freeze_authority: PrivateKey,
    pub collateral_definition: AccountId,
    pub observer: Option<Observer>,
}

/// The key whose account holds the whole collateral supply.
pub fn collateral_holder() -> PrivateKey {
    key(5)
}

/// The token, the TWAP oracle, the price source and the stablecoin deployed, the clock at `T0`,
/// and a collateral token whose supply is held by [`collateral_holder`]. Not initialized.
pub fn world() -> World {
    let program_key = key(1);
    let stablecoin = account(&program_key);
    let state = deploy_at(&[
        (stablecoin, STABLECOIN_ELF),
        (TOKEN, TOKEN_ELF),
        (ORACLE, TWAP_ORACLE_ELF),
        (SOURCE, PRICE_SOURCE_ELF),
    ]);
    let state = with_clock(state, T0);
    let mut world = World {
        state,
        stablecoin,
        token: TOKEN,
        oracle: ORACLE,
        program_key,
        admin: key(2),
        freeze_authority: key(3),
        collateral_definition: account(&key(4)),
        observer: None,
    };
    create_token(
        &mut world,
        &key(4),
        "Collateral",
        COLLATERAL_SUPPLY,
        &collateral_holder(),
    );
    world
}

pub fn stablecoin(world: &World, account_id: AccountId) -> Actor {
    Actor::new(account_id, world.stablecoin)
}

pub fn token(world: &World, account_id: AccountId) -> Actor {
    Actor::new(account_id, world.token)
}

pub fn engine_id(world: &World) -> AccountId {
    compute_engine_pda(world.stablecoin)
}

pub fn definition_id(world: &World) -> AccountId {
    compute_stablecoin_definition_pda(world.stablecoin)
}

pub fn surplus_id(world: &World) -> AccountId {
    compute_surplus_holding_pda(world.stablecoin)
}

pub fn vault_id(world: &World, owner: AccountId) -> AccountId {
    compute_position_vault_pda(world.stablecoin, owner)
}

// ── Sending ──────────────────────────────────────────────────────────────────────

/// A public transaction with `message` to `root`, declaring `declared` and signed by `signers`
/// at their current nonces (what `integration_tests::submit` builds).
pub fn transaction(
    state: &V03State,
    root: Actor,
    declared: &[Actor],
    signers: &[&PrivateKey],
    message: &impl BorshSerialize,
) -> PublicTransaction {
    let nonces: BTreeMap<_, _> = signers
        .iter()
        .map(|key| {
            let id = account(key);
            (id, state.get_account_by_id(id).nonce)
        })
        .collect();
    let message = TransactionMessage::try_new(root, declared.iter().copied(), nonces, message)
        .expect("the transaction message encodes");
    let witness_set = WitnessSet::for_message(&message, signers);
    PublicTransaction::new(message, witness_set)
}

/// Sends a stablecoin transaction: `root` (declared too) and the actors past it, signed by
/// `signers`. Afterwards, whatever the outcome, the engine (once it exists) is not waiting.
pub fn send(
    world: &mut World,
    root: Actor,
    past_root: &[Actor],
    signers: &[&PrivateKey],
    message: &impl BorshSerialize,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let mut declared = vec![root];
    declared.extend_from_slice(past_root);
    let tx = transaction(&world.state, root, &declared, signers, message);
    if let Some(observer) = world.observer {
        observer(&world.state, &tx);
    }
    let result = world.state.transition_from_public_transaction(&tx, 1, 0);
    if let Some(StablecoinState::Engine(engine)) = stablecoin_state(world, engine_id(world)) {
        assert_eq!(
            engine.pending, None,
            "the engine was left waiting after {result:?}"
        );
    }
    result
}

// ── Tokens ───────────────────────────────────────────────────────────────────────

/// A fixed-supply token at `definition`'s account, its supply in `holder`'s account.
pub fn create_token(
    world: &mut World,
    definition: &PrivateKey,
    name: &str,
    supply: u128,
    holder: &PrivateKey,
) {
    let root = token(world, account(definition));
    let declared = [root, token(world, account(holder))];
    submit(
        &mut world.state,
        root,
        &declared,
        &[definition, holder],
        &token_core::Message::NewDefinition {
            name: name.to_owned(),
            total_supply: supply,
            mint_authority: None,
            holding: account(holder),
        },
    )
    .expect("the token is created");
}

/// A zero holding of `definition` on `holding`'s account.
pub fn initialize_holding(world: &mut World, holding: &PrivateKey, definition: AccountId) {
    let root = token(world, account(holding));
    let declared = [root, token(world, definition)];
    submit(
        &mut world.state,
        root,
        &declared,
        &[holding],
        &token_core::Message::InitializeHolding { definition },
    )
    .expect("the holding is initialized");
}

/// A token transfer from `from`'s holding, signed by it.
pub fn transfer(
    world: &mut World,
    from: &PrivateKey,
    to: AccountId,
    amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = token(world, account(from));
    let declared = [root, token(world, to)];
    submit(
        &mut world.state,
        root,
        &declared,
        &[from],
        &token_core::Message::Transfer { to, amount },
    )
}

// ── Initialize and the pokes ─────────────────────────────────────────────────────

pub fn initialize_args(world: &World) -> InitializeArgs {
    InitializeArgs {
        initial_admin_account_id: account(&world.admin),
        freeze_authority_account_id: account(&world.freeze_authority),
        token_program_id: world.token,
        collateral_definition_id: world.collateral_definition,
        // The price account the price source's window will have. It does not exist until
        // `publish_price` creates it: the protocol launches before its oracle.
        market_price_oracle_id: price_account_id(world),
        market_price_oracle_program_id: world.oracle,
        // The top of the band (about 100% a year), so fees accrue in whole units within hours.
        initial_stability_fee_per_millisecond: MAXIMUM_STABILITY_FEE_PER_MILLISECOND,
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
pub fn initialize_actors(world: &World) -> Vec<Actor> {
    vec![
        stablecoin(world, surplus_id(world)),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
        stablecoin(world, engine_id(world)),
        clock_actor(),
    ]
}

/// `Initialize` at the program account, signed by it.
pub fn try_initialize(world: &mut World) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, world.stablecoin);
    let past_root = initialize_actors(world);
    let message = Message::from(initialize_args(world));
    let program_key = world.program_key.clone();
    send(world, root, &past_root, &[&program_key], &message)
}

pub fn initialize(world: &mut World) {
    try_initialize(world).expect("initialize succeeds");
}

/// The actors a poke that may mint the surplus reaches, past the engine.
pub fn poke_actors(world: &World) -> Vec<Actor> {
    vec![
        clock_actor(),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
    ]
}

/// A permissionless poke at the engine. It needs no signature.
pub fn poke(
    world: &mut World,
    past_root: &[Actor],
    message: &Message,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, engine_id(world));
    send(world, root, past_root, &[], message)
}

/// The fee poke.
pub fn accrue(world: &mut World) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = poke_actors(world);
    poke(world, &past_root, &Message::AccrueStabilityFee)
}

/// `RefreshGlobals`; with `consult_oracle` the oracle's price actor is declared too.
pub fn refresh(world: &mut World, consult_oracle: bool) -> Result<Vec<TransactionEvent>, LeeError> {
    let mut past_root = poke_actors(world);
    if consult_oracle {
        past_root.push(price_actor(world));
    }
    poke(
        world,
        &past_root,
        &Message::RefreshGlobals { consult_oracle },
    )
}

/// `UpdateRedemptionRate`: the clock, then the oracle's price actor. Nothing is minted.
pub fn update_redemption_rate(world: &mut World) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [clock_actor(), price_actor(world)];
    poke(world, &past_root, &Message::UpdateRedemptionRate)
}

// ── Seats ────────────────────────────────────────────────────────────────────────

/// An admin or freeze-authority message at `seat`'s own account, signed by it. The fee setter
/// accrues at the old rate first (the clock, and the surplus mint); the controller reset reads
/// the clock.
pub fn seat(
    world: &mut World,
    seat: &PrivateKey,
    message: &Message,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, account(seat));
    let mut past_root = vec![stablecoin(world, engine_id(world))];
    if matches!(message, Message::SetStabilityFeePerMillisecond { .. }) {
        past_root.extend(poke_actors(world));
    } else if matches!(message, Message::ResetRedemptionRateController) {
        past_root.push(clock_actor());
    }
    send(world, root, &past_root, &[seat], message)
}

pub fn admin(world: &mut World, message: &Message) -> Result<Vec<TransactionEvent>, LeeError> {
    let admin = world.admin.clone();
    seat(world, &admin, message)
}

pub fn freeze(world: &mut World) {
    let freeze_authority = world.freeze_authority.clone();
    seat(world, &freeze_authority, &Message::Freeze).expect("the freeze authority freezes");
}

pub fn unfreeze(world: &mut World) {
    let freeze_authority = world.freeze_authority.clone();
    seat(world, &freeze_authority, &Message::Unfreeze).expect("the freeze authority unfreezes");
}

// ── Users and positions ──────────────────────────────────────────────────────────

/// A position owner. A token holding is a whole account's token state, so the owner keeps the
/// collateral and the stablecoin in two more accounts.
pub struct User {
    /// The position lives on this account.
    pub owner: PrivateKey,
    /// A collateral holding.
    pub collateral: PrivateKey,
    /// A stablecoin holding.
    pub stable: PrivateKey,
}

impl User {
    pub fn id(&self) -> AccountId {
        account(&self.owner)
    }

    pub fn collateral_id(&self) -> AccountId {
        account(&self.collateral)
    }

    pub fn stable_id(&self) -> AccountId {
        account(&self.stable)
    }
}

/// Keys `byte`, `byte + 1`, `byte + 2`: a collateral holding funded with `collateral` from the
/// holder, and an empty stablecoin holding. Needs the stablecoin initialized.
pub fn user(world: &mut World, byte: u8, collateral: u128) -> User {
    let next = |offset: u8| key(byte.checked_add(offset).expect("key byte fits"));
    let user = User {
        owner: key(byte),
        collateral: next(1),
        stable: next(2),
    };
    initialize_holding(world, &user.collateral, world.collateral_definition);
    initialize_holding(world, &user.stable, definition_id(world));
    if collateral > 0 {
        transfer(
            world,
            &collateral_holder(),
            user.collateral_id(),
            collateral,
        )
        .expect("the holder funds the user");
    }
    user
}

/// A root message at the owner's position, signed by the owner and `also`.
pub fn position_message(
    world: &mut World,
    user: &User,
    past_root: &[Actor],
    also: &[&PrivateKey],
    message: &Message,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let root = stablecoin(world, user.id());
    let mut signers = vec![&user.owner];
    signers.extend_from_slice(also);
    send(world, root, past_root, &signers, message)
}

pub fn open_position(
    world: &mut World,
    user: &User,
    initial_collateral_amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [
        stablecoin(world, engine_id(world)),
        token(world, vault_id(world, user.id())),
        token(world, world.collateral_definition),
        token(world, user.collateral_id()),
    ];
    position_message(
        world,
        user,
        &past_root,
        &[&user.collateral],
        &Message::OpenPosition {
            collateral_holding: user.collateral_id(),
            initial_collateral_amount,
        },
    )
}

/// A deposit from `holding`'s account, which signs.
pub fn deposit_from(
    world: &mut World,
    user: &User,
    holding: &PrivateKey,
    amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [
        token(world, account(holding)),
        token(world, vault_id(world, user.id())),
    ];
    position_message(
        world,
        user,
        &past_root,
        &[holding],
        &Message::DepositCollateral {
            collateral_holding: account(holding),
            amount,
        },
    )
}

pub fn deposit(
    world: &mut World,
    user: &User,
    amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    deposit_from(world, user, &user.collateral, amount)
}

/// A withdrawal to the user's collateral holding.
pub fn withdraw(
    world: &mut World,
    user: &User,
    amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [
        stablecoin(world, engine_id(world)),
        clock_actor(),
        token(world, vault_id(world, user.id())),
        token(world, user.collateral_id()),
    ];
    position_message(
        world,
        user,
        &past_root,
        &[],
        &Message::WithdrawCollateral {
            to: user.collateral_id(),
            amount,
        },
    )
}

/// Debt minted to the user's stablecoin holding.
pub fn generate(
    world: &mut World,
    user: &User,
    amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [
        stablecoin(world, engine_id(world)),
        clock_actor(),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
        token(world, user.stable_id()),
    ];
    position_message(
        world,
        user,
        &past_root,
        &[],
        &Message::GenerateDebt {
            to: user.stable_id(),
            amount,
        },
    )
}

/// A repayment from `holding`'s account, which signs.
pub fn repay_from(
    world: &mut World,
    user: &User,
    holding: &PrivateKey,
    maximum_amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    let past_root = [
        stablecoin(world, engine_id(world)),
        clock_actor(),
        token(world, definition_id(world)),
        token(world, surplus_id(world)),
        token(world, account(holding)),
    ];
    position_message(
        world,
        user,
        &past_root,
        &[holding],
        &Message::RepayDebt {
            stablecoin_holding: account(holding),
            maximum_amount,
        },
    )
}

pub fn repay(
    world: &mut World,
    user: &User,
    maximum_amount: u128,
) -> Result<Vec<TransactionEvent>, LeeError> {
    repay_from(world, user, &user.stable, maximum_amount)
}

pub fn close(world: &mut World, user: &User) -> Result<Vec<TransactionEvent>, LeeError> {
    position_message(world, user, &[], &[], &Message::ClosePosition)
}

// ── The oracle ───────────────────────────────────────────────────────────────────

/// The price source guest's message (`methods/guest/src/bin/price_source.rs`).
#[derive(BorshSerialize, BorshDeserialize)]
enum SourceMessage {
    Forward {
        to: Actor,
        message: Vec<u8>,
        seeds: Vec<PdaSeed>,
    },
}

fn source_key() -> PrivateKey {
    key(20)
}

/// The price source's actor on its key's account.
pub fn source_actor() -> Actor {
    Actor::new(account(&source_key()), SOURCE)
}

fn current_tick_id() -> AccountId {
    PriceSource::program(source_actor()).current_tick_account_id()
}

pub fn price_account_id(world: &World) -> AccountId {
    compute_oracle_price_account_pda(world.oracle, current_tick_id(), WINDOW)
}

/// The oracle's price actor the engine reads.
pub fn price_actor(world: &World) -> Actor {
    Actor::new(price_account_id(world), world.oracle)
}

fn oracle_actor(world: &World, account_id: AccountId) -> Actor {
    Actor::new(account_id, world.oracle)
}

/// The price source's actor sends `message` to `to`, granting `seeds`.
fn forward(
    world: &mut World,
    to: Actor,
    message: &impl BorshSerialize,
    seeds: Vec<PdaSeed>,
    past_root: &[Actor],
) -> Result<Vec<TransactionEvent>, LeeError> {
    let mut declared = vec![source_actor()];
    declared.extend_from_slice(past_root);
    submit(
        &mut world.state,
        source_actor(),
        &declared,
        &[&source_key()],
        &SourceMessage::Forward {
            to,
            message: borsh::to_vec(message).expect("the message encodes"),
            seeds,
        },
    )
}

/// Gives the price source's own actor a state, so LEZ admits messages to it: the stand-in for an
/// oracle whose program accepts anything silently. Its actor is [`source_actor`].
pub fn make_silent_oracle(world: &mut World) {
    forward(world, source_actor(), &0_u8, vec![], &[]).expect("the price source stores a message");
}

/// Creates the price source's current-tick actor at `price` (Q64.64) and opens its window at the
/// clock's time, quoting the stablecoin (base) in collateral (quote): the price account then
/// holds `price` (rounded to its tick) stamped now. Once per world.
pub fn publish_price(world: &mut World, price: u128) {
    let current_tick = current_tick_id();
    forward(
        world,
        oracle_actor(world, current_tick),
        &twap_oracle_core::Message::CreateCurrentTickAccount {
            initial_price: price,
        },
        vec![compute_current_tick_account_pda_seed(
            account(&source_key()),
        )],
        &[oracle_actor(world, current_tick)],
    )
    .expect("the current-tick actor is created");
    let window = [
        oracle_actor(world, current_tick),
        clock_actor(),
        oracle_actor(
            world,
            compute_price_observations_pda(world.oracle, current_tick, WINDOW),
        ),
        price_actor(world),
    ];
    forward(
        world,
        oracle_actor(world, current_tick),
        &twap_oracle_core::Message::CreatePriceAccounts {
            window_duration: WINDOW,
            base_asset: definition_id(world),
            quote_asset: world.collateral_definition,
        },
        vec![],
        &window,
    )
    .expect("the window opens");
}

pub fn oracle_price(world: &World) -> OraclePrice {
    match OracleState::decode(
        world
            .state
            .get_account_by_id(price_account_id(world))
            .data
            .actor_state(world.oracle),
    ) {
        Some(OracleState::Price(price)) => price,
        other => panic!("expected an oracle price, got {other:?}"),
    }
}

// ── Reading state ────────────────────────────────────────────────────────────────

pub fn stablecoin_state(world: &World, account_id: AccountId) -> Option<StablecoinState> {
    StablecoinState::decode(
        world
            .state
            .get_account_by_id(account_id)
            .data
            .actor_state(world.stablecoin),
    )
}

pub fn token_state(world: &World, account_id: AccountId) -> Option<TokenState> {
    TokenState::decode(
        world
            .state
            .get_account_by_id(account_id)
            .data
            .actor_state(world.token),
    )
}

pub fn engine(world: &World) -> Engine {
    match stablecoin_state(world, engine_id(world)) {
        Some(StablecoinState::Engine(engine)) => engine,
        other => panic!("expected the engine, got {other:?}"),
    }
}

pub fn position(world: &World, owner: AccountId) -> Option<Position> {
    match stablecoin_state(world, owner) {
        Some(StablecoinState::Position(position)) => Some(position),
        None => None,
        other => panic!("expected a position, got {other:?}"),
    }
}

pub fn holding(world: &World, account_id: AccountId) -> TokenHolding {
    match token_state(world, account_id) {
        Some(TokenState::Holding(holding)) => holding,
        other => panic!("expected a token holding, got {other:?}"),
    }
}

pub fn definition(world: &World, definition_id: AccountId) -> TokenDefinition {
    match token_state(world, definition_id) {
        Some(TokenState::Definition(definition)) => definition,
        other => panic!("expected a token definition, got {other:?}"),
    }
}

pub fn holding_balance(world: &World, account_id: AccountId) -> u128 {
    holding(world, account_id).balance
}

pub fn total_supply(world: &World, definition_id: AccountId) -> u128 {
    definition(world, definition_id).total_supply
}

/// The stablecoin events of type `E` in a transaction's events.
pub fn events_of<E: StablecoinEvent + BorshDeserialize>(events: &[TransactionEvent]) -> Vec<E> {
    let selector = selector(E::NAME);
    events
        .iter()
        .filter(|record| record.event.selector == selector)
        .map(|record| borsh::from_slice(&record.event.data).expect("the event decodes"))
        .collect()
}
