//! Print the Associated Token Account (ATA) address for an owner + token definition.
//!
//! Usage:
//!   cargo run -q -p ata_program --example ata_pdas -- <ata_pid> <token_pid> <owner> <definition>
//!
//! `*_pid` are ProgramIds as 8 comma-separated u32 limbs (as printed by `spel program-id`);
//! `owner` and `definition` are base58 account ids.

use std::str::FromStr;

use ata_core::{compute_ata_seed, get_associated_token_account_id};
use nssa_core::{account::AccountId, program::ProgramId};

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
    let [ata_s, token_s, owner_s, def_s] = args.as_slice() else {
        eprintln!("usage: ata_pdas <ata_pid> <token_pid> <owner> <definition>");
        std::process::exit(1);
    };
    let ata = parse_pid(ata_s);
    let token = parse_pid(token_s);
    let owner = AccountId::from_str(owner_s).expect("owner must be base58");
    let definition = AccountId::from_str(def_s).expect("definition must be base58");

    let seed = compute_ata_seed(token, owner, definition);
    println!("ata {}", get_associated_token_account_id(&ata, &seed));
}
