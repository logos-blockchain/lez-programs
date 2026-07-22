//! Snapshot-to-transaction facade for wallet and API consumers.
//!
//! Each operation validates immutable account snapshots, delegates economic calculations to the
//! program-owned quote API, applies the shared slippage policy, and emits the canonical planner
//! output. The facade is deterministic and performs no RPC, signing, submission, clock lookup, or
//! runtime program-identity/version check.

use std::{error::Error, fmt};

use amm_program::quote::{
    AddLiquidityQuote, CreatePoolQuote, PairOrder, RemoveLiquidityQuote, SwapQuote,
};
use nssa_core::{
    account::{Account, AccountId},
    program::ProgramId,
    Commitment,
};
use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::Serialize;
use token_core::TokenHolding;

use crate::{
    discovery::{
        inspect_config, inspect_pair, ActivePairInspection, PairInspection, PairReadSnapshots,
    },
    intent::{pool_spot_change_bps, IntentError},
    plan::{
        plan_add_liquidity, plan_create_pool, plan_remove_liquidity, plan_swap_exact_input,
        plan_swap_exact_output, AddLiquidityPlanInput, CreatePoolPlanInput, PoolContext,
        RemoveLiquidityPlanInput, SwapExactInputPlanInput, SwapExactOutputPlanInput,
        TransactionPlan,
    },
    quote::{
        AccountSnapshot, ValidatedFungibleDefinition, ValidatedFungibleHolding,
        ValidatedPoolSnapshot,
    },
    slippage::{
        prepare_add_liquidity, prepare_create_pool, prepare_remove_liquidity,
        prepare_swap_exact_input, prepare_swap_exact_output, SlippageTolerance,
    },
    AmmContext, ClientError,
};

const COMMITMENT_DOMAIN: &str = "lez.amm.client.prepared-transaction.v1";

/// One AMM operation represented by a prepared transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum TransactionOperation {
    CreatePool,
    AddLiquidity,
    RemoveLiquidity,
    SwapExactInput,
    SwapExactOutput,
}

/// Exact operation amounts expressed in caller first/second order.
///
/// For pool creation and liquidity operations, `first` and `second` correspond to the supplied
/// token-definition IDs. For swaps, `first` is input and `second` is output.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CallerAmounts {
    first: u128,
    second: u128,
}

impl CallerAmounts {
    #[must_use]
    pub const fn new(first: u128, second: u128) -> Self {
        Self { first, second }
    }

    #[must_use]
    pub const fn first(self) -> u128 {
        self.first
    }

    #[must_use]
    pub const fn second(self) -> u128 {
        self.second
    }
}

/// One selected wallet holding and the spend capacity required by the instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FundingRequirement {
    holding_account_id: AccountId,
    token_definition_id: AccountId,
    available: u128,
    required: u128,
}

impl FundingRequirement {
    #[must_use]
    pub const fn holding_account_id(&self) -> AccountId {
        self.holding_account_id
    }

    #[must_use]
    pub const fn token_definition_id(&self) -> AccountId {
        self.token_definition_id
    }

    #[must_use]
    pub const fn available(&self) -> u128 {
        self.available
    }

    #[must_use]
    pub const fn required(&self) -> u128 {
        self.required
    }
}

/// Wallet-owned prerequisites extracted from the exact plan and selected snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletPrerequisites {
    signer_account_ids: Vec<AccountId>,
    fresh_account_ids: Vec<AccountId>,
    funding: Vec<FundingRequirement>,
}

impl WalletPrerequisites {
    /// Accounts that must be authorized, in instruction-account order.
    #[must_use]
    pub fn signer_account_ids(&self) -> &[AccountId] {
        &self.signer_account_ids
    }

    /// Selected destination accounts whose supplied snapshot was exactly `Account::default()`.
    #[must_use]
    pub fn fresh_account_ids(&self) -> &[AccountId] {
        &self.fresh_account_ids
    }

    /// Funding requirements in caller token order.
    #[must_use]
    pub fn funding(&self) -> &[FundingRequirement] {
        &self.funding
    }
}

/// SHA-256 commitment to the typed request, exact plan, and role-tagged account snapshots.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QuoteCommitment([u8; 32]);

impl QuoteCommitment {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Returns `quote_changed` when a refreshed preparation no longer matches this commitment.
    pub fn ensure_unchanged(self, actual: Self) -> Result<(), TransactionError> {
        ensure_quote_unchanged(self, actual)
    }
}

/// Compares a previously presented quote commitment with a freshly prepared one.
pub fn ensure_quote_unchanged(
    expected: QuoteCommitment,
    actual: QuoteCommitment,
) -> Result<(), TransactionError> {
    if expected.0 == actual.0 {
        Ok(())
    } else {
        Err(TransactionError::QuoteChanged { expected, actual })
    }
}

