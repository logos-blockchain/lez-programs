//! End-to-end tests for the Token-Mint-Authority faucet, driven through the zkVM
//! executor (dev mode). These exercise what the host-function unit tests cannot:
//! the runtime `user -> token-mint-authority -> token` chained `MintWithAuthority` under
//! the mint-authority PDA seed, the lazily-claimed allowance PDA, and the
//! per-day cooldown across real transactions.

use std::collections::HashMap;

use clock_core::{ClockAccountData, CLOCK_01_PROGRAM_ACCOUNT_ID};
use integration_tests::{
    private_authorized_init_identity, private_authorized_update_identity, GroupOwner,
};
use lee::{
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::ProgramWithDependencies, Message, PrivacyPreservingTransaction, WitnessSet,
    },
    program::Program,
    program_deployment_transaction::{self, ProgramDeploymentTransaction},
    public_transaction, PrivateKey, PublicKey, PublicTransaction, V03State,
};
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data, Nonce},
    encryption::ViewingPublicKey,
    Commitment, InputAccountIdentity, Nullifier, NullifierPublicKey, NullifierSecretKey,
};
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

// ---------------------------------------------------------------------------
// Privacy-preserving coverage
//
// The faucet is a pure delegating proxy (`user -> token-mint-authority -> token`), so every
// one of these goes through a chained `Token::MintWithAuthority` (category CHAIN). Two
// account slots can plausibly be private: the `recipient` identity that signs and is
// rate-limited, and the `user_holding` the grant lands in.
// ---------------------------------------------------------------------------

/// Private-account key material for the faucet's privacy tests.
struct PrivateKeys;

impl PrivateKeys {
    fn holding_nsk() -> NullifierSecretKey {
        [61; 32]
    }

    fn holding_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::holding_nsk())
    }

    fn holding_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[71; 32], &[72; 32])
    }

    fn holding_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::holding_npk(), &Self::holding_vpk(), 0)
    }

    fn recipient_nsk() -> NullifierSecretKey {
        [62; 32]
    }

    fn recipient_npk() -> NullifierPublicKey {
        NullifierPublicKey::from(&Self::recipient_nsk())
    }

    fn recipient_vpk() -> ViewingPublicKey {
        ViewingPublicKey::from_seed(&[73; 32], &[74; 32])
    }

    fn recipient_id() -> AccountId {
        AccountId::for_regular_private_account(&Self::recipient_npk(), &Self::recipient_vpk(), 0)
    }
}

fn faucet_with_deps() -> ProgramWithDependencies {
    ProgramWithDependencies::new(
        Program::new(
            token_mint_authority_methods::TOKEN_MINT_AUTHORITY_ELF
                .to_vec()
                .into(),
        )
        .expect("valid token-mint-authority ELF"),
        HashMap::from([(
            Ids::token_program(),
            Program::new(token_methods::TOKEN_ELF.to_vec().into()).expect("valid token ELF"),
        )]),
    )
}

/// The faucet's mint-allowance PDA for an arbitrary recipient — the public `Ids::mint_allowance`
/// is hard-wired to the public recipient, and these tests vary the recipient identity.
fn mint_allowance_for(recipient: AccountId) -> AccountId {
    compute_mint_allowance_pda(
        Ids::token_mint_authority_program(),
        recipient,
        Ids::faucet_definition(),
    )
}

