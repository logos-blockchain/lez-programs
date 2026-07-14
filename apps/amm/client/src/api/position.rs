use nssa_core::{account::AccountId, program::ProgramId};
use serde_json::{json, Value};

use super::{
    commitment::SourceCommitment, holding::SelectedHolding, pair::PairIds, quote_error::issue,
    PairSnapshot, PositionRequest, SCHEMA,
};
use crate::account::{account_id_from_hex, program_id_base58};

#[derive(Clone)]
pub(super) enum QuoteBranch {
    Missing {
        amount_a: u128,
        amount_b: u128,
    },
    Active {
        max_a: u128,
        max_b: u128,
        minimum_lp: u128,
        stored_reversed: bool,
    },
}

pub(super) struct EvaluatedQuote {
    pub(super) value: Value,
    pub(super) quote_hash: String,
    pub(super) plan: Option<NewPositionPlan>,
}

pub(super) enum QuoteComputation {
    Failed(QuoteFailure),
    Evaluated(EvaluatedQuote),
}

pub(super) struct QuoteFailure {
    pub(super) code: &'static str,
    pub(super) fields: Vec<&'static str>,
    pub(super) details: Value,
}

pub(super) struct NewPositionPlan {
    pub(super) accounts: AccountPlan,
    pub(super) branch: QuoteBranch,
}

pub(super) struct AccountPlan {
    pub(super) rows: Vec<AccountPlanRow>,
    pub(super) sources: Vec<SourceCommitment>,
}

pub(super) struct AccountPlanRow {
    pub(super) role: &'static str,
    pub(super) account_id: Option<AccountId>,
    pub(super) expected_program: Option<ProgramId>,
    pub(super) action: &'static str,
    pub(super) signer: bool,
    pub(super) init: bool,
}

pub(super) struct AccountPlanHoldings<'a> {
    pub(super) token_a: Option<&'a SelectedHolding>,
    pub(super) token_b: Option<&'a SelectedHolding>,
    pub(super) lp: Option<&'a SelectedHolding>,
}

impl QuoteComputation {
    pub(super) fn into_value(self, request: &PositionRequest) -> Value {
        match self {
            Self::Failed(failure) => failure.into_value(request),
            Self::Evaluated(EvaluatedQuote { value, .. }) => value,
        }
    }

    pub(super) fn quote_hash(&self) -> Option<&str> {
        match self {
            Self::Failed(_) => None,
            Self::Evaluated(quote) => Some(&quote.quote_hash),
        }
    }
}

impl QuoteFailure {
    pub(super) fn into_value(self, request: &PositionRequest) -> Value {
        json!({
            "schema": SCHEMA,
            "status": "error",
            "canSubmit": false,
            "code": self.code,
            "poolStatus": "unavailable_pool",
            "tokenAId": request.token_a_id,
            "tokenBId": request.token_b_id,
            "accountPreview": [],
            "errors": [issue(
                self.code,
                "Position quote is unavailable.",
                &self.fields,
                self.details,
            )],
            "warnings": [],
        })
    }
}

impl NewPositionPlan {
    pub(super) fn new(accounts: AccountPlan, branch: QuoteBranch) -> Result<Self, String> {
        accounts.validate_ready()?;
        Ok(Self { accounts, branch })
    }

    pub(super) fn requires_fresh_lp(&self) -> bool {
        self.accounts.requires_fresh_lp()
    }
}

impl AccountPlan {
    pub(super) fn validate_snapshot_ids(
        pair: &PairIds,
        snapshot: &PairSnapshot,
    ) -> Option<&'static str> {
        let expected = [
            (&snapshot.config, pair.config, "config"),
            (&snapshot.token_a, pair.token_a, "token_a"),
            (&snapshot.token_b, pair.token_b, "token_b"),
            (&snapshot.pool, pair.pool, "pool"),
            (&snapshot.vault_a, pair.vault_a, "vault_a"),
            (&snapshot.vault_b, pair.vault_b, "vault_b"),
            (&snapshot.lp_definition, pair.lp_definition, "lp_definition"),
            (
                &snapshot.lp_lock_holding,
                pair.lp_lock_holding,
                "lp_lock_holding",
            ),
            (&snapshot.current_tick, pair.current_tick, "current_tick"),
            (&snapshot.clock, pair.clock, "clock"),
        ];
        expected.into_iter().find_map(|(read, id, role)| {
            match account_id_from_hex(&read.id, "account id") {
                Ok(actual) if actual == id => None,
                _ => Some(role),
            }
        })
    }

    pub(super) fn preview(&self) -> Vec<Value> {
        self.rows
            .iter()
            .enumerate()
            .map(|(order, row)| row.preview(order))
            .collect()
    }

    pub(super) fn take_sources(&mut self) -> Vec<SourceCommitment> {
        std::mem::take(&mut self.sources)
    }

    pub(super) fn requires_fresh_lp(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.role == "user_holding_lp" && row.account_id.is_none())
    }

    pub(super) fn contains(&self, account_id: AccountId) -> bool {
        self.rows
            .iter()
            .any(|row| row.account_id == Some(account_id))
    }

    pub(super) fn validate_ready(&self) -> Result<(), String> {
        if self.rows.iter().any(|row| {
            row.account_id.is_none() && !(row.role == "user_holding_lp" && row.signer && row.init)
        }) {
            return Err(String::from("submittable quote has an unresolved account"));
        }
        Ok(())
    }

    pub(super) fn wallet_args(
        &self,
        fresh_lp: Option<AccountId>,
    ) -> Result<(Vec<AccountId>, Vec<bool>), String> {
        let mut account_ids = Vec::with_capacity(self.rows.len());
        let mut signing_requirements = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            let account_id = match row.account_id {
                Some(account_id) => account_id,
                None if row.role == "user_holding_lp" => {
                    fresh_lp.ok_or_else(|| String::from("transaction plan has no LP holding"))?
                }
                None => return Err(String::from("transaction plan has an unresolved account")),
            };
            account_ids.push(account_id);
            signing_requirements.push(row.signer);
        }
        Ok((account_ids, signing_requirements))
    }
}

impl AccountPlanRow {
    pub(super) fn new(
        role: &'static str,
        account_id: Option<AccountId>,
        expected_program: Option<ProgramId>,
        action: &'static str,
        signer: bool,
        init: bool,
    ) -> Self {
        Self {
            role,
            account_id,
            expected_program,
            action,
            signer,
            init,
        }
    }

    fn preview(&self, order: usize) -> Value {
        json!({
            "order": order,
            "role": self.role,
            "accountId": self.account_id.map(|id| id.to_string()),
            "expectedProgramId": self.expected_program.map(program_id_base58),
            "action": self.action,
            "writable": self.action != "read",
            "signer": self.signer,
            "init": self.init,
        })
    }
}