/// Canonical quote paired with the exact transaction that consumes it.
pub struct PreparedTransaction<Q> {
    operation: TransactionOperation,
    quote: Q,
    plan: TransactionPlan,
    quote_commitment: QuoteCommitment,
    affected_account_ids: Vec<AccountId>,
    wallet_prerequisites: WalletPrerequisites,
    caller_amounts: CallerAmounts,
    deadline: u64,
    pool_spot_change_bps: Option<u128>,
}

impl<Q> PreparedTransaction<Q> {
    #[must_use]
    pub const fn operation(&self) -> TransactionOperation {
        self.operation
    }

    #[must_use]
    pub const fn quote(&self) -> &Q {
        &self.quote
    }

    #[must_use]
    pub const fn plan(&self) -> &TransactionPlan {
        &self.plan
    }

    #[must_use]
    pub const fn quote_commitment(&self) -> QuoteCommitment {
        self.quote_commitment
    }

    /// Writable account IDs in first-occurrence instruction order.
    #[must_use]
    pub fn affected_account_ids(&self) -> &[AccountId] {
        &self.affected_account_ids
    }

    #[must_use]
    pub const fn wallet_prerequisites(&self) -> &WalletPrerequisites {
        &self.wallet_prerequisites
    }

    #[must_use]
    pub const fn caller_amounts(&self) -> CallerAmounts {
        self.caller_amounts
    }

    #[must_use]
    pub const fn deadline(&self) -> u64 {
        self.deadline
    }

    /// Directional pre/post pool spot movement for swaps; absent for non-swap operations.
    #[must_use]
    pub const fn pool_spot_change_bps(&self) -> Option<u128> {
        self.pool_spot_change_bps
    }

    #[must_use]
    pub fn into_quote_and_plan(self) -> (Q, TransactionPlan) {
        (self.quote, self.plan)
    }
}

/// Failure while validating snapshots or materializing a prepared transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TransactionError {
    Client(ClientError),
    Intent(IntentError),
    FeeMismatch {
        expected: u128,
        actual: u128,
    },
    QuoteChanged {
        expected: QuoteCommitment,
        actual: QuoteCommitment,
    },
    DuplicateAccountId {
        account_id: AccountId,
    },
    InstructionEncoding,
    CommitmentEncoding,
}

impl TransactionError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Client(error) => error.code(),
            Self::Intent(error) => error.code(),
            Self::FeeMismatch { .. } => "fee_mismatch",
            Self::QuoteChanged { .. } => "quote_changed",
            Self::DuplicateAccountId { .. } => "duplicate_account_id",
            Self::InstructionEncoding => "instruction_encoding_failed",
            Self::CommitmentEncoding => "quote_commitment_encoding_failed",
        }
    }
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => error.fmt(formatter),
            Self::Intent(error) => error.fmt(formatter),
            Self::FeeMismatch { expected, actual } => write!(
                formatter,
                "expected pool fee {expected} bps, snapshot contains {actual} bps"
            ),
            Self::QuoteChanged { .. } => {
                formatter.write_str("prepared quote changed after snapshot refresh")
            }
            Self::DuplicateAccountId { .. } => {
                formatter.write_str("transaction plan contains a duplicate account ID")
            }
            Self::InstructionEncoding => {
                formatter.write_str("AMM instruction serialization failed")
            }
            Self::CommitmentEncoding => {
                formatter.write_str("prepared-transaction commitment serialization failed")
            }
        }
    }
}

impl Error for TransactionError {}

impl From<ClientError> for TransactionError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

impl From<IntentError> for TransactionError {
    fn from(error: IntentError) -> Self {
        Self::Intent(error)
    }
}

/// Raw fetched accounts required to validate an initialized pool.
#[derive(Clone, Copy)]
pub struct PoolAccountSnapshots<'a> {
    pub config: &'a AccountSnapshot,
    pub pair: PairReadSnapshots<'a>,
}

impl PoolAccountSnapshots<'_> {
    /// Validates config, complete pair lifecycle, current tick, and clock snapshots.
    pub fn validate(
        self,
        amm_program_id: ProgramId,
        first_token_definition_id: AccountId,
        second_token_definition_id: AccountId,
    ) -> Result<(AmmContext, Box<ActivePairInspection>), ClientError> {
        let context = inspect_config(amm_program_id, self.config)?;
        match inspect_pair(
            &context,
            first_token_definition_id,
            second_token_definition_id,
            self.pair,
        )? {
            PairInspection::Active(active) => Ok((context, active)),
            PairInspection::Missing(_) => Err(ClientError::InvalidAccountData {
                account: "AMM pool",
                expected: "initialized pool lifecycle",
            }),
        }
    }
}

