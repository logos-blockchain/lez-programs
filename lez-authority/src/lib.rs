//! Agnostic mint authority library for LEZ programs.
//! Implements the approval model defined in RFP-001.
//! No dependency on any specific program or nssa_core.

use borsh::{BorshDeserialize, BorshSerialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityError {
    Revoked,
    Unauthorized,
    AlreadyRevoked,
}

impl core::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Revoked => write!(f, "mint authority has been revoked; supply is fixed"),
            Self::Unauthorized => write!(f, "signer is not the current mint authority"),
            Self::AlreadyRevoked => write!(f, "authority already revoked; cannot set again"),
        }
    }
}

/// A mint authority slot. None = permanently fixed supply.
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct AuthoritySlot(pub Option<[u8; 32]>);

impl AuthoritySlot {
    pub fn new(authority: [u8; 32]) -> Self {
        Self(Some(authority))
    }

    pub fn fixed() -> Self {
        Self(None)
    }

    pub fn check(&self, signer: [u8; 32]) -> Result<(), AuthorityError> {
        match self.0 {
            None => Err(AuthorityError::Revoked),
            Some(auth) if auth != signer => Err(AuthorityError::Unauthorized),
            Some(_) => Ok(()),
        }
    }

    /// Rotate or revoke. Only mutates AFTER all checks pass.
    pub fn set(
        &mut self,
        signer: [u8; 32],
        new_authority: Option<[u8; 32]>,
    ) -> Result<(), AuthorityError> {
        match self.0 {
            None => Err(AuthorityError::AlreadyRevoked),
            Some(auth) if auth != signer => Err(AuthorityError::Unauthorized),
            Some(_) => {
                self.0 = new_authority;
                Ok(())
            }
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.0.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: [u8; 32] = [1u8; 32];
    const BOB: [u8; 32] = [2u8; 32];

    #[test]
    fn check_succeeds_for_correct_signer() {
        assert!(AuthoritySlot::new(ALICE).check(ALICE).is_ok());
    }

    #[test]
    fn check_fails_unauthorized() {
        assert_eq!(
            AuthoritySlot::new(ALICE).check(BOB),
            Err(AuthorityError::Unauthorized)
        );
    }

    #[test]
    fn check_fails_when_revoked() {
        assert_eq!(
            AuthoritySlot::fixed().check(ALICE),
            Err(AuthorityError::Revoked)
        );
    }

    #[test]
    fn set_rotates_authority() {
        let mut slot = AuthoritySlot::new(ALICE);
        slot.set(ALICE, Some(BOB)).unwrap();
        assert_eq!(slot.0, Some(BOB));
        assert_eq!(slot.check(ALICE), Err(AuthorityError::Unauthorized));
    }

    #[test]
    fn set_revokes_permanently() {
        let mut slot = AuthoritySlot::new(ALICE);
        slot.set(ALICE, None).unwrap();
        assert!(slot.is_revoked());
        assert_eq!(
            slot.set(ALICE, Some(ALICE)),
            Err(AuthorityError::AlreadyRevoked)
        );
    }

    #[test]
    fn wrong_authority_cannot_rotate_and_state_unchanged() {
        let mut slot = AuthoritySlot::new(ALICE);
        assert_eq!(slot.set(BOB, Some(BOB)), Err(AuthorityError::Unauthorized));
        assert_eq!(slot.0, Some(ALICE)); // state unchanged
    }

    #[test]
    fn set_none_on_already_fixed_fails() {
        let mut slot = AuthoritySlot::fixed();
        assert_eq!(slot.set(ALICE, None), Err(AuthorityError::AlreadyRevoked));
    }
}
