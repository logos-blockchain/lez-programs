//! End-to-end tests for the Token-Mint-Authority faucet, driven through the zkVM
//! executor (dev mode). These exercise what the host-function unit tests cannot:
//! the runtime `user -> token-mint-authority -> token` chained `MintWithAuthority` under
//! the mint-authority PDA seed, the lazily-claimed allowance PDA, and the
//! per-day cooldown across real transactions.

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use lee::{
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::account::{Account, AccountId, Data, Nonce};
use token_core::{TokenDefinition, TokenHolding};
use token_mint_authority_core::{
    compute_mint_allowance_pda, compute_mint_authority_pda, MintAllowance, FAUCET_MINT_AMOUNT,
    MINT_COOLDOWN_MS,
};

struct Keys;
struct Ids;

/// Faucet-mint wall-clock anchor (Unix milliseconds).
const T0: u64 = 1_700_000_000_000;

impl Keys {
    fn recipient() -> PrivateKey {
        PrivateKey::try_new([21; 32]).expect("valid private key")
    }

    fn faucet_definition() -> PrivateKey {
        PrivateKey::try_new([23; 32]).expect("valid private key")
    }

    fn user_holding() -> PrivateKey {
        PrivateKey::try_new([24; 32]).expect("valid private key")
    }
}

impl Ids {
    fn token_program() -> lee_core::program::ProgramId {
        token_methods::TOKEN_ID
    }

    fn token_mint_authority_program() -> lee_core::program::ProgramId {
        token_mint_authority_methods::TOKEN_MINT_AUTHORITY_ID
    }

    fn recipient() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::recipient()))
    }

    fn faucet_definition() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::faucet_definition()))
    }

    fn user_holding() -> AccountId {
        AccountId::from(&PublicKey::new_from_private_key(&Keys::user_holding()))
    }

    /// The faucet token's mint authority — a Token-Mint-Authority PDA the deploy step
    /// wires into the definition. Uninitialized until first use.
    fn mint_authority() -> AccountId {
        compute_mint_authority_pda(Ids::token_mint_authority_program())
    }

    /// The recipient's per-token rate-limit PDA. Uninitialized until first use.
    fn mint_allowance() -> AccountId {
        compute_mint_allowance_pda(
            Ids::token_mint_authority_program(),
            Ids::recipient(),
            Ids::faucet_definition(),
        )
    }
}

/// The faucet token definition: a normal fungible whose mint authority is the
/// Token-Mint-Authority PDA. Starts at zero supply.
fn faucet_definition_init() -> Account {
    Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenDefinition::Fungible {
            name: String::from("Faucet Token"),
            total_supply: 0,
            metadata_id: None,
            authority: Some(Ids::mint_authority()),
        }),
        nonce: Nonce(0),
    }
}

/// The recipient's existing holding for the faucet token (so no holding
/// signature is needed — the Token Program just writes to it).
fn user_holding_init() -> Account {
    Account {
        program_owner: Ids::token_program(),
        balance: 0,
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::faucet_definition(),
            balance: 0,
        }),
        nonce: Nonce(0),
    }
}

/// The recipient identity that signs and is rate-limited. Non-default owner so
/// its (unchanged) post-state survives the framework output filter as its nonce
/// bumps across transactions.
fn recipient_init() -> Account {
    Account {
        program_owner: [7u32; 8],
        ..Account::default()
    }
}

/// Seed the canonical `CLOCK_01` account at `timestamp`. Non-default owner for
/// the same output-filter reason the stablecoin tests use.
fn seed_clock(state: &mut V03State, timestamp: u64) {
    let data = ClockAccountData {
        block_id: 0,
        timestamp,
    }
    .to_bytes();
    let clock_account = Account {
        program_owner: [8u32; 8],
        data: Data::try_from(data).expect("clock account data fits"),
        ..Account::default()
    };
    state.force_insert_account(CLOCK_01_PROGRAM_ACCOUNT_ID, clock_account);
}

fn deploy_programs(state: &mut V03State) {
    for elf in [
        token_methods::TOKEN_ELF.to_vec(),
        token_mint_authority_methods::TOKEN_MINT_AUTHORITY_ELF.to_vec(),
    ] {
        state
            .transition_from_program_deployment_transaction(&ProgramDeploymentTransaction::new(
                program_deployment_transaction::Message::new(elf),
            ))
            .expect("program deployment must succeed");
    }
}

fn state_for_faucet_tests() -> V03State {
    let mut state = V03State::new();
    deploy_programs(&mut state);
    seed_clock(&mut state, T0);
    state.force_insert_account(Ids::faucet_definition(), faucet_definition_init());
    state.force_insert_account(Ids::user_holding(), user_holding_init());
    state.force_insert_account(Ids::recipient(), recipient_init());
    state
}

