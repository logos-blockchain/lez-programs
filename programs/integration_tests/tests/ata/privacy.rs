use std::collections::BTreeSet;

use integration_tests::{account, assert_refused, key};
use lee::{
    error::LeeError,
    privacy_preserving_transaction::{
        circuit::{execute_and_prove, ProgramCatalog, Simulation},
        Message, WitnessSet,
    },
    program::Program,
    test_utils::{init_witness, no_seal, update_witness, TestPrivateKeys},
    PrivacyPreservingTransaction, PrivateKey, ValidatedStateDiff,
};
use lee_core::{
    account::{Account, AccountId, Actor},
    execution_state::{BoundaryStep, PublicExecutionContext, TransactionEntry},
    program::TransactionEvent,
    AuthorizationSecretKey, Commitment, EncryptionScheme, PrivacyPreservingCircuitOutput,
    ProvingInput, RegularKey, RootCall, SenderPresentation, SharedSecretKey, WitnessKind,
};
use nft_core::NftState;
use token_core::TokenState;

use super::support::*;

struct Private {
    keys: TestPrivateKeys,
    account: Option<Account>,
}

impl Private {
    fn new(byte: u8) -> Self {
        Self {
            keys: TestPrivateKeys {
                ask: AuthorizationSecretKey([byte; 32]),
                d: [byte; 32],
                z: [byte; 32],
            },
            account: None,
        }
    }

    fn id(&self) -> AccountId {
        self.keys.account_id()
    }

    fn state(&self) -> &Account {
        self.account.as_ref().expect("private account committed")
    }

    fn balance(&self) -> u128 {
        let Some(TokenState::Holding(holding)) =
            TokenState::decode(self.state().data.actor_state(TOKEN))
        else {
            panic!("private token holding")
        };
        holding.balance
    }

    fn nft(&self) -> NftState {
        NftState::decode(self.state().data.actor_state(NFT)).expect("private NFT holding")
    }

    fn idle(&self) {
        assert!(self.state().data.actor_state(ATA).is_empty());
    }
}

