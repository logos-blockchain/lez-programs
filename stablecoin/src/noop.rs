use nssa_core::{account::AccountWithMetadata, program::AccountPostState};

/// Pass `account` through unchanged as a single post-state entry.
pub fn noop(account: AccountWithMetadata) -> Vec<AccountPostState> {
    vec![AccountPostState::new(account.account)]
}