/// REGULAR, CHAIN: the grant lands in an already-shielded private holding. The recipient
/// identity (and therefore the rate-limit PDA) stays public; only the funded holding is private,
/// so the faucet's chained `MintWithAuthority` has to credit a private account.
#[test]
fn faucet_mint_into_private_user_holding() {
    let mut state = state_for_faucet_tests();

    let holding_nsk = PrivateKeys::holding_nsk();
    let holding_vpk = PrivateKeys::holding_vpk();
    let holding_id = PrivateKeys::holding_id();
    let holding_account = Account {
        nonce: Nonce::private_account_nonce_init(&holding_id),
        ..user_holding_init()
    };
    state = state.with_private_accounts([(
        Commitment::new(&holding_id, &holding_account),
        Nullifier::for_account_initialization(&holding_id),
    )]);
    let membership_proof = state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &holding_account))
        .expect("the private holding's commitment must be in the set");

    let recipient_account = state.get_account_by_id(Ids::recipient());
    let recipient_nonce = recipient_account.nonce;
    let recipient_pre = AccountWithMetadata::new(recipient_account, true, Ids::recipient());
    let allowance_pre = AccountWithMetadata::new(Account::default(), false, Ids::mint_allowance());
    // A `PrivateAuthorizedUpdate` pre-state must be authorized — the circuit requires it for any
    // authenticated private account, even though the faucet itself never checks the holding.
    let user_holding_pre = AccountWithMetadata::new(holding_account, true, holding_id);
    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::faucet_definition()),
        false,
        Ids::faucet_definition(),
    );
    let mint_authority_pre =
        AccountWithMetadata::new(Account::default(), false, Ids::mint_authority());
    let clock_pre = AccountWithMetadata::new(
        state.get_account_by_id(CLOCK_01_PROGRAM_ACCOUNT_ID),
        false,
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    );

    let (output, proof) = execute_and_prove(
        vec![
            recipient_pre,
            allowance_pre,
            user_holding_pre,
            definition_pre,
            mint_authority_pre,
            clock_pre,
        ],
        Program::serialize_instruction(token_mint_authority_core::Instruction::FaucetMint).unwrap(),
        vec![
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            private_authorized_update_identity(holding_nsk, &holding_vpk, membership_proof),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &faucet_with_deps(),
    )
    .expect("FaucetMint into an existing private holding must succeed");

    let message = Message::from_circuit_output(vec![recipient_nonce], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[&Keys::recipient()]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    // Public side: the supply grew and the allowance PDA was claimed at the current clock.
    assert_eq!(
        definition_supply(&state, Ids::faucet_definition()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(allowance_last_mint(&state, Ids::mint_allowance()), T0);

    // Private side: the shielded holding carries the grant, under its incremented nonce.
    let funded = Account {
        data: Data::from(&TokenHolding::Fungible {
            definition_id: Ids::faucet_definition(),
            balance: FAUCET_MINT_AMOUNT,
        }),
        nonce: Nonce::private_account_nonce_init(&holding_id)
            .private_account_nonce_increment(&holding_nsk),
        ..user_holding_init()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&holding_id, &funded))
        .is_some());
}

/// REGULAR, CHAIN: the rate-limited `recipient` itself is a private account. `recipient` is the
/// faucet's only `#[account(signer)]`, and its account id is a seed of the mint-allowance PDA —
/// so this proves the per-account cooldown keys off a private identity just as well as a public
/// one, with no public signer in the transaction at all.
#[test]
fn faucet_mint_with_private_recipient() {
    let mut state = state_for_faucet_tests();

    let recipient_id = PrivateKeys::recipient_id();
    let allowance_id = mint_allowance_for(recipient_id);

    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);
    let allowance_pre = AccountWithMetadata::new(Account::default(), false, allowance_id);
    let user_holding_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::user_holding()),
        false,
        Ids::user_holding(),
    );
    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::faucet_definition()),
        false,
        Ids::faucet_definition(),
    );
    let mint_authority_pre =
        AccountWithMetadata::new(Account::default(), false, Ids::mint_authority());
    let clock_pre = AccountWithMetadata::new(
        state.get_account_by_id(CLOCK_01_PROGRAM_ACCOUNT_ID),
        false,
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    );

    let (output, proof) = execute_and_prove(
        vec![
            recipient_pre,
            allowance_pre,
            user_holding_pre,
            definition_pre,
            mint_authority_pre,
            clock_pre,
        ],
        Program::serialize_instruction(token_mint_authority_core::Instruction::FaucetMint).unwrap(),
        vec![
            private_authorized_init_identity(
                PrivateKeys::recipient_nsk(),
                &PrivateKeys::recipient_vpk(),
                state.commitment_root(),
            ),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &faucet_with_deps(),
    )
    .expect("FaucetMint signed by a private recipient must succeed");

    let message = Message::from_circuit_output(vec![], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    // The grant landed, and the cooldown is now anchored to the *private* recipient's PDA.
    assert_eq!(
        holding_balance(&state, Ids::user_holding()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(allowance_last_mint(&state, allowance_id), T0);
    assert_eq!(
        MintAllowance::try_from(&state.get_account_by_id(allowance_id).data)
            .expect("valid allowance")
            .recipient_id,
        recipient_id
    );

    // The private recipient marker survived as a commitment under its init nonce.
    let recipient_expected = Account {
        nonce: Nonce::private_account_nonce_init(&recipient_id),
        ..Account::default()
    };
    assert!(state
        .get_proof_for_commitment(&Commitment::new(&recipient_id, &recipient_expected))
        .is_some());
}

/// GROUP, CHAIN: same as [`faucet_mint_with_private_recipient`], but the recipient is a
/// group-owned account — a member who received the Group Master Secret through the real
/// seal/unseal handshake (never the key itself) signs the faucet claim.
#[test]
fn faucet_mint_with_group_owned_recipient() {
    let mut state = state_for_faucet_tests();

    let alice = GroupOwner::new([41_u8; 32]);
    let bob_nsk = alice.admit_member();
    let recipient_id = alice.id;
    let allowance_id = mint_allowance_for(recipient_id);

    let recipient_pre = AccountWithMetadata::new(Account::default(), true, recipient_id);
    let allowance_pre = AccountWithMetadata::new(Account::default(), false, allowance_id);
    let user_holding_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::user_holding()),
        false,
        Ids::user_holding(),
    );
    let definition_pre = AccountWithMetadata::new(
        state.get_account_by_id(Ids::faucet_definition()),
        false,
        Ids::faucet_definition(),
    );
    let mint_authority_pre =
        AccountWithMetadata::new(Account::default(), false, Ids::mint_authority());
    let clock_pre = AccountWithMetadata::new(
        state.get_account_by_id(CLOCK_01_PROGRAM_ACCOUNT_ID),
        false,
        CLOCK_01_PROGRAM_ACCOUNT_ID,
    );

    let (output, proof) = execute_and_prove(
        vec![
            recipient_pre,
            allowance_pre,
            user_holding_pre,
            definition_pre,
            mint_authority_pre,
            clock_pre,
        ],
        Program::serialize_instruction(token_mint_authority_core::Instruction::FaucetMint).unwrap(),
        vec![
            private_authorized_init_identity(bob_nsk, &alice.vpk, state.commitment_root()),
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
            InputAccountIdentity::Public,
        ],
        &faucet_with_deps(),
    )
    .expect("FaucetMint signed by a group-owned recipient must succeed");

    let message = Message::from_circuit_output(vec![], output);
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    state
        .transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            0,
            0,
        )
        .unwrap();

    assert_eq!(
        holding_balance(&state, Ids::user_holding()),
        FAUCET_MINT_AMOUNT
    );
    assert_eq!(
        MintAllowance::try_from(&state.get_account_by_id(allowance_id).data)
            .expect("valid allowance")
            .recipient_id,
        recipient_id
    );
}
