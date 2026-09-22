//! The TWAP Oracle Program implementation.

pub use twap_oracle_core as core;

pub mod create_current_tick_account;
pub mod create_oracle_price_account;
pub mod create_price_observations;
pub mod publish_price;
pub mod record_tick;
pub mod update_current_tick;

/// Test-only view of what an [`AccountStateDiff`] leaves behind.
///
/// Since LEZ v0.2.5 a diff carries a *change* rather than a whole post-state account:
/// `post_data` is `None` when the data is untouched, and ownership is not in the diff at
/// all — the runtime derives it (see `acquire_ownership_on_data_write`). These mirror that
/// derivation so assertions can stay written in terms of the resulting account.
/// `pub` only so an accessor no single program happens to use is not dead code; the whole
/// trait is `#[cfg(test)]`, so it never exists in a real build.
#[cfg(test)]
pub trait StateDiffExt {
    /// The data the account ends up with.
    fn post_data(&self) -> &lee_core::account::Data;

    /// The owner the account ends up with: writing data to a default-owned account makes
    /// `executing_account_id` its owner.
    fn post_owner(
        &self,
        executing_account_id: lee_core::account::AccountId,
    ) -> lee_core::account::AccountId;

    /// Whether this diff changes the account's data at all.
    fn writes_data(&self) -> bool;

    /// The whole account the diff leaves behind, via the runtime's own `post_state`.
    fn post_account(
        &self,
        executing_account_id: lee_core::account::AccountId,
    ) -> lee_core::account::Account;
}

#[cfg(test)]
impl StateDiffExt for lee_core::program::AccountStateDiff {
    fn post_data(&self) -> &lee_core::account::Data {
        self.post_data
            .as_ref()
            .unwrap_or(&self.pre_state.account.data)
    }

    fn post_owner(
        &self,
        executing_account_id: lee_core::account::AccountId,
    ) -> lee_core::account::AccountId {
        let pre = &self.pre_state.account;
        if pre.program_owner == lee_core::program::DEFAULT_PROGRAM_OWNER && self.writes_data() {
            executing_account_id
        } else {
            pre.program_owner
        }
    }

    fn writes_data(&self) -> bool {
        self.post_data
            .as_ref()
            .is_some_and(|data| *data != self.pre_state.account.data)
    }

    fn post_account(
        &self,
        executing_account_id: lee_core::account::AccountId,
    ) -> lee_core::account::Account {
        lee_core::program::post_state(self, executing_account_id)
            .expect("balance diff must apply to the pre-state balance")
    }
}