/// Caller-order pool-creation request over raw account snapshots.
#[derive(Clone, Copy)]
pub struct CreatePoolTransactionInput<'a> {
    pub amm_program_id: ProgramId,
    pub config: &'a AccountSnapshot,
    pub pair: PairReadSnapshots<'a>,
    pub first_token_definition_id: AccountId,
    pub second_token_definition_id: AccountId,
    pub first_token_holding: &'a AccountSnapshot,
    pub second_token_holding: &'a AccountSnapshot,
    pub liquidity_holding: &'a AccountSnapshot,
    pub first_amount: u128,
    pub second_amount: u128,
    pub fee_bps: u128,
    pub deadline: u64,
}

/// Caller-order add-liquidity request over a complete pool snapshot.
#[derive(Clone, Copy)]
pub struct AddLiquidityTransactionInput<'a> {
    pub amm_program_id: ProgramId,
    pub pool_accounts: PoolAccountSnapshots<'a>,
    pub first_token_definition_id: AccountId,
    pub second_token_definition_id: AccountId,
    pub first_token_holding: &'a AccountSnapshot,
    pub second_token_holding: &'a AccountSnapshot,
    pub liquidity_holding: &'a AccountSnapshot,
    pub max_first_amount: u128,
    pub max_second_amount: u128,
    pub slippage: SlippageTolerance,
    pub expected_fee_bps: Option<u128>,
    pub deadline: u64,
}

/// Caller-order remove-liquidity request over a complete pool snapshot.
#[derive(Clone, Copy)]
pub struct RemoveLiquidityTransactionInput<'a> {
    pub amm_program_id: ProgramId,
    pub pool_accounts: PoolAccountSnapshots<'a>,
    pub first_token_definition_id: AccountId,
    pub second_token_definition_id: AccountId,
    pub first_token_holding: &'a AccountSnapshot,
    pub second_token_holding: &'a AccountSnapshot,
    pub liquidity_holding: &'a AccountSnapshot,
    pub remove_liquidity_amount: u128,
    pub slippage: SlippageTolerance,
    pub expected_fee_bps: Option<u128>,
    pub deadline: u64,
}

/// Exact-input swap request over a complete pool snapshot.
#[derive(Clone, Copy)]
pub struct SwapExactInputTransactionInput<'a> {
    pub amm_program_id: ProgramId,
    pub pool_accounts: PoolAccountSnapshots<'a>,
    pub input_token_definition_id: AccountId,
    pub output_token_definition_id: AccountId,
    pub input_holding: &'a AccountSnapshot,
    pub output_holding: &'a AccountSnapshot,
    pub amount_in: u128,
    pub slippage: SlippageTolerance,
    pub expected_fee_bps: Option<u128>,
    pub deadline: u64,
}

/// Exact-output swap request over a complete pool snapshot.
#[derive(Clone, Copy)]
pub struct SwapExactOutputTransactionInput<'a> {
    pub amm_program_id: ProgramId,
    pub pool_accounts: PoolAccountSnapshots<'a>,
    pub input_token_definition_id: AccountId,
    pub output_token_definition_id: AccountId,
    pub input_holding: &'a AccountSnapshot,
    pub output_holding: &'a AccountSnapshot,
    pub exact_amount_out: u128,
    pub slippage: SlippageTolerance,
    pub expected_fee_bps: Option<u128>,
    pub deadline: u64,
}

