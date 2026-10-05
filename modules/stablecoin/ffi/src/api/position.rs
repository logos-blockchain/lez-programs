use lee_core::{account::AccountId, program::ProgramId};
use stablecoin_core::{compute_position_pda, Position};

use super::StablecoinApiError;
use crate::{account::decode_account, AccountRead};

pub(super) fn validated_position(
    program_id: ProgramId,
    owner: AccountId,
    nonce: u64,
    read: &AccountRead,
) -> Result<(AccountId, Position), StablecoinApiError> {
    let (position_id, account) =
        decode_account(read).map_err(|_| StablecoinApiError::new("account_read_failed"))?;
    if position_id != compute_position_pda(program_id, owner, nonce) {
        return Err(StablecoinApiError::new("position_pda_mismatch"));
    }
    if account.program_owner != program_id {
        return Err(StablecoinApiError::new("stablecoin_program_mismatch"));
    }
    let position = Position::try_from(&account.data)
        .map_err(|_| StablecoinApiError::new("invalid_position_data"))?;
    if position.owner_account_id != owner {
        return Err(StablecoinApiError::new("position_owner_mismatch"));
    }
    if position.position_nonce != nonce {
        return Err(StablecoinApiError::new("position_nonce_mismatch"));
    }
    Ok((position_id, position))
}