fn current_nonce(state: &V03State, account_id: AccountId) -> Nonce {
    state.get_account_by_id(account_id).nonce
}

/// Submit one `FaucetMint`. Only the recipient signs; the mint-authority and
/// allowance PDAs are authorized/claimed by the program via seeds.
fn faucet_mint(state: &mut V03State) -> Result<(), lee::error::LeeError> {
    let message = public_transaction::Message::try_new(
        Ids::token_mint_authority_program(),
        vec![
            Ids::recipient(),
            Ids::mint_allowance(),
            Ids::user_holding(),
            Ids::faucet_definition(),
            Ids::mint_authority(),
            CLOCK_01_PROGRAM_ACCOUNT_ID,
        ],
        vec![current_nonce(state, Ids::recipient())],
        token_mint_authority_core::Instruction::FaucetMint,
    )
    .expect("faucet-mint message is valid");
    let witness_set = public_transaction::WitnessSet::for_message(&message, &[&Keys::recipient()]);
    let tx = PublicTransaction::new(message, witness_set);
    state
        .transition_from_public_transaction(&tx, 0, 0)
        .map(|_| ())
}

fn holding_balance(state: &V03State, account_id: AccountId) -> u128 {
    match TokenHolding::try_from(&state.get_account_by_id(account_id).data).expect("valid holding")
    {
        TokenHolding::Fungible { balance, .. } => balance,
        TokenHolding::NftMaster { .. } | TokenHolding::NftPrintedCopy { .. } => {
            panic!("expected a fungible holding")
        }
    }
}

fn definition_supply(state: &V03State, account_id: AccountId) -> u128 {
    match TokenDefinition::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid definition")
    {
        TokenDefinition::Fungible { total_supply, .. } => total_supply,
        TokenDefinition::NonFungible { .. } => panic!("expected a fungible definition"),
    }
}

fn allowance_last_mint(state: &V03State, account_id: AccountId) -> u64 {
    MintAllowance::try_from(&state.get_account_by_id(account_id).data)
        .expect("valid allowance")
        .last_mint_ms
}

#[test]
fn faucet_grants_fixed_amount_and_enforces_daily_cooldown() {
    let mut state = state_for_faucet_tests();

    // 1. First mint: the chained MintWithAuthority credits exactly FAUCET_MINT_AMOUNT and the
    //    allowance PDA is claimed with the current timestamp.
    faucet_mint(&mut state).expect("first faucet mint must succeed");
    assert_eq!(
        holding_balance(&state, Ids::user_holding()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(
        definition_supply(&state, Ids::faucet_definition()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(allowance_last_mint(&state, Ids::mint_allowance()), T0);
    // The program owns the allowance PDA now, but never the authority PDA.
    assert_eq!(
        state.get_account_by_id(Ids::mint_allowance()).program_owner,
        Ids::token_mint_authority_program()
    );

    // 2. Second mint at the same clock: blocked by the cooldown, state unchanged.
    assert!(
        faucet_mint(&mut state).is_err(),
        "a second mint within 24h must be rejected"
    );
    assert_eq!(
        holding_balance(&state, Ids::user_holding()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(allowance_last_mint(&state, Ids::mint_allowance()), T0);

    // 3. Advance the clock past the cooldown: minting is allowed again and stacks.
    seed_clock(&mut state, T0 + MINT_COOLDOWN_MS);
    faucet_mint(&mut state).expect("mint after the cooldown must succeed");
    assert_eq!(
        holding_balance(&state, Ids::user_holding()),
        2 * FAUCET_MINT_AMOUNT
    );
    assert_eq!(
        definition_supply(&state, Ids::faucet_definition()),
        2 * FAUCET_MINT_AMOUNT
    );
    assert_eq!(
        allowance_last_mint(&state, Ids::mint_allowance()),
        T0 + MINT_COOLDOWN_MS
    );
}

#[test]
fn faucet_rejects_a_token_whose_authority_is_not_the_mint_authority_pda() {
    let mut state = state_for_faucet_tests();
    // Re-point the faucet token's mint authority at some unrelated account: the
    // Token-Mint-Authority must refuse to mint a token it does not control.
    let mut definition = faucet_definition_init();
    definition.data = Data::from(&TokenDefinition::Fungible {
        name: String::from("Faucet Token"),
        total_supply: 0,
        metadata_id: None,
        authority: Some(AccountId::new([0xEE; 32])),
    });
    state.force_insert_account(Ids::faucet_definition(), definition);

    assert!(
        faucet_mint(&mut state).is_err(),
        "minting a token not controlled by the mint-authority PDA must fail"
    );
    assert_eq!(holding_balance(&state, Ids::user_holding()), 0);
}
