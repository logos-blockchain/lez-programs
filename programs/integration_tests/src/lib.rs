//! Shared account/key setup helpers for privacy-preserving integration tests.

use key_protocol::key_management::{
    group_key_holder::{GroupKeyHolder, SealingPublicKey},
    secret_holders::SecretSpendingKey,
};
use nssa::SharedSecretKey;
use nssa_core::{
    account::AccountId,
    encryption::{EphemeralPublicKey, ViewingPublicKey},
    EncryptedAccountData, InputAccountIdentity, MembershipProof, NullifierPublicKey,
    NullifierSecretKey,
};

/// Builds a `PrivateUnauthorized` identity: a third party credits a fresh private account it
/// does not control (no `nsk`, `is_authorized` must be `false` on the paired pre-state).
pub fn private_unauthorized_identity(
    npk: NullifierPublicKey,
    vpk: &ViewingPublicKey,
    output_index: u32,
) -> InputAccountIdentity {
    InputAccountIdentity::PrivateUnauthorized {
        epk: EphemeralPublicKey(Vec::new()),
        view_tag: EncryptedAccountData::compute_view_tag(&npk, vpk),
        npk,
        ssk: SharedSecretKey::encapsulate_deterministic(vpk, &[0u8; 32], output_index).0,
        identifier: 0,
    }
}

/// Builds a `PrivateAuthorizedInit` identity: the owner self-initializes a fresh private
/// account by supplying its own `nsk` directly (`is_authorized` must be `true`).
pub fn private_authorized_init_identity(
    nsk: NullifierSecretKey,
    vpk: &ViewingPublicKey,
    output_index: u32,
) -> InputAccountIdentity {
    let npk = NullifierPublicKey::from(&nsk);
    InputAccountIdentity::PrivateAuthorizedInit {
        epk: EphemeralPublicKey(Vec::new()),
        view_tag: EncryptedAccountData::compute_view_tag(&npk, vpk),
        ssk: SharedSecretKey::encapsulate_deterministic(vpk, &[0u8; 32], output_index).0,
        nsk,
        identifier: 0,
    }
}

/// Builds a `PrivateAuthorizedUpdate` identity: spends/credits an *existing* private account,
/// requiring its own `nsk` and a membership proof of its current committed state.
pub fn private_authorized_update_identity(
    nsk: NullifierSecretKey,
    vpk: &ViewingPublicKey,
    membership_proof: MembershipProof,
    output_index: u32,
) -> InputAccountIdentity {
    let npk = NullifierPublicKey::from(&nsk);
    InputAccountIdentity::PrivateAuthorizedUpdate {
        epk: EphemeralPublicKey(Vec::new()),
        view_tag: EncryptedAccountData::compute_view_tag(&npk, vpk),
        ssk: SharedSecretKey::encapsulate_deterministic(vpk, &[0u8; 32], output_index).0,
        nsk,
        membership_proof,
        identifier: 0,
    }
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
        let id = AccountId::for_regular_private_account(&npk, 0);
        Self {
            holder,
            derivation_seed,
            npk,
            vpk,
            id,
        }
    }

    /// "Bob": distributes the GMS to a new member via the real seal/unseal handshake and
    /// returns that member's independently re-derived secret key — the member never touches
    /// this `GroupOwner`'s `GroupKeyHolder`, only the sealed bytes.
    #[must_use]
    pub fn admit_member(&self) -> NullifierSecretKey {
        let member_sealing_keys = SecretSpendingKey([9_u8; 32]).produce_private_key_holder(None);
        let member_sealing_vpk = member_sealing_keys.generate_viewing_public_key();
        let member_sealing_vsk = member_sealing_keys.viewing_secret_key;
        let sealed_gms = self.holder.seal_for(&SealingPublicKey::from_bytes(
            member_sealing_vpk.to_bytes().to_vec(),
        ));
        let member_holder = GroupKeyHolder::unseal(&sealed_gms, &member_sealing_vsk)
            .expect("member must unseal the GMS");

        let member_keys = member_holder.derive_keys_for_shared_account(&self.derivation_seed);
        let member_nsk = member_keys.nullifier_secret_key;
        assert_eq!(
            member_keys.generate_nullifier_public_key(),
            self.npk,
            "member must derive the identical npk as the group owner from the shared GMS"
        );
        member_nsk
    }
}