/// Validates funding, quotes creation, and emits one exact `NewDefinition` plan.
pub fn prepare_create_pool_transaction(
    input: CreatePoolTransactionInput<'_>,
) -> Result<PreparedTransaction<CreatePoolQuote>, TransactionError> {
    let context = inspect_config(input.amm_program_id, input.config)?;
    let missing = match inspect_pair(
        &context,
        input.first_token_definition_id,
        input.second_token_definition_id,
        input.pair,
    )? {
        PairInspection::Missing(missing) => missing,
        PairInspection::Active(_) => {
            return Err(ClientError::InvalidAccountData {
                account: "AMM pool",
                expected: "uninitialized pool lifecycle",
            }
            .into())
        }
    };
    let first_definition = missing.first_token_definition();
    let second_definition = missing.second_token_definition();
    let first_holding =
        ValidatedFungibleHolding::new(&context, input.first_token_holding, first_definition)?;
    let second_holding =
        ValidatedFungibleHolding::new(&context, input.second_token_holding, second_definition)?;

    let canonical = missing.manifest().canonical_pair();
    let stored_a_id = canonical.token_a_id();
    let stored_b_id = canonical.token_b_id();
    let order = if first_definition.account_id() == stored_a_id {
        PairOrder::Stored
    } else {
        PairOrder::Reversed
    };
    let (stored_a_definition, stored_b_definition, stored_a_holding, stored_b_holding) = match order
    {
        PairOrder::Stored => (
            &first_definition,
            &second_definition,
            &first_holding,
            &second_holding,
        ),
        PairOrder::Reversed => (
            &second_definition,
            &first_definition,
            &second_holding,
            &first_holding,
        ),
    };
    let (stored_a_amount, stored_b_amount) =
        order.amounts_to_stored(input.first_amount, input.second_amount);
    let prepared = prepare_create_pool(
        &context,
        stored_a_definition,
        stored_b_definition,
        stored_a_amount,
        stored_b_amount,
        input.fee_bps,
    )?;
    ensure_funded(&first_holding, input.first_amount, "first token holding")?;
    ensure_funded(&second_holding, input.second_amount, "second token holding")?;

    let fresh_liquidity = validate_holding_destination(
        &context,
        input.liquidity_holding,
        missing.manifest().liquidity_definition_id(),
        "liquidity holding",
    )?;
    let plan = plan_create_pool(CreatePoolPlanInput {
        context: &context,
        token_a_definition_id: stored_a_id,
        token_b_definition_id: stored_b_id,
        user_holding_a: stored_a_holding.account_id(),
        user_holding_b: stored_b_holding.account_id(),
        user_holding_lp: input.liquidity_holding.account_id(),
        token_a_amount: prepared.token_a_amount,
        token_b_amount: prepared.token_b_amount,
        fees: prepared.fees,
        deadline: input.deadline,
    })?;
    let sources = pair_sources(
        input.config,
        input.pair,
        input.first_token_holding,
        input.second_token_holding,
        Some(input.liquidity_holding),
    );
    let funding = vec![
        funding(&first_holding, input.first_amount),
        funding(&second_holding, input.second_amount),
    ];
    let fresh = fresh_liquidity
        .then_some(input.liquidity_holding.account_id())
        .into_iter()
        .collect();

    PreparedTransaction::new(
        TransactionOperation::CreatePool,
        TransactionIntent::CreatePool {
            first_token_definition_id: input.first_token_definition_id,
            second_token_definition_id: input.second_token_definition_id,
            first_amount: input.first_amount,
            second_amount: input.second_amount,
            fee_bps: input.fee_bps,
        },
        prepared.quote,
        plan,
        sources,
        fresh,
        funding,
        CallerAmounts::new(input.first_amount, input.second_amount),
        input.deadline,
        None,
    )
}

/// Validates funding, quotes a proportional deposit, and emits one exact add plan.
pub fn prepare_add_liquidity_transaction(
    input: AddLiquidityTransactionInput<'_>,
) -> Result<PreparedTransaction<AddLiquidityQuote>, TransactionError> {
    let (context, active) = input.pool_accounts.validate(
        input.amm_program_id,
        input.first_token_definition_id,
        input.second_token_definition_id,
    )?;
    let snapshot = active.pool();
    let order = active.caller_order();
    validate_expected_fee(snapshot, input.expected_fee_bps)?;
    let (first_definition, second_definition) = caller_definitions(snapshot, order);
    let first_holding =
        ValidatedFungibleHolding::new(&context, input.first_token_holding, first_definition)?;
    let second_holding =
        ValidatedFungibleHolding::new(&context, input.second_token_holding, second_definition)?;
    let (stored_max_a, stored_max_b) =
        order.amounts_to_stored(input.max_first_amount, input.max_second_amount);
    let prepared = prepare_add_liquidity(snapshot, stored_max_a, stored_max_b, input.slippage)?;
    let (quoted_first, quoted_second) = order.amounts_from_stored(
        prepared.quote.actual_amount_a,
        prepared.quote.actual_amount_b,
    );
    ensure_funded(
        &first_holding,
        input.max_first_amount,
        "first token holding",
    )?;
    ensure_funded(
        &second_holding,
        input.max_second_amount,
        "second token holding",
    )?;
    let fresh_liquidity = validate_holding_destination(
        &context,
        input.liquidity_holding,
        snapshot.liquidity_definition().account_id(),
        "liquidity holding",
    )?;
    let (stored_holding_a, stored_holding_b) =
        stored_holdings(order, &first_holding, &second_holding);
    let pool = PoolContext::new(&context, snapshot.pool_id(), snapshot.pool())?;
    let plan = plan_add_liquidity(AddLiquidityPlanInput {
        context: &context,
        pool,
        user_holding_a: stored_holding_a.account_id(),
        user_holding_b: stored_holding_b.account_id(),
        user_holding_lp: input.liquidity_holding.account_id(),
        min_amount_liquidity: prepared.min_amount_liquidity,
        max_amount_to_add_token_a: prepared.max_amount_to_add_token_a,
        max_amount_to_add_token_b: prepared.max_amount_to_add_token_b,
        deadline: input.deadline,
    });
    let sources = pool_sources(
        input.pool_accounts,
        input.first_token_holding,
        input.second_token_holding,
        Some(input.liquidity_holding),
    );
    let funding = vec![
        funding(&first_holding, input.max_first_amount),
        funding(&second_holding, input.max_second_amount),
    ];
    let fresh = fresh_liquidity
        .then_some(input.liquidity_holding.account_id())
        .into_iter()
        .collect();

    PreparedTransaction::new(
        TransactionOperation::AddLiquidity,
        TransactionIntent::AddLiquidity {
            first_token_definition_id: input.first_token_definition_id,
            second_token_definition_id: input.second_token_definition_id,
            max_first_amount: input.max_first_amount,
            max_second_amount: input.max_second_amount,
            slippage_bps: input.slippage.bps(),
            expected_fee_bps: input.expected_fee_bps,
        },
        prepared.quote,
        plan,
        sources,
        fresh,
        funding,
        CallerAmounts::new(quoted_first, quoted_second),
        input.deadline,
        None,
    )
}

