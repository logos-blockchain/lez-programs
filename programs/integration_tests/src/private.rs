//! Privacy-preserving transactions: private accounts known by their keys, proven with LEZ's
//! circuit (a fake receipt under `RISC0_DEV_MODE`) and settled on the `V03State`.
//!
//! The state holds only commitments to private accounts, so a [`PrivateAccount`] keeps its own
//! copy, and each test states what the account holds after a transaction
//! ([`PrivateAccount::settle`]), checked against the commitment the transaction added.

use std::{borrow::Cow, collections::BTreeSet};

use lee::{
    error::LeeError,
    execute_and_prove,
    privacy_preserving_transaction::{
        circuit::ProgramCatalog, message::Message, witness_set::WitnessSet,
    },
    program::Program,
    test_utils::TestPrivateKeys,
    DummyOutput, MessageWitness, PrivacyPreservingTransaction, PrivateKey, ProvingInput,
    PublicExecutionContext, Publication, RecipientEncryption, RootCall, SenderPresentation,
    Simulation, TransactionEntry, V03State,
};
use lee_core::{
    account::{Account, AccountId, Actor},
    AuthorizationSecretKey, Commitment, EphemeralSecretKey, MembershipProof, NullifierWitness,
    PrivateAccountKind, PrivateWitness, Recipient, RegularKey, WitnessKind, DUMMY_COMMITMENT_HASH,
};

use crate::account;

/// A private account: its keys and the account the chain committed to last, if any.
pub struct PrivateAccount {
    keys: TestPrivateKeys,
    account: Option<Account>,
}

impl PrivateAccount {
    /// A fresh private account; `byte` picks its keys.
    #[must_use]
    pub fn new(byte: u8) -> Self {
        Self {
            keys: TestPrivateKeys {
                ask: AuthorizationSecretKey([byte; 32]),
                d: [byte.wrapping_add(1); 32],
                z: [byte.wrapping_add(2); 32],
            },
            account: None,
        }
    }

    #[must_use]
    pub fn id(&self) -> AccountId {
        self.keys.account_id()
    }

    /// The account's state under `program`, empty before the account exists.
    #[must_use]
    pub fn actor_state(&self, program: AccountId) -> Vec<u8> {
        self.account
            .as_ref()
            .map(|account| account.data.actor_state(program).as_ref().to_vec())
            .unwrap_or_default()
    }

    /// Records that the last transaction left `actor_state` on this account under `program`.
    ///
    /// # Panics
    /// If the state holds no commitment to that account: the transaction wrote something else.
    pub fn settle(&mut self, state: &V03State, program: AccountId, actor_state: Vec<u8>) {
        let previous = self.account.clone().unwrap_or_default();
        let account = Account {
            nonce: previous
                .nonce
                .private_account_nonce_increment(&self.keys.nsk()),
            data: previous.data.with_actor_state(program, actor_state.into()),
        };
        assert!(
            self.membership_proof(state, &account).is_some(),
            "the transaction committed another state for private account {}",
            self.id()
        );
        self.account = Some(account);
    }

    /// The seal a credit to this account travels under.
    #[must_use]
    pub fn seal(&self) -> RecipientEncryption {
        RecipientEncryption {
            recipient: Recipient {
                npk: self.keys.npk(),
                vpk: self.keys.vpk(),
                kind: PrivateAccountKind::Regular,
                opening: None,
            },
            esk: EphemeralSecretKey([7; 32]),
        }
    }

    /// The witness proving this account in a transaction: with its authorization key when
    /// `authorized`, else with its nullifier key only, so its actors can be credited but not
    /// debited.
    fn witness(&self, state: &V03State, authorized: bool) -> PrivateWitness {
        let kind = if authorized {
            RegularKey::Authorized(self.keys.ask)
        } else {
            RegularKey::Nullifying(self.keys.nsk())
        };
        let nullifier = match &self.account {
            None => NullifierWitness::Init {
                commitment_root: DUMMY_COMMITMENT_HASH,
            },
            Some(account) => NullifierWitness::Update {
                account: account.clone(),
                membership_proof: self
                    .membership_proof(state, account)
                    .expect("a private account's last state is committed"),
            },
        };
        PrivateWitness {
            vpk: self.keys.vpk(),
            random_seed: [0; 32],
            kind: WitnessKind::Regular(kind),
            nullifier,
            address_openings: BTreeSet::new(),
        }
    }

    fn membership_proof(&self, state: &V03State, account: &Account) -> Option<MembershipProof> {
        state.get_proof_for_commitment(&Commitment::new(&self.id(), account))
    }
}

/// A privacy-preserving transaction, built up and then sent with [`PrivateCall::submit`].
pub struct PrivateCall<'a> {
    root: TransactionEntry<MessageWitness>,
    private: Vec<(&'a PrivateAccount, bool)>,
    public: Vec<Actor>,
    signers: Vec<&'a PrivateKey>,
    seals: Vec<RecipientEncryption>,
    recoveries: Vec<RecipientEncryption>,
    presentation: SenderPresentation,
}

impl<'a> PrivateCall<'a> {
    /// A call sending `message` to `root`.
    #[must_use]
    pub fn to(root: Actor, message: &impl borsh::BorshSerialize) -> Self {
        Self::with_root(TransactionEntry::Sync(RootCall {
            to: root,
            message: borsh::to_vec(message).expect("borsh serialization is infallible"),
        }))
    }

    fn with_root(root: TransactionEntry<MessageWitness>) -> Self {
        Self {
            root,
            private: Vec::new(),
            public: Vec::new(),
            signers: Vec::new(),
            seals: Vec::new(),
            recoveries: Vec::new(),
            presentation: SenderPresentation::Canonical,
        }
    }

