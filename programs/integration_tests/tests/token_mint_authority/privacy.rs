//! Mixed public/private faucet claims, proved and settled through LEZ.

use std::borrow::Cow;

use lee::{
    execute_and_prove,
    privacy_preserving_transaction::{circuit::ProgramCatalog, Message, WitnessSet},
    program::Program,
    test_utils::{init_witness, no_seal, update_witness, TestPrivateKeys},
    PrivacyPreservingTransaction, ProvingInput, PublicExecutionContext, RootCall,
    SenderPresentation, Simulation, TransactionEntry,
};
use lee_core::{
    account::{Account, Nonce},
    AuthorizationSecretKey, Commitment, Nullifier, PrivateWitness, RegularKey, WitnessKind,
};
use token_core::TokenHolding;
use token_mint_authority_core::compute_mint_authority_pda;

use super::*;

fn private_keys() -> TestPrivateKeys {
    TestPrivateKeys {
        ask: AuthorizationSecretKey([42; 32]),
        d: [43; 32],
        z: [44; 32],
    }
}

/// Address the private participant through its blinded alias. The public allowance is still
/// derived from the claimant's canonical account. A private holding must promote its credit.
fn private_claim(
    chain: &mut Chain,
    recipient: AccountId,
    definition: AccountId,
    holding: AccountId,
    witness: PrivateWitness,
    signers: &[&PrivateKey],
    promote_credit: bool,
) -> Result<(), LeeError> {
    let private_id = witness.account_id();
    let factor = *witness
        .address_openings
        .first()
        .expect("private address opening");
    let alias = private_id.blinded(&factor);
    let presented = |id| if id == private_id { alias } else { id };
    let public: Vec<_> = claim_actors(recipient, definition, holding)
        .into_iter()
        .filter(|actor| actor.account_id != private_id)
        .collect();
    let simulation = Simulation {
        public_actor_states: public
            .iter()
            .map(|actor| {
                let data = chain.state.get_account_by_id(actor.account_id).data;
                (*actor, data.actor_state(actor.program_account_id).clone())
            })
            .collect(),
        admitted_accounts: None,
    };
    let programs = ProgramCatalog::from([
        (
            TOKEN,
            Program::new(Cow::Borrowed(TOKEN_ELF)).expect("token guest loads"),
        ),
        (
            FAUCET,
            Program::new(Cow::Borrowed(TOKEN_MINT_AUTHORITY_ELF)).expect("faucet guest loads"),
        ),
        (clock_core::clock_account_id(), programs::clock()),
    ]);
    let input = ProvingInput {
        root: TransactionEntry::Sync(RootCall {
            to: faucet(presented(recipient)),
            message: borsh::to_vec(&claim_message(definition, presented(holding)))
                .expect("claim encodes"),
        }),
        public_context: PublicExecutionContext::new(public, signers.iter().map(|k| account(k))),
        private_witnesses: vec![witness],
        claims: Vec::new(),
        dummy_inputs: Vec::new(),
        ciphertext_padding: None,
        recoveries: Vec::new(),
        private_promotions: Default::default(),
        fee: None,
    };
    let (output, proof) = execute_and_prove(
        input,
        &simulation,
        &programs,
        |_| SenderPresentation::Blinded(factor),
        |_, body| promote_credit && body.to == token(alias),
        no_seal,
    )?;
    let nonces = signers
        .iter()
        .map(|k| {
            let id = account(k);
            (id, chain.state.get_account_by_id(id).nonce)
        })
        .collect();
    let message = Message::from_circuit_output(nonces, output);
    let witness_set = WitnessSet::for_message(&message, proof, signers);
    chain.state.transition_from_privacy_preserving_transaction(
        &PrivacyPreservingTransaction::new(message, witness_set),
        1,
        0,
    )
}

#[test]
fn a_public_recipient_can_claim_to_a_private_holding() {
    let mut chain = Chain::new();
    let (definition, recipient) = (key(1), key(2));
    chain.define(
        &definition,
        &recipient,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let definition = account(&definition);
    let keys = private_keys();
    let holding = keys.account_id();
    let holding_state = |balance| {
        TokenState::Holding(TokenHolding {
            definition_id: definition,
            balance,
        })
        .encode()
        .into()
    };
    let pre = Account {
        nonce: Nonce::default().private_account_nonce_increment(&keys.nsk()),
        ..Account::default().with_actor_state(TOKEN, holding_state(0))
    };
    let commitment = Commitment::new(&holding, &pre);
    let predecessor = Commitment::new(&holding, &Account::default());
    chain.state = chain.state.with_private_accounts([(
        commitment,
        Nullifier::for_account_update(&predecessor, &keys.nsk()),
    )]);
    let membership = chain
        .state
        .get_proof_for_commitment(&commitment)
        .expect("holding is shielded");
    let mut witness = update_witness(&keys, pre.clone(), membership);
    witness.kind = WitnessKind::Regular(RegularKey::Nullifying(keys.nsk()));
    witness.address_openings.insert([11; 32]);

    let unpromoted = private_claim(
        &mut chain,
        account(&recipient),
        definition,
        holding,
        witness.clone(),
        &[&recipient],
        false,
    );
    assert_refused(&unpromoted, "faucet credit did not settle");
    assert_eq!(chain.supply(definition), 0);
    assert_eq!(chain.last_mint(account(&recipient), definition), None);

    private_claim(
        &mut chain,
        account(&recipient),
        definition,
        holding,
        witness,
        &[&recipient],
        true,
    )
    .expect("a promoted private credit settles");

    assert_eq!(chain.supply(definition), FAUCET_MINT_AMOUNT);
    assert_eq!(chain.last_mint(account(&recipient), definition), Some(T0));
    let post = Account {
        nonce: pre.nonce.private_account_nonce_increment(&keys.nsk()),
        ..pre.with_actor_state(TOKEN, holding_state(FAUCET_MINT_AMOUNT))
    };
    assert!(chain
        .state
        .get_proof_for_commitment(&Commitment::new(&holding, &post))
        .is_some());
}

#[test]
fn a_private_recipient_claims_without_a_public_signer() {
    let mut chain = Chain::new();
    let (definition, holder) = (key(1), key(2));
    chain.define(
        &definition,
        &holder,
        compute_mint_authority_pda(FAUCET, TOKEN),
    );
    let (definition, holding) = (account(&definition), account(&holder));
    let keys = private_keys();
    let recipient = keys.account_id();
    let mut witness = init_witness(&keys);
    witness.address_openings.insert([7; 32]);

    private_claim(
        &mut chain,
        recipient,
        definition,
        holding,
        witness,
        &[],
        false,
    )
    .expect("private recipient claim settles");

    assert_eq!(chain.last_mint(recipient, definition), Some(T0));
    assert_eq!(chain.balance(holding), FAUCET_MINT_AMOUNT);
    let post = Account {
        nonce: Nonce::default().private_account_nonce_increment(&keys.nsk()),
        ..Account::default()
    };
    let membership = chain
        .state
        .get_proof_for_commitment(&Commitment::new(&recipient, &post))
        .expect("the claimant's private note is committed");
    // A new presentation of the same private claimant shares its existing cooldown.
    let mut next = update_witness(&keys, post, membership);
    next.address_openings.insert([9; 32]);
    let retry = private_claim(&mut chain, recipient, definition, holding, next, &[], false);
    assert_refused(&retry, "cooldown");
    assert_eq!(chain.balance(holding), FAUCET_MINT_AMOUNT);
}