/// Prove, settle and decrypt actual account notes. Failed proofs/settlement update
/// neither the chain nor the wallet's current note. Receiving needs a nullifying
/// witness; only owner spending/holding initialization gets an authorized witness.
fn private_call(
    chain: &mut Chain,
    root: Actor,
    public: &[Actor],
    signers: &[&PrivateKey],
    private: &mut [(&mut Private, bool)],
    message: &impl borsh::BorshSerialize,
    promote_credit: bool,
) -> Result<(PrivacyPreservingCircuitOutput, Vec<TransactionEvent>), LeeError> {
    let witnesses = private
        .iter()
        .map(|(wallet, authorized)| {
            let mut witness = match &wallet.account {
                None => init_witness(&wallet.keys),
                Some(state) => update_witness(
                    &wallet.keys,
                    state.clone(),
                    chain
                        .state
                        .get_proof_for_commitment(&Commitment::new(&wallet.id(), state))
                        .expect("current private note is committed"),
                ),
            };
            if !authorized {
                witness.kind = WitnessKind::Regular(RegularKey::Nullifying(wallet.keys.nsk()));
            }
            witness
        })
        .collect();
    let private_ids: BTreeSet<_> = private.iter().map(|(wallet, _)| wallet.id()).collect();
    let programs: ProgramCatalog = [
        (ATA, ata_methods::ATA_ELF),
        (TOKEN, token_methods::TOKEN_ELF),
        (NFT, nft_methods::NFT_ELF),
    ]
    .into_iter()
    .map(|(id, elf)| {
        (
            id,
            Program::new(elf.to_vec().into()).expect("real guest loads"),
        )
    })
    .collect();
    let (output, proof) = execute_and_prove(
        ProvingInput {
            root: TransactionEntry::Sync(RootCall {
                to: root,
                message: borsh::to_vec(message).expect("message encodes"),
            }),
            public_context: PublicExecutionContext::new(
                public.to_vec(),
                signers.iter().map(|signer| account(signer)),
            ),
            private_witnesses: witnesses,
            dummy_inputs: Vec::new(),
            ciphertext_padding: None,
            recoveries: Vec::new(),
            private_promotions: BTreeSet::new(),
        },
        &Simulation {
            public_actor_states: public
                .iter()
                .map(|actor| {
                    (
                        *actor,
                        chain
                            .state
                            .get_account_by_id(actor.account_id)
                            .data
                            .actor_state(actor.program_account_id)
                            .clone(),
                    )
                })
                .collect(),
            admitted_accounts: None,
        },
        &programs,
        |_| SenderPresentation::Canonical,
        |_, body| {
            promote_credit
                && private_ids.contains(&body.to.account_id)
                && [TOKEN, NFT].contains(&body.to.program_account_id)
        },
        no_seal,
    )?;
    assert!(
        output.execution.async_messages.is_empty(),
        "ATA leaves no queued credit"
    );
    let nonces = signers
        .iter()
        .map(|signer| {
            let id = account(signer);
            (id, chain.state.get_account_by_id(id).nonce)
        })
        .collect();
    let tx_message = Message::from_circuit_output(nonces, output.clone());
    let tx_witnesses = WitnessSet::for_message(&tx_message, proof, signers);
    let diff = ValidatedStateDiff::from_privacy_preserving_transaction(
        &PrivacyPreservingTransaction::new(tx_message, tx_witnesses),
        &chain.state,
        1,
        0,
    )?;
    let events = chain.state.apply_state_diff(diff)?;
    // LEZ sorts commitments independently of nullifiers/notes to avoid linking
    // their positions. Match the decrypted account against the whole output set.
    let commitments = output.execution.commitments();
    for (wallet, _) in private {
        let updated = output
            .execution
            .private_actions
            .iter()
            .find_map(|action| {
                let secret = SharedSecretKey::decapsulate(
                    &action.encrypted_post_state.epk,
                    &wallet.keys.d,
                    &wallet.keys.z,
                )?;
                let (_, state) = EncryptionScheme::decrypt(
                    &action.encrypted_post_state.ciphertext,
                    &secret,
                    &action.nullifier,
                )?;
                commitments
                    .contains(&Commitment::new(&wallet.id(), &state))
                    .then_some(state)
            })
            .expect("wallet decrypts its committed note");
        wallet.account = Some(updated);
    }
    Ok((output, events))
}

fn initialize(chain: &mut Chain, wallet: &mut Private, program: AccountId, definition: AccountId) {
    let root = Actor::new(wallet.id(), program);
    if program == TOKEN {
        private_call(
            chain,
            root,
            &[Actor::new(definition, program)],
            &[],
            &mut [(wallet, true)],
            &token_core::Message::InitializeHolding { definition },
            false,
        )
    } else {
        private_call(
            chain,
            root,
            &[Actor::new(definition, program)],
            &[],
            &mut [(wallet, true)],
            &nft_core::Message::InitializeHolding { definition },
            false,
        )
    }
    .expect("private holding initializes");
}

