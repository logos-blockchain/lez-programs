//! Agnostic admin/mint authority library for LEZ programs.
//! Implements the approval model defined in RFP-001.
//! No dependency on any specific program or nssa_core.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityError {
    /// The authority slot is empty (renounced); the resource is permanently fixed.
    Revoked,
    /// The signer does not match the current authority.
    Unauthorized,
    /// Attempted to act on an already-renounced authority.
    AlreadyRevoked,
}

impl core::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Revoked => write!(f, "authority has been revoked; resource is fixed"),
            Self::Unauthorized => write!(f, "signer is not the current authority"),
            Self::AlreadyRevoked => write!(f, "authority already revoked; cannot set again"),
        }
    }
}

/// An ownership/authority slot. `None` = permanently renounced (no further changes
/// or privileged actions are possible).
#[derive(
    BorshSerialize, BorshDeserialize, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
pub struct Authority(Option<[u8; 32]>);

impl Authority {
    /// Create an authority owned by `owner`.
    #[must_use]
    pub fn new(owner: [u8; 32]) -> Self {
        Self(Some(owner))
    }

    /// Create a permanently renounced authority (fixed resource).
    #[must_use]
    pub fn renounced() -> Self {
        Self(None)
    }

    /// The current authority key, or `None` if renounced.
    #[must_use]
    pub fn authority(&self) -> Option<[u8; 32]> {
        self.0
    }

    /// Returns `true` if the authority has been permanently renounced.
    #[must_use]
    pub fn is_renounced(&self) -> bool {
        self.0.is_none()
    }

    /// Require that `signer` is the current authority.
    pub fn require(&self, signer: [u8; 32]) -> Result<(), AuthorityError> {
        match self.0 {
            None => Err(AuthorityError::Revoked),
            Some(auth) if auth != signer => Err(AuthorityError::Unauthorized),
            Some(_) => Ok(()),
        }
    }

    /// Rotate to a new authority, or renounce with `None`.
    /// Only mutates AFTER all checks pass (atomic).
    pub fn rotate(
        &mut self,
        signer: [u8; 32],
        new: Option<[u8; 32]>,
    ) -> Result<(), AuthorityError> {
        match self.0 {
            None => Err(AuthorityError::AlreadyRevoked),
            Some(auth) if auth != signer => Err(AuthorityError::Unauthorized),
            Some(_) => {
                self.0 = new;
                Ok(())
            }
        }
    }
}

/// A type that carries an [`Authority`] slot and can be guarded by it.
///
/// Programs "inherit the owner slot" by embedding an [`Authority`] field in their
/// account type and implementing this trait; the default methods then provide the
/// standard require / transfer / renounce semantics.
pub trait Ownable {
    fn authority(&self) -> &Authority;
    fn authority_mut(&mut self) -> &mut Authority;

    /// Require that `signer` is the current owner.
    fn require_owner(&self, signer: [u8; 32]) -> Result<(), AuthorityError> {
        self.authority().require(signer)
    }

    /// Transfer ownership to `new`, authorized by the current owner `signer`.
    fn transfer_ownership(
        &mut self,
        signer: [u8; 32],
        new: [u8; 32],
    ) -> Result<(), AuthorityError> {
        self.authority_mut().rotate(signer, Some(new))
    }

    /// Permanently renounce ownership, authorized by the current owner `signer`.
    fn renounce_ownership(&mut self, signer: [u8; 32]) -> Result<(), AuthorityError> {
        self.authority_mut().rotate(signer, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: [u8; 32] = [1u8; 32];
    const BOB: [u8; 32] = [2u8; 32];

    #[test]
    fn require_succeeds_for_correct_owner() {
        assert!(Authority::new(ALICE).require(ALICE).is_ok());
    }

    #[test]
    fn require_fails_unauthorized() {
        assert_eq!(
            Authority::new(ALICE).require(BOB),
            Err(AuthorityError::Unauthorized)
        );
    }

    #[test]
    fn require_fails_when_renounced() {
        assert_eq!(
            Authority::renounced().require(ALICE),
            Err(AuthorityError::Revoked)
        );
    }

    #[test]
    fn rotate_transfers_authority() {
        let mut auth = Authority::new(ALICE);
        auth.rotate(ALICE, Some(BOB)).unwrap();
        assert_eq!(auth.authority(), Some(BOB));
        assert_eq!(auth.require(ALICE), Err(AuthorityError::Unauthorized));
    }

    #[test]
    fn rotate_renounces_permanently() {
        let mut auth = Authority::new(ALICE);
        auth.rotate(ALICE, None).unwrap();
        assert!(auth.is_renounced());
        assert_eq!(
            auth.rotate(ALICE, Some(ALICE)),
            Err(AuthorityError::AlreadyRevoked)
        );
    }

    #[test]
    fn wrong_owner_cannot_rotate_and_state_unchanged() {
        let mut auth = Authority::new(ALICE);
        assert_eq!(
            auth.rotate(BOB, Some(BOB)),
            Err(AuthorityError::Unauthorized)
        );
        assert_eq!(auth.authority(), Some(ALICE));
    }

    #[test]
    fn renounce_on_already_renounced_fails() {
        let mut auth = Authority::renounced();
        assert_eq!(
            auth.rotate(ALICE, None),
            Err(AuthorityError::AlreadyRevoked)
        );
    }

    // Ownable trait via a tiny embedding type.
    struct Resource {
        owner: Authority,
    }
    impl Ownable for Resource {
        fn authority(&self) -> &Authority {
            &self.owner
        }

        fn authority_mut(&mut self) -> &mut Authority {
            &mut self.owner
        }
    }

    #[test]
    fn ownable_require_transfer_renounce() {
        let mut r = Resource {
            owner: Authority::new(ALICE),
        };
        assert!(r.require_owner(ALICE).is_ok());
        assert_eq!(r.require_owner(BOB), Err(AuthorityError::Unauthorized));

        r.transfer_ownership(ALICE, BOB).unwrap();
        assert!(r.require_owner(BOB).is_ok());

        r.renounce_ownership(BOB).unwrap();
        assert!(r.authority().is_renounced());
    }
}
