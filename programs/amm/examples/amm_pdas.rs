//! Print the AMM PDAs for a deployment (and, given a token pair, a pool's PDAs).
//!
//! Usage:
//!   cargo run -q -p amm_program --example amm_pdas -- <amm_pid> [<twap_pid> <defA> <defB>]
//!
//! `*_pid` are ProgramIds as 8 comma-separated u32 limbs (as printed by `spel program-id`);
//! `defA`/`defB` are base58 token-definition account ids. With only `<amm_pid>` it prints the
//! singleton config PDA; with all four args it also prints the pool/vault/LP/lock/tick PDAs.

use std::str::FromStr;

use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda,
};
use lee_core::{account::AccountId, program::ProgramId};
use twap_oracle_core::compute_current_tick_account_pda;

// Accepts a ProgramId as 8 comma-separated u32 limbs, a 64-char ImageID hex, or a base58
// ImageID. Hex/base58 are decoded as the 32 ImageID bytes read little-endian per u32 word,
// matching how `spel program-id` maps the ImageID to limbs.
fn parse_pid(s: &str) -> ProgramId {
    if s.contains(',') {
        let limbs: Vec<u32> = s
            .split(',')
            .map(|x| x.trim().parse().expect("ProgramId limb must be a u32"))
            .collect();
        assert_eq!(limbs.len(), 8, "ProgramId must be 8 u32 limbs");
        let mut pid: ProgramId = [0u32; 8];
        pid.copy_from_slice(&limbs);
        return pid;
    }
    let bytes: [u8; 32] = if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for (byte, pair) in out.iter_mut().zip(s.as_bytes().chunks_exact(2)) {
            let pair: [u8; 2] = pair.try_into().expect("hex pair");
            let hex = std::str::from_utf8(&pair).expect("ascii hex");
            *byte = u8::from_str_radix(hex, 16).expect("invalid hex digit");
        }
        out
    } else {
        AccountId::from_str(s)
            .expect("ProgramId must be 8 u32 limbs, a 64-char hex ImageID, or base58")
            .into_value()
    };
    let mut pid: ProgramId = [0u32; 8];
    for (limb, chunk) in pid.iter_mut().zip(bytes.chunks_exact(4)) {
        *limb = u32::from_le_bytes(chunk.try_into().expect("4-byte chunk"));
    }
    pid
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((amm_s, rest)) = args.split_first() else {
        eprintln!("usage: amm_pdas <amm_pid> [<twap_pid> <defA> <defB>]");
        std::process::exit(1);
    };
    let amm = parse_pid(amm_s);
    let config = compute_config_pda(amm);
    println!("config               {config}");

    if let [twap_s, def_a_s, def_b_s] = rest {
        let twap = parse_pid(twap_s);
        let def_a = AccountId::from_str(def_a_s).expect("defA must be base58");
        let def_b = AccountId::from_str(def_b_s).expect("defB must be base58");
        let pool = compute_pool_pda(amm, def_a, def_b);
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
    }
}