/// Quotes an LP burn and emits one exact remove plan.
pub fn prepare_remove_liquidity_transaction(
    input: RemoveLiquidityTransactionInput<'_>,
) -> Result<PreparedTransaction<RemoveLiquidityQuote>, TransactionError> {
    let (context, active) = input.pool_accounts.validate(
        input.amm_program_id,
        input.first_token_definition_id,
        input.second_token_definition_id,
    )?;
    let snapshot = active.pool();
    let order = active.caller_order();
    validate_expected_fee(snapshot, input.expected_fee_bps)?;
    let (first_definition, second_definition) = caller_definitions(snapshot, order);
    let first_fresh = validate_holding_destination(
        &context,
        input.first_token_holding,
        first_definition.account_id(),
        "first token holding",
    )?;
    let second_fresh = validate_holding_destination(
        &context,
        input.second_token_holding,
        second_definition.account_id(),
        "second token holding",
    )?;
    let liquidity_holding = ValidatedFungibleHolding::new(
        &context,
        input.liquidity_holding,
        snapshot.liquidity_definition(),
    )?;
    let prepared = prepare_remove_liquidity(
        snapshot,
        &liquidity_holding,
        input.remove_liquidity_amount,
        input.slippage,
    )?;
    let caller_amounts = order.amounts_from_stored(
        prepared.quote.withdraw_amount_a,
        prepared.quote.withdraw_amount_b,
    );
    let (stored_holding_a, stored_holding_b) = order_pair(
        order,
        input.first_token_holding.account_id(),
        input.second_token_holding.account_id(),
    );
    let pool = PoolContext::new(&context, snapshot.pool_id(), snapshot.pool())?;
    let plan = plan_remove_liquidity(RemoveLiquidityPlanInput {
        context: &context,
        pool,
        user_holding_a: stored_holding_a,
        user_holding_b: stored_holding_b,
        user_holding_lp: liquidity_holding.account_id(),
        remove_liquidity_amount: prepared.remove_liquidity_amount,
        min_amount_to_remove_token_a: prepared.min_amount_to_remove_token_a,
        min_amount_to_remove_token_b: prepared.min_amount_to_remove_token_b,
        deadline: input.deadline,
    });
    let sources = pool_sources(
        input.pool_accounts,
        input.first_token_holding,
        input.second_token_holding,
        Some(input.liquidity_holding),
    );
    let fresh = [
        first_fresh.then_some(input.first_token_holding.account_id()),
        second_fresh.then_some(input.second_token_holding.account_id()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let funding = vec![funding(
        &liquidity_holding,
        prepared.remove_liquidity_amount,
    )];

    PreparedTransaction::new(
        TransactionOperation::RemoveLiquidity,
        TransactionIntent::RemoveLiquidity {
            first_token_definition_id: input.first_token_definition_id,
            second_token_definition_id: input.second_token_definition_id,
            remove_liquidity_amount: input.remove_liquidity_amount,
            slippage_bps: input.slippage.bps(),
            expected_fee_bps: input.expected_fee_bps,
        },
        prepared.quote,
        plan,
        sources,
        fresh,
        funding,
        CallerAmounts::new(caller_amounts.0, caller_amounts.1),
        input.deadline,
        None,
    )
}

/// Quotes an exact-input swap and emits one exact swap plan.
pub fn prepare_swap_exact_input_transaction(
    input: SwapExactInputTransactionInput<'_>,
) -> Result<PreparedTransaction<SwapQuote>, TransactionError> {
    let (context, active) = input.pool_accounts.validate(
        input.amm_program_id,
        input.input_token_definition_id,
        input.output_token_definition_id,
    )?;
    let snapshot = active.pool();
    validate_expected_fee(snapshot, input.expected_fee_bps)?;
    let (input_definition, output_definition) = swap_definitions(
        snapshot,
        input.input_token_definition_id,
        input.output_token_definition_id,
    )?;
    let input_holding =
        ValidatedFungibleHolding::new(&context, input.input_holding, input_definition)?;
    let output_holding =
        ValidatedFungibleHolding::new(&context, input.output_holding, output_definition)?;
    let prepared = prepare_swap_exact_input(
        snapshot,
        &input_holding,
        &output_holding,
        input.amount_in,
        input.slippage,
    )?;
    let spot_change = pool_spot_change_bps(snapshot.pool(), &prepared.quote)?;
    let pool = PoolContext::new(&context, snapshot.pool_id(), snapshot.pool())?;
    let plan = plan_swap_exact_input(SwapExactInputPlanInput {
        context: &context,
        pool,
        user_input_holding: input_holding.account_id(),
        user_output_holding: output_holding.account_id(),
        swap_amount_in: prepared.swap_amount_in,
        min_amount_out: prepared.min_amount_out,
        deadline: input.deadline,
    });
    let sources = pool_sources(
        input.pool_accounts,
        input.input_holding,
        input.output_holding,
        None,
    );
    let funding = vec![funding(&input_holding, prepared.quote.amount_in)];

    PreparedTransaction::new(
        TransactionOperation::SwapExactInput,
        TransactionIntent::SwapExactInput {
            input_token_definition_id: input.input_token_definition_id,
            output_token_definition_id: input.output_token_definition_id,
            amount_in: input.amount_in,
            slippage_bps: input.slippage.bps(),
            expected_fee_bps: input.expected_fee_bps,
        },
        prepared.quote,
        plan,
        sources,
        Vec::new(),
        funding,
        CallerAmounts::new(prepared.quote.amount_in, prepared.quote.amount_out),
        input.deadline,
        Some(spot_change),
    )
}

/// Quotes an exact-output swap and emits one exact swap plan.
pub fn prepare_swap_exact_output_transaction(
    input: SwapExactOutputTransactionInput<'_>,
) -> Result<PreparedTransaction<SwapQuote>, TransactionError> {
    let (context, active) = input.pool_accounts.validate(
        input.amm_program_id,
        input.input_token_definition_id,
        input.output_token_definition_id,
    )?;
    let snapshot = active.pool();
    validate_expected_fee(snapshot, input.expected_fee_bps)?;
    let (input_definition, output_definition) = swap_definitions(
        snapshot,
        input.input_token_definition_id,
        input.output_token_definition_id,
    )?;
    let input_holding =
        ValidatedFungibleHolding::new(&context, input.input_holding, input_definition)?;
    let output_holding =
        ValidatedFungibleHolding::new(&context, input.output_holding, output_definition)?;
    let prepared = prepare_swap_exact_output(
        snapshot,
        &input_holding,
        &output_holding,
        input.exact_amount_out,
        input.slippage,
    )?;
    let spot_change = pool_spot_change_bps(snapshot.pool(), &prepared.quote)?;
    let pool = PoolContext::new(&context, snapshot.pool_id(), snapshot.pool())?;
    let plan = plan_swap_exact_output(SwapExactOutputPlanInput {
        context: &context,
        pool,
        user_input_holding: input_holding.account_id(),
        user_output_holding: output_holding.account_id(),
        exact_amount_out: prepared.exact_amount_out,
        max_amount_in: prepared.max_amount_in,
        deadline: input.deadline,
    });
    let sources = pool_sources(
        input.pool_accounts,
        input.input_holding,
        input.output_holding,
        None,
    );
    let funding = vec![funding(&input_holding, prepared.max_amount_in)];

    PreparedTransaction::new(
        TransactionOperation::SwapExactOutput,
        TransactionIntent::SwapExactOutput {
            input_token_definition_id: input.input_token_definition_id,
            output_token_definition_id: input.output_token_definition_id,
            exact_amount_out: input.exact_amount_out,
            slippage_bps: input.slippage.bps(),
            expected_fee_bps: input.expected_fee_bps,
        },
        prepared.quote,
        plan,
        sources,
        Vec::new(),
        funding,
        CallerAmounts::new(prepared.quote.amount_in, prepared.quote.amount_out),
        input.deadline,
        Some(spot_change),
    )
}

impl<Q> PreparedTransaction<Q> {
    #[expect(
        clippy::too_many_arguments,
        reason = "prepared transaction construction binds each externally visible artifact"
    )]
    fn new(
        operation: TransactionOperation,
        intent: TransactionIntent,
        quote: Q,
        plan: TransactionPlan,
        sources: Vec<SnapshotCommitment>,
        fresh_account_ids: Vec<AccountId>,
        funding: Vec<FundingRequirement>,
        caller_amounts: CallerAmounts,
        deadline: u64,
        pool_spot_change_bps: Option<u128>,
    ) -> Result<Self, TransactionError> {
        let mut unique_account_ids = Vec::new();
        for account_id in plan.account_ids() {
            if unique_account_ids.contains(&account_id) {
                return Err(TransactionError::DuplicateAccountId { account_id });
            }
            unique_account_ids.push(account_id);
        }

        let instruction_words = plan
            .instruction_data()
            .map_err(|_| TransactionError::InstructionEncoding)?;
        let plan_accounts = plan
            .accounts()
            .iter()
            .map(|account| PlanAccountCommitment {
                role: String::from(account.role().as_str()),
                account_id: account.id(),
                writable: account.writable(),
                signer: account.signer(),
                init: account.init(),
            })
            .collect();
        let payload = PreparedCommitmentPayload {
            domain: String::from(COMMITMENT_DOMAIN),
            operation,
            intent,
            program_id: plan.program_id(),
            instruction_words,
            plan_accounts,
            sources,
            caller_amounts,
            deadline,
        };
        let words = risc0_zkvm::serde::to_vec(&payload)
            .map_err(|_| TransactionError::CommitmentEncoding)?;
        let mut bytes = Vec::with_capacity(words.len().saturating_mul(4));
        for word in words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let digest = Impl::hash_bytes(&bytes);
        let mut commitment_bytes = [0_u8; 32];
        commitment_bytes.copy_from_slice(digest.as_bytes());
        let affected_account_ids = plan.affected_account_ids();
        let wallet_prerequisites = WalletPrerequisites {
            signer_account_ids: plan.signer_account_ids(),
            fresh_account_ids,
            funding,
        };

        Ok(Self {
            operation,
            quote,
            plan,
            quote_commitment: QuoteCommitment(commitment_bytes),
            affected_account_ids,
            wallet_prerequisites,
            caller_amounts,
            deadline,
            pool_spot_change_bps,
        })
    }
}

