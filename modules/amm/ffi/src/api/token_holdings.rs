//! Lists the wallet's fungible token holdings for the account selector.
//!
//! A flat, decoded view of every `TokenHolding` the wallet owns (owned by the
//! configured token program), each row carrying the id in **both** encodings so
//! the swap view (hex ids) and the liquidity view (base58 ids) can each filter by
//! their own token id via the selector's `stateField`. Pure over the wallet reads +
//! config — no chain access of its own.
//!
//! Thin stopgap: token holdings are wallet/token-program data, not AMM data. This
//! lives here only because `amm_ffi` is the one place wired to decode `TokenHolding`
//! (via `token_core`); it should move to a dedicated token-program logos module once
//! one exists.

use serde_json::{json, Value};

use super::{config::load_config, holding::wallet_holdings, TokenHoldingsRequest};
use crate::account::{account_id_hex, parse_program_id};

pub(super) fn token_holdings(request: TokenHoldingsRequest) -> Result<Value, String> {
    let amm_program = parse_program_id(&request.amm_program_id)?;
    let config = load_config(amm_program, &request.config)?;
    let holdings = wallet_holdings(&request.wallet_accounts, config.token_program_id);
    let rows = holdings
        .into_iter()
        .map(|holding| {
            json!({
                "accountId": account_id_hex(holding.id),
                "accountType": "TokenHolding",
                // Both encodings: the swap view filters on definitionIdHex, the
                // liquidity view on the base58 definitionId.
                "definitionId": holding.definition_id.to_string(),
                "definitionIdHex": account_id_hex(holding.definition_id),
                "balanceRaw": holding.balance.to_string(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "holdings": rows }))
}

#[cfg(test)]
mod tests {
    use amm_core::{compute_config_pda, AmmConfig};
    use lee_core::{
        account::{Account, AccountId, Data},
        program::ProgramId,
    };
    use token_core::TokenHolding;

    use super::*;
    use crate::account::{account_read, AccountRead};

    fn token_program() -> ProgramId {
        parse_program_id(&"01".repeat(32)).unwrap()
    }

    fn holding_read(id: AccountId, definition: AccountId, balance: u128) -> AccountRead {
        let account = Account {
            program_owner: token_program(),
            data: (&TokenHolding::Fungible {
                definition_id: definition,
                balance,
            })
                .into(),
            ..Account::default()
        };
        account_read(id, &account)
    }

    fn config_read(amm: ProgramId) -> AccountRead {
        let account = Account {
            program_owner: amm,
            data: Data::from(&AmmConfig {
                token_program_id: token_program(),
                twap_oracle_program_id: parse_program_id(&"02".repeat(32)).unwrap(),
                authority: AccountId::new([0x09; 32]),
            }),
            ..Account::default()
        };
        account_read(compute_config_pda(amm), &account)
    }

    #[test]
    fn lists_wallet_token_holdings_with_both_id_encodings() {
        let amm = parse_program_id(&"00".repeat(32)).unwrap();
        let def = AccountId::new([0xAA; 32]);
        let holding_id = AccountId::new([0x01; 32]);

        let value = token_holdings(TokenHoldingsRequest {
            amm_program_id: "00".repeat(32),
            config: config_read(amm),
            wallet_accounts: vec![holding_read(holding_id, def, 500)],
        })
        .unwrap();

        let rows = value["holdings"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["accountId"], account_id_hex(holding_id));
        assert_eq!(rows[0]["accountType"], "TokenHolding");
        assert_eq!(rows[0]["definitionId"], def.to_string());
        assert_eq!(rows[0]["definitionIdHex"], account_id_hex(def));
        assert_eq!(rows[0]["balanceRaw"], "500");
    }

    #[test]
    fn skips_non_token_accounts_and_fails_closed_on_bad_config() {
        let amm = parse_program_id(&"00".repeat(32)).unwrap();
        // A non-token-program account is not a holding.
        let foreign = Account {
            program_owner: parse_program_id(&"ee".repeat(32)).unwrap(),
            ..Account::default()
        };
        let value = token_holdings(TokenHoldingsRequest {
            amm_program_id: "00".repeat(32),
            config: config_read(amm),
            wallet_accounts: vec![account_read(AccountId::new([0x02; 32]), &foreign)],
        })
        .unwrap();
        assert!(value["holdings"].as_array().unwrap().is_empty());

        // A read-failed config fails closed as Err.
        assert!(token_holdings(TokenHoldingsRequest {
            amm_program_id: "00".repeat(32),
            config: AccountRead {
                id: String::new(),
                status: String::from("read_failed"),
                account: None,
            },
            wallet_accounts: vec![],
        })
        .is_err());
    }
}
