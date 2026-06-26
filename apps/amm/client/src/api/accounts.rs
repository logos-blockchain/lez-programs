use amm_core::PoolDefinition;
use nssa_core::program::ProgramId;

use super::{
    funding::{append_holding_source, sources_from_reads},
    pair::PairIds,
    position::{AccountPlan, AccountPlanHoldings, AccountPlanRow},
    QuoteRequest,
};

pub(super) fn missing_account_plan(
    input: &QuoteRequest,
    pair: PairIds,
    amm_program: ProgramId,
    holdings: AccountPlanHoldings<'_>,
) -> Result<AccountPlan, String> {
    let mut sources = sources_from_reads(&[
        ("config", &input.snapshot.config),
        ("token_a", &input.snapshot.token_a),
        ("token_b", &input.snapshot.token_b),
        ("pool", &input.snapshot.pool),
        ("vault_a", &input.snapshot.vault_a),
        ("vault_b", &input.snapshot.vault_b),
        ("lp_definition", &input.snapshot.lp_definition),
        ("lp_lock_holding", &input.snapshot.lp_lock_holding),
        ("current_tick", &input.snapshot.current_tick),
    ])?;
    append_holding_source(&mut sources, "holding_a", holdings.token_a);
    append_holding_source(&mut sources, "holding_b", holdings.token_b);
    Ok(AccountPlan {
        rows: vec![
            AccountPlanRow::new(
                "config",
                Some(pair.config),
                Some(amm_program),
                "read",
                false,
                false,
            ),
            AccountPlanRow::new(
                "pool",
                Some(pair.pool),
                Some(amm_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new(
                "vault_a",
                Some(pair.vault_a),
                Some(pair.token_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new(
                "vault_b",
                Some(pair.vault_b),
                Some(pair.token_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new(
                "lp_definition",
                Some(pair.lp_definition),
                Some(pair.token_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new(
                "lp_lock_holding",
                Some(pair.lp_lock_holding),
                Some(pair.token_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new(
                "user_holding_a",
                holdings.token_a.map(|value| value.id),
                Some(pair.token_program),
                "update",
                true,
                false,
            ),
            AccountPlanRow::new(
                "user_holding_b",
                holdings.token_b.map(|value| value.id),
                Some(pair.token_program),
                "update",
                true,
                false,
            ),
            AccountPlanRow::new(
                "user_holding_lp",
                None,
                Some(pair.token_program),
                "create",
                true,
                true,
            ),
            AccountPlanRow::new(
                "current_tick",
                Some(pair.current_tick),
                Some(pair.twap_program),
                "create",
                false,
                true,
            ),
            AccountPlanRow::new("clock", Some(pair.clock), None, "read", false, false),
        ],
        sources,
    })
}

pub(super) fn active_account_plan(
    input: &QuoteRequest,
    pair: PairIds,
    amm_program: ProgramId,
    pool: &PoolDefinition,
    stored_reversed: bool,
    holdings: AccountPlanHoldings<'_>,
) -> Result<AccountPlan, String> {
    let (stored_holding_a, stored_holding_b) = if stored_reversed {
        (holdings.token_b, holdings.token_a)
    } else {
        (holdings.token_a, holdings.token_b)
    };
    let mut sources = sources_from_reads(&[
        ("config", &input.snapshot.config),
        ("token_a", &input.snapshot.token_a),
        ("token_b", &input.snapshot.token_b),
        ("pool", &input.snapshot.pool),
        ("vault_a", &input.snapshot.vault_a),
        ("vault_b", &input.snapshot.vault_b),
        ("lp_definition", &input.snapshot.lp_definition),
        ("current_tick", &input.snapshot.current_tick),
    ])?;
    append_holding_source(&mut sources, "holding_a", holdings.token_a);
    append_holding_source(&mut sources, "holding_b", holdings.token_b);
    append_holding_source(&mut sources, "holding_lp", holdings.lp);
    Ok(AccountPlan {
        rows: vec![
            AccountPlanRow::new(
                "config",
                Some(pair.config),
                Some(amm_program),
                "read",
                false,
                false,
            ),
            AccountPlanRow::new(
                "pool",
                Some(pair.pool),
                Some(amm_program),
                "update",
                false,
                false,
            ),
            AccountPlanRow::new(
                "vault_a",
                Some(pool.vault_a_id),
                Some(pair.token_program),
                "update",
                false,
                false,
            ),
            AccountPlanRow::new(
                "vault_b",
                Some(pool.vault_b_id),
                Some(pair.token_program),
                "update",
                false,
                false,
            ),
            AccountPlanRow::new(
                "lp_definition",
                Some(pair.lp_definition),
                Some(pair.token_program),
                "update",
                false,
                false,
            ),
            AccountPlanRow::new(
                "user_holding_a",
                stored_holding_a.map(|value| value.id),
                Some(pair.token_program),
                "update",
                true,
                false,
            ),
            AccountPlanRow::new(
                "user_holding_b",
                stored_holding_b.map(|value| value.id),
                Some(pair.token_program),
                "update",
                true,
                false,
            ),
            AccountPlanRow::new(
                "user_holding_lp",
                holdings.lp.map(|value| value.id),
                Some(pair.token_program),
                if holdings.lp.is_some() {
                    "update"
                } else {
                    "create"
                },
                holdings.lp.is_none(),
                holdings.lp.is_none(),
            ),
            AccountPlanRow::new(
                "current_tick",
                Some(pair.current_tick),
                Some(pair.twap_program),
                "update",
                false,
                false,
            ),
            AccountPlanRow::new("clock", Some(pair.clock), None, "read", false, false),
        ],
        sources,
    })
}