#[derive(Serialize)]
struct PreparedCommitmentPayload {
    domain: String,
    operation: TransactionOperation,
    intent: TransactionIntent,
    program_id: ProgramId,
    instruction_words: Vec<u32>,
    plan_accounts: Vec<PlanAccountCommitment>,
    sources: Vec<SnapshotCommitment>,
    caller_amounts: CallerAmounts,
    deadline: u64,
}

#[derive(Serialize)]
enum TransactionIntent {
    CreatePool {
        first_token_definition_id: AccountId,
        second_token_definition_id: AccountId,
        first_amount: u128,
        second_amount: u128,
        fee_bps: u128,
    },
    AddLiquidity {
        first_token_definition_id: AccountId,
        second_token_definition_id: AccountId,
        max_first_amount: u128,
        max_second_amount: u128,
        slippage_bps: u128,
        expected_fee_bps: Option<u128>,
    },
    RemoveLiquidity {
        first_token_definition_id: AccountId,
        second_token_definition_id: AccountId,
        remove_liquidity_amount: u128,
        slippage_bps: u128,
        expected_fee_bps: Option<u128>,
    },
    SwapExactInput {
        input_token_definition_id: AccountId,
        output_token_definition_id: AccountId,
        amount_in: u128,
        slippage_bps: u128,
        expected_fee_bps: Option<u128>,
    },
    SwapExactOutput {
        input_token_definition_id: AccountId,
        output_token_definition_id: AccountId,
        exact_amount_out: u128,
        slippage_bps: u128,
        expected_fee_bps: Option<u128>,
    },
}

