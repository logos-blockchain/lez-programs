//! Shared account/key setup helpers for privacy-preserving integration tests.

use key_protocol::key_management::{
    group_key_holder::{GroupKeyHolder, SealingPublicKey},
    secret_holders::SecretSpendingKey,
};
use lee_core::{
    account::AccountId, encryption::ViewingPublicKey, AuthorizationSecretKey, CommitmentSetDigest,
    InputAccountIdentity, MembershipProof, NullifierPublicKey, NullifierSecretKey,
    NullifierWitness, PrivateWitness, WitnessKind,
};

/// Builds a foreign-init identity: a third party credits a fresh private account it does
/// not control, using only the owner's public key material (`npk`/`vpk`), no credential.
///
/// LEZ v0.2.5 collapsed the three `InputAccountIdentity::Private*` variants into a single
/// `Private(PrivateWitness)` carrying a `kind` and a `nullifier`; this shape is the former
/// `PrivateForeignInit`. The circuit now asserts `pre_state.is_authorized == ask.is_some()`,
/// so a pre-state paired with this identity must carry `is_authorized = false` — the
/// opposite of what logos-execution-zone PR #621 required under v0.2.4.
pub fn private_foreign_init_identity(
    npk: NullifierPublicKey,
    vpk: &ViewingPublicKey,
    commitment_root: CommitmentSetDigest,
) -> InputAccountIdentity {
    InputAccountIdentity::Private(PrivateWitness {
        vpk: vpk.clone(),
        random_seed: [0; 32],
        identifier: 0,
        kind: WitnessKind::Regular { ask: None },
        nullifier: NullifierWitness::Init {
            npk,
            commitment_root,
        },
    })
}

/// Builds an authorized-init identity: the owner self-initializes a fresh private account by
/// supplying its own authorization key (`is_authorized` must be `true`).
///
/// Takes an `ask` rather than v0.2.4's `nsk`: the credential the circuit checks is the
/// authorization key, and it derives the `npk` from it to bind the account id.
pub fn private_authorized_init_identity(
    ask: AuthorizationSecretKey,
    vpk: &ViewingPublicKey,
    commitment_root: CommitmentSetDigest,
) -> InputAccountIdentity {
    let npk = NullifierPublicKey::from(&NullifierSecretKey::from(&ask));
    InputAccountIdentity::Private(PrivateWitness {
        vpk: vpk.clone(),
        random_seed: [0; 32],
        identifier: 0,
        kind: WitnessKind::Regular { ask: Some(ask) },
        nullifier: NullifierWitness::Init {
            npk,
            commitment_root,
        },
    })
}

/// Builds an authorized-update identity: spends/credits an *existing* private account,
/// requiring its authorization key and a membership proof of its current committed state.
///
/// Takes an `ask` rather than v0.2.4's `nsk`; the `nsk` the nullifier witness needs is
/// derived from it, and the circuit cross-checks the two agree.
pub fn private_authorized_update_identity(
    ask: AuthorizationSecretKey,
    vpk: &ViewingPublicKey,
    membership_proof: MembershipProof,
) -> InputAccountIdentity {
    let nsk = NullifierSecretKey::from(&ask);
    InputAccountIdentity::Private(PrivateWitness {
        vpk: vpk.clone(),
        random_seed: [0; 32],
        identifier: 0,
        kind: WitnessKind::Regular { ask: Some(ask) },
        nullifier: NullifierWitness::Update {
            view_tag: 0,
            nsk,
            membership_proof,
        },
    })
}

/// "Alice": creates a shared private account's `GroupKeyHolder` (Group Master Secret) and
/// derives its public identity. The GMS itself never leaves this struct — other parties only
/// ever receive it through [`GroupOwner::admit_member`]'s real seal/unseal ML-KEM-768 handshake,
/// never by handing over key material directly.
pub struct GroupOwner {
    holder: GroupKeyHolder,
    derivation_seed: [u8; 32],
    pub npk: NullifierPublicKey,
    pub vpk: ViewingPublicKey,
    pub id: AccountId,
}

impl GroupOwner {
    /// Creates the group and derives the shared account's public identity from
    /// `derivation_seed`.
    #[must_use]
    pub fn new(derivation_seed: [u8; 32]) -> Self {
        let holder = GroupKeyHolder::new();
        let keys = holder.derive_keys_for_shared_account(&derivation_seed);
        let npk = keys.generate_nullifier_public_key();
        let vpk = keys.generate_viewing_public_key();
        let id = AccountId::for_regular_private_account(&npk, &vpk, 0);
        Self {
            holder,
            derivation_seed,
            npk,
            vpk,
            id,
        }
    }

    /// "Bob": distributes the GMS to a new member via the real seal/unseal handshake and
    /// returns that member's independently re-derived authorization key — the member never
    /// touches this `GroupOwner`'s `GroupKeyHolder`, only the sealed bytes.
    ///
    /// Returns the `ask` rather than v0.2.4's `nsk`: that is what the identity helpers take
    /// now, and the `nsk` derives from it.
    #[must_use]
    pub fn admit_member(&self) -> AuthorizationSecretKey {
        let member_sealing_keys = SecretSpendingKey([9_u8; 32]).produce_private_key_holder(None);
        let member_sealing_vpk = member_sealing_keys.generate_viewing_public_key();
        let member_sealing_vsk = member_sealing_keys.viewing_secret_key;
        let sealed_gms = self.holder.seal_for(&SealingPublicKey::from_bytes(
            member_sealing_vpk.to_bytes().to_vec(),
        ));
        let member_holder = GroupKeyHolder::unseal(&sealed_gms, &member_sealing_vsk)
            .expect("member must unseal the GMS");

        let member_keys = member_holder.derive_keys_for_shared_account(&self.derivation_seed);
        assert_eq!(
            member_keys.generate_nullifier_public_key(),
            self.npk,
            "member must derive the identical npk as the group owner from the shared GMS"
        );
        member_keys.authorization_secret_key
    }
}
