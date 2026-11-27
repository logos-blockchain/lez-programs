//! Print the stablecoin program's PDAs: the engine, the stablecoin definition and the surplus
//! holding, and with an owner that owner's position vault.
//!
//! Usage:
//!   cargo run -q -p stablecoin_program --example stablecoin_pdas -- \
//!     <stablecoin_program_id> [<owner_account_id>]
//!
//! Ids are base58. A program is addressed by the account id of its deployed header.

use std::str::FromStr;

use lee_core::account::AccountId;
use stablecoin_core::{
    compute_engine_pda, compute_position_vault_pda, compute_stablecoin_definition_pda,
    compute_surplus_holding_pda,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (program, owner) = match args.as_slice() {
        [program] => (program, None),
        [program, owner] => (program, Some(owner)),
        _ => {
            eprintln!("usage: stablecoin_pdas <stablecoin_program_id> [<owner_account_id>]");
            std::process::exit(1);
        }
    };
    let program =
        AccountId::from_str(program).expect("stablecoin program id must be a base58 account id");

    println!("engine                {}", compute_engine_pda(program));
    println!(
        "stablecoin_definition {}",
        compute_stablecoin_definition_pda(program)
    );
    println!(
        "surplus_holding       {}",
        compute_surplus_holding_pda(program)
    );
    if let Some(owner) = owner {
        let owner = AccountId::from_str(owner).expect("owner must be a base58 account id");
        println!(
            "position_vault        {}",
            compute_position_vault_pda(program, owner)
        );
    }
}