#[derive(Serialize)]
struct PlanAccountCommitment {
    role: String,
    account_id: AccountId,
    writable: bool,
    signer: bool,
    init: bool,
}

#[derive(Clone, Copy, Serialize)]
enum SnapshotRole {
    Config,
    Pool,
    CallerFirstDefinition,
    CallerSecondDefinition,
    CallerFirstVault,
    CallerSecondVault,
    LiquidityDefinition,
    LpLockHolding,
    CallerFirstHolding,
    CallerSecondHolding,
    LiquidityHolding,
}

#[derive(Serialize)]
struct SnapshotCommitment {
    role: SnapshotRole,
    account_id: AccountId,
    commitment: [u8; 32],
}

fn source(role: SnapshotRole, snapshot: &AccountSnapshot) -> SnapshotCommitment {
    SnapshotCommitment {
        role,
        account_id: snapshot.account_id(),
        commitment: Commitment::new(&snapshot.account_id(), snapshot.account()).to_byte_array(),
    }
}

fn pool_sources(
    pool: PoolAccountSnapshots<'_>,
    first_holding: &AccountSnapshot,
    second_holding: &AccountSnapshot,
    liquidity_holding: Option<&AccountSnapshot>,
) -> Vec<SnapshotCommitment> {
    pair_sources(
        pool.config,
        pool.pair,
        first_holding,
        second_holding,
        liquidity_holding,
    )
}

