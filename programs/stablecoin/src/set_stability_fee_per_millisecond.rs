use nssa_core::{
    account::{AccountWithMetadata, Data},
    program::{AccountPostState, ChainedCall, ProgramId},
};
use stablecoin_core::assert_valid_stability_fee_per_millisecond;

use crate::shared::{
    accrue_stability_fee_state, read_clock_timestamp, read_protocol_parameters,
    read_stability_fee_accumulator,
};

/// Updates the protocol stability-fee rate after accruing at the old rate.
///
/// # Panics
/// Panics if `admin` is not the configured admin or `new_rate` is out of bounds.
pub fn set_stability_fee_per_millisecond(
    admin: AccountWithMetadata,
    protocol_parameters: AccountWithMetadata,
    stability_fee_accumulator: AccountWithMetadata,
    clock: AccountWithMetadata,
    stablecoin_program_id: ProgramId,
    new_rate: u128,
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    assert!(admin.is_authorized, "Admin authorization is missing");
    assert_valid_stability_fee_per_millisecond(new_rate);

    let mut params = read_protocol_parameters(&protocol_parameters, stablecoin_program_id);
    assert_eq!(
        admin.account_id, params.admin_account_id,
        "Admin account does not match protocol admin"
    );

    let accumulator =
        read_stability_fee_accumulator(&stability_fee_accumulator, stablecoin_program_id);
    let now = read_clock_timestamp(&clock);
    let updated_accumulator = accrue_stability_fee_state(&accumulator, &params, now);
    params.stability_fee_per_millisecond = new_rate;

    let mut protocol_post = protocol_parameters.account.clone();
    protocol_post.data = Data::from(&params);
    let mut accumulator_post = stability_fee_accumulator.account.clone();
    accumulator_post.data = Data::from(&updated_accumulator);

    let post_states = vec![
        AccountPostState::new(admin.account),
        AccountPostState::new(protocol_post),
        AccountPostState::new(accumulator_post),
        AccountPostState::new(clock.account),
    ];

    (post_states, vec![])
}