    /// Takes part with its authorization key, the private counterpart of a signature.
    #[must_use]
    pub fn authorized(mut self, account: &'a PrivateAccount) -> Self {
        self.private.push((account, true));
        self
    }

    /// Takes part with its nullifier key only: it can be credited, not debited.
    #[must_use]
    pub fn witnessed(mut self, account: &'a PrivateAccount) -> Self {
        self.private.push((account, false));
        self
    }

    /// Runs `actor` publicly.
    #[must_use]
    pub fn public(mut self, actor: Actor) -> Self {
        self.public.push(actor);
        self
    }

    /// Signs with `key`, authorizing its public account.
    #[must_use]
    pub fn signer(mut self, key: &'a PrivateKey) -> Self {
        self.signers.push(key);
        self
    }

    /// Seals private execution's credit to `account`.
    #[must_use]
    pub fn seal(mut self, account: &PrivateAccount) -> Self {
        self.seals.push(account.seal());
        self
    }

    /// Publishes public execution's credit to `account` with a recovery binding, introduced
    /// here unless the chain already has one for its address.
    #[must_use]
    pub fn recover(mut self, account: &PrivateAccount) -> Self {
        self.recoveries.push(account.seal());
        self
    }

    /// Shows private senders to public actors under a blinded alias instead of their id.
    #[must_use]
    pub fn blinded(mut self) -> Self {
        self.presentation = SenderPresentation::Blinded([9; 32]);
        self
    }

    /// Proves the call against `state` and settles it there.
    ///
    /// # Errors
    /// The proof's error if an actor refuses, else settlement's.
    pub fn submit(
        self,
        state: &mut V03State,
        programs: &[(AccountId, &'static [u8])],
    ) -> Result<(), LeeError> {
        let Self {
            root,
            private,
            public,
            signers,
            seals,
            recoveries,
            presentation,
        } = self;
        let catalog: ProgramCatalog = programs
            .iter()
            .map(|(id, elf)| {
                (
                    *id,
                    Program::new(Cow::Borrowed(*elf)).expect("the guest loads"),
                )
            })
            .collect();
        let simulation = Simulation {
            public_actor_states: public
                .iter()
                .map(|actor| {
                    let account = state.get_account_by_id(actor.account_id);
                    (
                        *actor,
                        account.data.actor_state(actor.program_account_id).clone(),
                    )
                })
                .collect(),
            admitted_accounts: None,
        };
        let signer_ids: Vec<AccountId> = signers.iter().map(|key| account(key)).collect();
        let input = ProvingInput {
            claims: Vec::new(),
            root,
            public_context: PublicExecutionContext::new(public, signer_ids.iter().copied()),
            private_witnesses: private
                .iter()
                .map(|(account, authorized)| account.witness(state, *authorized))
                .collect(),
            dummy_inputs: Vec::new(),
            ciphertext_padding: None,
            recoveries: recoveries
                .into_iter()
                .filter(|seal| state.recovery_binding(seal.recipient.address()).is_none())
                .collect(),
            private_promotions: BTreeSet::new(),
            fee: None,
        };
        let (output, proof) = execute_and_prove(
            input,
            &simulation,
            &catalog,
            |_| presentation,
            |_, _| false,
            |body| {
                seals
                    .iter()
                    .find(|seal| seal.recipient.address() == body.to.account_id)
                    .cloned()
                    .ok_or_else(|| LeeError::InvalidInput(format!("no seal for {:?}", body.to)))
            },
        )?;
        let nonces = signer_ids
            .iter()
            .map(|id| (*id, state.get_account_by_id(*id).nonce))
            .collect();
        let message = Message::from_circuit_output(nonces, output);
        let witness_set = WitnessSet::for_message(&message, proof, &signers);
        state.transition_from_privacy_preserving_transaction(
            &PrivacyPreservingTransaction::new(message, witness_set),
            1,
            0,
        )
    }
}

/// An async message waiting in the state's publication log for a private recipient.
pub struct Pending {
    position: u64,
    publication: Publication,
}

impl Pending {
    /// The publications from `position` on, oldest first.
    #[must_use]
    pub fn from(state: &V03State, position: u64) -> Vec<Self> {
        state
            .publications_from(position)
            .map(|(position, publication)| Self {
                position,
                publication: publication.clone(),
            })
            .collect()
    }

    /// Whether `account` can read it: a sealed message only its recipient opens, a clear one
    /// anyone reads.
    #[must_use]
    pub fn is_for(&self, account: &PrivateAccount) -> bool {
        self.open(account)
            .is_some_and(|(body, _)| body.to.account_id == account.id())
    }

    /// The transaction applying it to `account`, which takes part with its nullifier key, as
    /// `wallet pending receive` does.
    ///
    /// # Panics
    /// If `account` can't open it.
    #[must_use]
    pub fn receive<'a>(&self, state: &V03State, account: &'a PrivateAccount) -> PrivateCall<'a> {
        let (body, rho) = self.open(account).expect("the recipient opens its message");
        PrivateCall::with_root(TransactionEntry::Async(MessageWitness {
            body,
            rho,
            membership_proof: state
                .get_proof_for_position(self.position)
                .expect("a published message has a membership path"),
            filler: DummyOutput::default(),
        }))
        .witnessed(account)
    }

    fn open(&self, account: &PrivateAccount) -> Option<(lee::MessageBody, Option<[u8; 32]>)> {
        match &self.publication {
            Publication::Clear { body, .. } => Some((body.clone(), None)),
            Publication::Sealed(sealed) => sealed
                .open(account.keys.npk(), &account.keys.d, &account.keys.z)
                .map(|(body, _, rho)| (body, Some(rho))),
        }
    }
}
