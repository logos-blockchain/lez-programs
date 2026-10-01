//! Print the stablecoin position + collateral-vault PDAs.
//!
//! Usage:
//!   cargo run -q -p stablecoin_program --example stablecoin_pdas --
//! <stablecoin_account_id> <owner> <position_nonce>
//!
//! Since LEZ v0.2.5 a program is addressed by the account id of its deployed `ProgramHeader`,
//! not by its ImageID, so `stablecoin_account_id` is a base58 account id, as is `owner`.
//! `position_nonce` is a u64.

use std::str::FromStr;

use lee_core::account::AccountId;
use stablecoin_core::{compute_position_pda, compute_position_vault_pda};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [stablecoin_s, owner_s, nonce_s] = args.as_slice() else {
        eprintln!("usage: stablecoin_pdas <stablecoin_account_id> <owner> <position_nonce>");
        std::process::exit(1);
    };
    let stablecoin =
        AccountId::from_str(stablecoin_s).expect("stablecoin program account id must be base58");
    let owner = AccountId::from_str(owner_s).expect("owner must be base58");
    let position_nonce: u64 = nonce_s.parse().expect("position_nonce must be a u64");

    let position = compute_position_pda(stablecoin, owner, position_nonce);
    println!("position        {position}");
    println!(
        "position_vault  {}",
        compute_position_vault_pda(stablecoin, position)
    );
}
