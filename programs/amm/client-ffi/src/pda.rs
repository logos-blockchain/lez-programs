//! PDA derivation and program-id helpers, delegating to `amm_core` /
//! `twap_oracle_core` so the client never re-implements seed hashing.

use amm_core::{compute_config_pda, compute_pool_pda, compute_vault_pda};
use nssa_core::{account::AccountId, program::ProgramId};
use risc0_binfmt::ProgramBinary;
use twap_oracle_core::compute_current_tick_account_pda;

/// Computes the `ProgramId` (Image ID) of a compiled guest ELF, the same way
/// the sequencer/wallet does when deploying a program.
pub fn program_id_from_elf(elf: &[u8]) -> Result<ProgramId, String> {
    let binary = ProgramBinary::decode(elf).map_err(|e| format!("{e:?}"))?;
    let id = binary.compute_image_id().map_err(|e| format!("{e:?}"))?;
    Ok(id.into())
}

pub fn config_pda(amm: ProgramId) -> AccountId {
    compute_config_pda(amm)
}

pub fn pool_pda(amm: ProgramId, def_a: AccountId, def_b: AccountId) -> AccountId {
    compute_pool_pda(amm, def_a, def_b)
}

pub fn vault_pda(amm: ProgramId, pool: AccountId, def: AccountId) -> AccountId {
    compute_vault_pda(amm, pool, def)
}

pub fn current_tick_pda(twap: ProgramId, pool: AccountId) -> AccountId {
    compute_current_tick_account_pda(twap, pool)
}

#[cfg(test)]
mod tests {
    use amm_core::compute_pool_pda;
    use nssa_core::account::AccountId;
    use nssa_core::program::ProgramId;

    use super::*;

    #[test]
    fn pool_pda_matches_core() {
        let amm: ProgramId = [1u32; 8];
        let a = AccountId::new([2u8; 32]);
        let b = AccountId::new([3u8; 32]);
        assert_eq!(pool_pda(amm, a, b), compute_pool_pda(amm, a, b));
    }
}
