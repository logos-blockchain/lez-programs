//! Print the TWAP oracle PDAs for a price source (current-tick always; the windowed
//! price-observations / oracle-price accounts when a window duration is given).
//!
//! Usage:
//!   cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- <oracle_pid> <price_source>
//! [<window_duration>]
//!
//! `oracle_pid` is a ProgramId as 8 comma-separated u32 limbs (as printed by `spel program-id`);
//! `price_source` is a base58 account id (e.g. an AMM pool); `window_duration` is a u64.

use std::str::FromStr;

use nssa_core::{account::AccountId, program::ProgramId};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda,
};

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
    let (oracle_s, source_s, window_s) = match args.as_slice() {
        [oracle, source] => (oracle, source, None),
        [oracle, source, window] => (oracle, source, Some(window)),
        _ => {
            eprintln!("usage: twap_oracle_pdas <oracle_pid> <price_source> [<window_duration>]");
            std::process::exit(1);
        }
    };
    let oracle = parse_pid(oracle_s);
    let source = AccountId::from_str(source_s).expect("price source must be base58");

    println!(
        "current_tick_account {}",
        compute_current_tick_account_pda(oracle, source)
    );

    if let Some(window_s) = window_s {
        let window: u64 = window_s.parse().expect("window_duration must be a u64");
        println!(
            "price_observations   {}",
            compute_price_observations_pda(oracle, source, window)
        );
        println!(
            "oracle_price_account {}",
            compute_oracle_price_account_pda(oracle, source, window)
        );
    }
}
