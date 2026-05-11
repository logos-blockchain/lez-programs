use nssa_core::{account::AccountWithMetadata, program::AccountPostState};

pub fn noop(account: AccountWithMetadata) -> Vec<AccountPostState> {
    vec![AccountPostState::new(account.account)]
}
