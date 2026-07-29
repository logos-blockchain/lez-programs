use borsh::BorshSerialize;

#[derive(BorshSerialize)]
pub(super) enum RequestCommitment {
    Missing {
        amount_a: u128,
        amount_b: u128,
    },
    Active {
        max_a: u128,
        max_b: u128,
        slippage_bps: u32,
    },
}

#[derive(BorshSerialize)]
pub(super) struct SourceCommitment {
    pub(super) role: String,
    pub(super) commitment: [u8; 32],
}

#[derive(BorshSerialize)]
pub(super) struct FundingCommitment {
    pub(super) token_id: [u8; 32],
    pub(super) holding_id: Option<[u8; 32]>,
    pub(super) available: u128,
    pub(super) requested: u128,
}

#[derive(BorshSerialize)]
pub(super) struct QuoteCommitment {
    pub(super) network_id: String,
    pub(super) network_fingerprint: String,
    pub(super) amm_program_id: [u8; 32],
    pub(super) token_a_id: [u8; 32],
    pub(super) token_b_id: [u8; 32],
    pub(super) fee_bps: u32,
    pub(super) pool_status: u8,
    pub(super) request: RequestCommitment,
    pub(super) max_a: u128,
    pub(super) max_b: u128,
    pub(super) actual_a: u128,
    pub(super) actual_b: u128,
    pub(super) expected_lp: u128,
    pub(super) lp_guard: u128,
    pub(super) requires_fresh_lp: bool,
    pub(super) sources: Vec<SourceCommitment>,
    pub(super) funding: Vec<FundingCommitment>,
    pub(super) warnings: Vec<String>,
}
