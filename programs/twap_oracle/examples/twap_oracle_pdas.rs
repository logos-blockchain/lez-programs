//! Print the TWAP oracle PDAs for a price source (current-tick always; the windowed
//! price-observations / oracle-price accounts when a window duration is given).
//!
//! Usage:
//!   cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- <oracle_account_id>
//! <price_source> [<window_duration>]
//!
//! Since LEZ v0.2.5 a program is addressed by the account id of its deployed `ProgramHeader`,
//! not by its ImageID, so `oracle_account_id` is a base58 account id — the same form as
//! `price_source` (e.g. an AMM pool). `window_duration` is a u64.

use std::str::FromStr;

use lee_core::account::AccountId;
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (oracle_s, source_s, window_s) = match args.as_slice() {
        [oracle, source] => (oracle, source, None),
        [oracle, source, window] => (oracle, source, Some(window)),
        _ => {
            eprintln!(
                "usage: twap_oracle_pdas <oracle_account_id> <price_source> [<window_duration>]"
            );
            std::process::exit(1);
        }
    };
    let oracle = AccountId::from_str(oracle_s).expect("oracle program account id must be base58");
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