fn pair_sources(
    config: &AccountSnapshot,
    pair: PairReadSnapshots<'_>,
    first_holding: &AccountSnapshot,
    second_holding: &AccountSnapshot,
    liquidity_holding: Option<&AccountSnapshot>,
) -> Vec<SnapshotCommitment> {
    let mut sources = vec![
        source(SnapshotRole::Config, config),
        source(SnapshotRole::Pool, pair.pool),
        source(
            SnapshotRole::CallerFirstDefinition,
            pair.first_token_definition,
        ),
        source(
            SnapshotRole::CallerSecondDefinition,
            pair.second_token_definition,
        ),
        source(SnapshotRole::CallerFirstVault, pair.first_token_vault),
        source(SnapshotRole::CallerSecondVault, pair.second_token_vault),
        source(SnapshotRole::LiquidityDefinition, pair.liquidity_definition),
        source(SnapshotRole::LpLockHolding, pair.lp_lock_holding),
        source(SnapshotRole::CallerFirstHolding, first_holding),
        source(SnapshotRole::CallerSecondHolding, second_holding),
    ];
    if let Some(liquidity_holding) = liquidity_holding {
        sources.push(source(SnapshotRole::LiquidityHolding, liquidity_holding));
    }
    sources
}

fn caller_definitions(
    snapshot: &ValidatedPoolSnapshot,
    order: PairOrder,
) -> (&ValidatedFungibleDefinition, &ValidatedFungibleDefinition) {
    match order {
        PairOrder::Stored => (snapshot.token_a_definition(), snapshot.token_b_definition()),
        PairOrder::Reversed => (snapshot.token_b_definition(), snapshot.token_a_definition()),
    }
}

fn validate_expected_fee(
    snapshot: &ValidatedPoolSnapshot,
    expected_fee_bps: Option<u128>,
) -> Result<(), TransactionError> {
    if let Some(expected) = expected_fee_bps {
        let actual = snapshot.pool().fees;
        if expected != actual {
            return Err(TransactionError::FeeMismatch { expected, actual });
        }
    }
    Ok(())
}

fn stored_holdings<'a>(
    order: PairOrder,
    first: &'a ValidatedFungibleHolding,
    second: &'a ValidatedFungibleHolding,
) -> (&'a ValidatedFungibleHolding, &'a ValidatedFungibleHolding) {
    match order {
        PairOrder::Stored => (first, second),
        PairOrder::Reversed => (second, first),
    }
}

fn order_pair<T>(order: PairOrder, first: T, second: T) -> (T, T) {
    match order {
        PairOrder::Stored => (first, second),
        PairOrder::Reversed => (second, first),
    }
}

fn swap_definitions(
    snapshot: &ValidatedPoolSnapshot,
    input_definition_id: AccountId,
    output_definition_id: AccountId,
) -> Result<(&ValidatedFungibleDefinition, &ValidatedFungibleDefinition), ClientError> {
    match amm_program::quote::pair_order(
        snapshot.pool(),
        input_definition_id,
        output_definition_id,
    )? {
        PairOrder::Stored => Ok((snapshot.token_a_definition(), snapshot.token_b_definition())),
        PairOrder::Reversed => Ok((snapshot.token_b_definition(), snapshot.token_a_definition())),
    }
}

fn validate_holding_destination(
    context: &AmmContext,
    snapshot: &AccountSnapshot,
    expected_definition_id: AccountId,
    account_name: &'static str,
) -> Result<bool, ClientError> {
    if snapshot.account() == &Account::default() {
        return Ok(true);
    }
    if snapshot.account().program_owner != context.token_program_id() {
        return Err(ClientError::ProgramOwnerMismatch {
            account: account_name,
            expected: context.token_program_id(),
            actual: snapshot.account().program_owner,
        });
    }
    let holding = TokenHolding::try_from(&snapshot.account().data).map_err(|_| {
        ClientError::InvalidAccountData {
            account: account_name,
            expected: "fungible TokenHolding",
        }
    })?;
    let TokenHolding::Fungible { definition_id, .. } = holding else {
        return Err(ClientError::ExpectedFungibleToken {
            account: account_name,
        });
    };
    if definition_id != expected_definition_id {
        return Err(ClientError::TokenDefinitionMismatch {
            account: account_name,
            expected: expected_definition_id,
            actual: definition_id,
        });
    }
    Ok(false)
}

fn ensure_funded(
    holding: &ValidatedFungibleHolding,
    required: u128,
    account_name: &'static str,
) -> Result<(), ClientError> {
    if holding.balance() < required {
        return Err(ClientError::InsufficientBalance {
            account: account_name,
            available: holding.balance(),
            required,
        });
    }
    Ok(())
}

fn funding(holding: &ValidatedFungibleHolding, required: u128) -> FundingRequirement {
    FundingRequirement {
        holding_account_id: holding.account_id(),
        token_definition_id: holding.definition_id(),
        available: holding.balance(),
        required,
    }
}
