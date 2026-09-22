//! Print the Associated Token Account (ATA) address for an owner + token definition.
//!
//! Usage:
//!   cargo run -q -p ata_program --example ata_pdas -- <ata_account_id> <token_account_id>
//! <owner> <definition>
//!
//! Since LEZ v0.2.5 a program is addressed by the account id of its deployed `ProgramHeader`,
//! not by its ImageID, so every argument is a base58 account id.

use std::str::FromStr;

use ata_core::{compute_ata_seed, get_associated_token_account_id};
use lee_core::account::AccountId;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ata_s, token_s, owner_s, def_s] = args.as_slice() else {
        eprintln!("usage: ata_pdas <ata_account_id> <token_account_id> <owner> <definition>");
        std::process::exit(1);
    };
    let ata = AccountId::from_str(ata_s).expect("ata program account id must be base58");
    let token = AccountId::from_str(token_s).expect("token program account id must be base58");
    let owner = AccountId::from_str(owner_s).expect("owner must be base58");
    let definition = AccountId::from_str(def_s).expect("definition must be base58");

    let seed = compute_ata_seed(token, owner, definition);
    println!("ata {}", get_associated_token_account_id(&ata, &seed));
}
