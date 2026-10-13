//! Print the AMM PDAs for a namespaced instance (and, given a token pair, a pool's PDAs).
//!
//! Usage:
//!   cargo run -q -p amm_program --example amm_pdas -- <amm_account_id> <owner>
//! [<twap_account_id> <defA> <defB>]
//!
//! Since LEZ v0.2.5 a program is addressed by the account id of its deployed `ProgramHeader`,
//! not by its ImageID, so every argument is a base58 account id. AMM instances are namespaced
//! by `(owner, nonce)`; this prints the owner's default instance (all-zero nonce). With
//! `<amm_account_id> <owner>` it prints the instance's config PDA; with all args it also prints
//! the pool/vault/LP/lock/tick and protocol-fee-holding PDAs.

use std::str::FromStr;

use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_protocol_fee_pda, compute_vault_pda,
};
use lee_core::account::AccountId;
use twap_oracle_core::compute_current_tick_account_pda;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [amm_s, owner_s, rest @ ..] = args.as_slice() else {
        eprintln!("usage: amm_pdas <amm_account_id> <owner> [<twap_account_id> <defA> <defB>]");
        std::process::exit(1);
    };
    let amm = AccountId::from_str(amm_s).expect("amm program account id must be base58");
    let owner = AccountId::from_str(owner_s).expect("owner must be base58");
    // The owner's default instance uses the all-zero nonce.
    let nonce = [0u8; 32];
    let config = compute_config_pda(amm, owner, nonce);
    println!("config               {config}");

    if let [twap_s, def_a_s, def_b_s] = rest {
        let twap = AccountId::from_str(twap_s).expect("twap program account id must be base58");
        let def_a = AccountId::from_str(def_a_s).expect("defA must be base58");
        let def_b = AccountId::from_str(def_b_s).expect("defB must be base58");
        let pool = compute_pool_pda(amm, config, def_a, def_b);
        println!("pool                 {pool}");
        println!(
            "vault_a              {}",
            compute_vault_pda(amm, pool, def_a)
        );
        println!(
            "vault_b              {}",
            compute_vault_pda(amm, pool, def_b)
        );
        println!(
            "pool_definition_lp   {}",
            compute_liquidity_token_pda(amm, pool)
        );
        println!(
            "lp_lock_holding      {}",
            compute_lp_lock_holding_pda(amm, pool)
        );
        println!(
            "current_tick_account {}",
            compute_current_tick_account_pda(twap, pool)
        );
        // Protocol-fee holdings are per (config, token definition) — the input token's holding is
        // the one a swap credits and `WithdrawProtocolFees` drains.
        println!(
            "protocol_fee_a       {}",
            compute_protocol_fee_pda(amm, config, def_a)
        );
        println!(
            "protocol_fee_b       {}",
            compute_protocol_fee_pda(amm, config, def_b)
        );
    }
}
