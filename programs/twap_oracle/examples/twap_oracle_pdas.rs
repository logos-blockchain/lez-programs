//! Print a price source's TWAP oracle accounts: the current-tick actor's, and with a window the
//! observations and price actors'.
//!
//! Usage:
//!   cargo run -q -p twap_oracle_program --example twap_oracle_pdas -- \
//!     <oracle_account_id> <price_source> <source_program | key> [<window_duration>]
//!
//! `source_program` is the program whose actor on `price_source` sends to the oracle (e.g. the
//! AMM for a pool); `key` means `price_source` is a key-owned account. Ids are base58.

use std::str::FromStr;

use lee_core::account::{AccountId, Actor};
use twap_oracle_core::{
    compute_oracle_price_account_pda, compute_price_observations_pda, PriceSource,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (oracle, source, kind, window) = match args.as_slice() {
        [oracle, source, kind] => (oracle, source, kind, None),
        [oracle, source, kind, window] => (oracle, source, kind, Some(window)),
        _ => {
            eprintln!(
                "usage: twap_oracle_pdas <oracle_account_id> <price_source> \
                 <source_program | key> [<window_duration>]"
            );
            std::process::exit(1);
        }
    };
    let oracle = AccountId::from_str(oracle).expect("oracle program account id must be base58");
    let source = AccountId::from_str(source).expect("price source must be base58");
    let source = if kind == "key" {
        PriceSource::Key(source)
    } else {
        let program = AccountId::from_str(kind).expect("source program must be base58 or `key`");
        PriceSource::program(Actor::new(source, program))
    };
    let current_tick = source.current_tick_account_id();
    println!("current_tick_account {current_tick}");

    if let Some(window) = window {
        let window: u64 = window.parse().expect("window_duration must be a u64");
        println!(
            "price_observations   {}",
            compute_price_observations_pda(oracle, current_tick, window)
        );
        println!(
            "oracle_price_account {}",
            compute_oracle_price_account_pda(oracle, current_tick, window)
        );
    }
}