#[test]
fn private_token_credit_requires_promotion_and_keeps_snapshots_private() {
    let mut chain = Chain::new();
    let owner = key(12);
    let definition = chain.define(false, 100);
    let source = chain.create(TOKEN, account(&owner), definition);
    chain.fund(TOKEN, source, 81);
    let mut recipient = Private::new(50);
    initialize(&mut chain, &mut recipient, TOKEN, definition);
    let to = recipient.id();
    let issuer = key(11);
    let funding_root = Actor::new(account(&issuer), TOKEN);
    private_call(
        &mut chain,
        funding_root,
        &[funding_root],
        &[&issuer],
        &mut [(&mut recipient, false)],
        &token_core::Message::Transfer { to, amount: 19 },
        true,
    )
    .expect("prefund private balance");

    let root = Actor::new(account(&owner), ATA);
    let public = [root, Actor::new(source, TOKEN)];
    let before = chain.state.clone();
    let private_before = recipient.account.clone();
    let result = private_call(
        &mut chain,
        root,
        &public,
        &[&owner],
        &mut [(&mut recipient, false)],
        &transfer_message(TOKEN, definition, to, 37),
        false,
    );
    assert_refused(&result, "ATA transfer did not settle atomically");
    assert!(
        chain.state == before,
        "unpromoted credit rolls back every public/private change"
    );
    assert_eq!(recipient.account, private_before);
    assert_eq!(chain.balance(source), 81);
    assert_eq!(recipient.balance(), 19);
    recipient.idle();
    assert_eq!(chain.state.publications_from(0).count(), 0);

    // Zero validates both holdings without an async credit, promotion or transfer event.
    let (_, events) = private_call(
        &mut chain,
        root,
        &public,
        &[&owner],
        &mut [(&mut recipient, false)],
        &transfer_message(TOKEN, definition, to, 0),
        false,
    )
    .expect("zero transfer needs no promotion");
    assert!(events.is_empty());
    assert_eq!(chain.balance(source), 81);
    assert_eq!(recipient.balance(), 19);
    recipient.idle();

    let (output, _) = private_call(
        &mut chain,
        root,
        &public,
        &[&owner],
        &mut [(&mut recipient, false)],
        &transfer_message(TOKEN, definition, to, 37),
        true,
    )
    .expect("promoted credit settles");
    assert_eq!(chain.balance(source), 44);
    assert_eq!(recipient.balance(), 56);
    recipient.idle();
    chain.idle(account(&owner));
    assert_eq!(chain.state.publications_from(0).count(), 0);

    // The only private-to-public messages are source ReadState and Transfer.
    // Neither includes the recipient's before/after balances (19 and 56).
    let outgoing: Vec<_> = output
        .execution
        .boundary
        .iter()
        .filter_map(|step| {
            if let BoundaryStep::PrivateToPublic(delivery) = step {
                Some(&delivery.envelope)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(outgoing.len(), 2);
    for envelope in &outgoing {
        assert_eq!(envelope.from, Actor::new(to, ATA));
        assert_eq!(envelope.to, Actor::new(source, TOKEN));
    }
    let messages: Vec<token_core::Message> = outgoing
        .iter()
        .map(|envelope| {
            borsh::from_slice(&envelope.message)
                .expect("public request, never a private StateReply")
        })
        .collect();
    assert_eq!(
        messages,
        [
            token_core::Message::ReadState,
            token_core::Message::Transfer { to, amount: 37 },
        ]
    );
}

#[test]
fn private_nft_master_and_copy_require_promotion_and_transfer_whole_value() {
    for copy in [false, true] {
        let mut chain = Chain::new();
        let owner = key(12);
        let definition = chain.define(true, 5);
        let source = chain.create(NFT, account(&owner), definition);
        if copy {
            let issuer = key(11);
            let root = Actor::new(account(&issuer), NFT);
            chain
                .call(
                    root,
                    &[root, Actor::new(source, NFT)],
                    &[&issuer],
                    &nft_core::Message::PrintNft { to: source },
                )
                .expect("print a copy through NFT");
        } else {
            chain.fund(NFT, source, 0);
        }
        let value = chain.nft(source);
        let mut recipient = Private::new(51);
        initialize(&mut chain, &mut recipient, NFT, definition);
        let to = recipient.id();
        let root = Actor::new(account(&owner), ATA);
        let public = [root, Actor::new(source, NFT)];
        let before = chain.state.clone();
        let private_before = recipient.account.clone();
        let result = private_call(
            &mut chain,
            root,
            &public,
            &[&owner],
            &mut [(&mut recipient, false)],
            &transfer_message(NFT, definition, to, 0),
            false,
        );
        assert_refused(&result, "ATA transfer did not settle atomically");
        assert!(
            chain.state == before,
            "failed NFT credit changes no ownership"
        );
        assert_eq!(chain.nft(source), value);
        assert_eq!(recipient.account, private_before);
        recipient.idle();
        assert_eq!(chain.state.publications_from(0).count(), 0);

        private_call(
            &mut chain,
            root,
            &public,
            &[&owner],
            &mut [(&mut recipient, false)],
            &transfer_message(NFT, definition, to, 0),
            true,
        )
        .expect("promoted NFT credit settles");
        assert_eq!(recipient.nft(), value);
        assert!(
            matches!(chain.nft(source), NftState::Master(m) if m.print_balance == 0)
                || matches!(chain.nft(source), NftState::Copy(c) if !c.owned)
        );
        recipient.idle();
        chain.idle(account(&owner));
        assert_eq!(chain.state.publications_from(0).count(), 0);
    }
}

#[test]
fn private_and_shared_owners_authorize_token_and_nft_spending() {
    for program in [TOKEN, NFT] {
        let mut chain = Chain::new();
        let definition = chain.define(program == NFT, 100);
        // Shared regular accounts have ordinary private authorization on-chain.
        // Both controllers hold the same spending/viewing material; distributing
        // those keys off-chain belongs to the wallet, not the ATA program.
        let mut first = Private::new(52);
        let mut second = Private::new(52);
        assert_eq!(first.id(), second.id());
        let owner = first.id();
        let source = chain.create(program, owner, definition);
        chain.fund(program, source, 100);
        let recipient = key(13);
        chain.initialize(program, &recipient, definition);
        let to = account(&recipient);
        let root = Actor::new(owner, ATA);
        let public = [Actor::new(source, program), Actor::new(definition, program)];
        let before = chain.state.clone();

        let denied = private_call(
            &mut chain,
            root,
            &public,
            &[],
            &mut [(&mut first, false)],
            &burn_message(program, definition, 7),
            false,
        );
        assert_refused(&denied, "ATA owner must authorize spending");
        assert!(chain.state == before);
        assert!(first.account.is_none());
        private_call(
            &mut chain,
            root,
            &public,
            &[],
            &mut [(&mut first, true)],
            &burn_message(program, definition, 7),
            false,
        )
        .expect("private owner burns through its public ATA");
        first.idle();
        // The other controller scans the same encrypted account note.
        second.account = first.account.clone();

        let public = [
            Actor::new(source, program),
            Actor::new(to, ATA),
            Actor::new(to, program),
        ];
        let denied = private_call(
            &mut chain,
            root,
            &public,
            &[],
            &mut [(&mut second, false)],
            &transfer_message(program, definition, to, 20),
            false,
        );
        assert_refused(&denied, "ATA owner must authorize spending");
        private_call(
            &mut chain,
            root,
            &public,
            &[],
            &mut [(&mut second, true)],
            &transfer_message(program, definition, to, 20),
            false,
        )
        .expect("second controller spends shared ATA");
        second.idle();
        chain.idle(to);
        if program == TOKEN {
            assert_eq!(chain.balance(source), 73);
            assert_eq!(chain.balance(to), 20);
            assert!(
                matches!(chain.token(definition), TokenState::Definition(d) if d.total_supply == 93)
            );
        } else {
            assert!(matches!(chain.nft(source), NftState::Master(m) if m.print_balance == 0));
            assert!(matches!(chain.nft(to), NftState::Master(m) if m.print_balance == 93));
            assert!(
                matches!(chain.nft(definition), NftState::Definition(d) if d.printable_supply == 93)
            );
        }
        assert_eq!(chain.state.publications_from(0).count(), 0);
    }
}

#[test]
fn private_owner_to_private_recipient_settles_with_both_witnesses() {
    for program in [TOKEN, NFT] {
        let mut chain = Chain::new();
        let definition = chain.define(program == NFT, 100);
        let mut owner = Private::new(54);
        let mut recipient = Private::new(55);
        let source = chain.create(program, owner.id(), definition);
        chain.fund(program, source, 100);
        initialize(&mut chain, &mut recipient, program, definition);
        let root = Actor::new(owner.id(), ATA);
        let to = recipient.id();
        private_call(
            &mut chain,
            root,
            &[Actor::new(source, program)],
            &[],
            &mut [(&mut owner, true), (&mut recipient, false)],
            &transfer_message(program, definition, to, 25),
            true,
        )
        .expect("private owner and recipient settle atomically");
        owner.idle();
        recipient.idle();
        if program == TOKEN {
            assert_eq!(chain.balance(source), 75);
            assert_eq!(recipient.balance(), 25);
        } else {
            assert!(matches!(chain.nft(source), NftState::Master(m) if m.print_balance == 0));
            assert!(matches!(recipient.nft(), NftState::Master(m) if m.print_balance == 100));
        }
        assert_eq!(chain.state.publications_from(0).count(), 0);
    }
}
