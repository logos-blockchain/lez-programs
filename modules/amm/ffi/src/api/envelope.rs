//! The plan envelope the module's C++ hands to the wallet.

use amm_core::{PoolDefinition, PoolIdentity};
use lee_core::account::AccountId;
use serde_json::{json, Value};

use crate::account::account_id_hex;

/// One transaction slot: an address, the program whose shard of it the
/// transaction selects, and whether the slot must be signed.
///
/// `programAccountIds` is new in v0.3.0 -- a transaction names *shards*, not
/// accounts, so every slot carries a program alongside its address. It reaches
/// the wallet as an array parallel to `accountIds` and `signingRequirements`,
/// but it is built from a table of slots here so the three cannot drift out of
/// alignment.
///
/// A slot the instruction only wants a signature from names the program being
/// called: nothing is written there, and v0.3.0's `validate_apply_output` has no
/// rule about a declared account with no effect, so no `program_account_id` is
/// ever consulted for it.
#[derive(Clone, Copy)]
pub(super) struct Slot {
    account: AccountId,
    program: AccountId,
    signs: bool,
}

/// A slot the instruction reads or writes.
pub(super) const fn slot(account: AccountId, program: AccountId) -> Slot {
    Slot {
        account,
        program,
        signs: false,
    }
}

/// A slot that must additionally be signed.
pub(super) const fn signer(account: AccountId, program: AccountId) -> Slot {
    Slot {
        account,
        program,
        signs: true,
    }
}

pub(super) fn plan_response(program_id: &str, slots: &[Slot], instruction: Vec<u8>) -> Value {
    json!({
        "programId": program_id,
        "accountIds": slots
            .iter()
            .map(|slot| account_id_hex(slot.account))
            .collect::<Vec<_>>(),
        "programAccountIds": slots
            .iter()
            .map(|slot| account_id_hex(slot.program))
            .collect::<Vec<_>>(),
        "signingRequirements": slots.iter().map(|slot| slot.signs).collect::<Vec<_>>(),
        "instruction": instruction,
    })
}

/// The pool's five ids, declared because no plan phase can read them.
///
/// Every instruction that touches a pool needs them to derive the pool PDA and
/// address the chained transfers, mint or burn; the pool shard's own effect is
/// what pins them.
pub(super) fn identity_of(pool: &PoolDefinition) -> PoolIdentity {
    PoolIdentity {
        definition_token_a_id: pool.definition_token_a_id,
        definition_token_b_id: pool.definition_token_b_id,
        vault_a_id: pool.vault_a_id,
        vault_b_id: pool.vault_b_id,
        liquidity_pool_id: pool.liquidity_pool_id,
    }
}

/// Decodes the AMM's shard of a pool account from the hex blob the module reads.
pub(super) fn pool_from_hex(pool_data: &str) -> Result<PoolDefinition, String> {
    hex::decode(pool_data)
        .ok()
        .and_then(|bytes| borsh::from_slice::<PoolDefinition>(&bytes).ok())
        .ok_or_else(|| String::from("no_pool"))
}
