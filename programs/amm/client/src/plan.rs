use amm_core::{
    compute_config_pda, compute_liquidity_token_pda, compute_lp_lock_holding_pda, compute_pool_pda,
    compute_vault_pda, AmmConfig, Instruction, PoolDefinition,
};
use clock_core::CLOCK_01_PROGRAM_ACCOUNT_ID;
use nssa_core::{
    account::AccountId,
    program::{InstructionData, ProgramId},
};
use twap_oracle_core::{
    compute_current_tick_account_pda, compute_oracle_price_account_pda,
    compute_price_observations_pda,
};

use crate::ClientError;

/// Configured AMM program context used by deterministic planners.
///
/// `amm_program_id` is accepted optimistically. The client derives addresses for that program but
/// does not perform release, ImageID, or deployment-version checks.
#[derive(Clone)]
pub struct AmmContext {
    pub amm_program_id: ProgramId,
    pub config: AmmConfig,
}

impl AmmContext {
    #[must_use]
    pub const fn new(amm_program_id: ProgramId, config: AmmConfig) -> Self {
        Self {
            amm_program_id,
            config,
        }
    }

    #[must_use]
    pub fn config_id(&self) -> AccountId {
        compute_config_pda(self.amm_program_id)
    }

    #[must_use]
    pub const fn token_program_id(&self) -> ProgramId {
        self.config.token_program_id
    }

    #[must_use]
    pub const fn twap_oracle_program_id(&self) -> ProgramId {
        self.config.twap_oracle_program_id
    }
}

/// An initialized pool and its canonical stored identity fields.
#[derive(Clone, Copy)]
pub struct PoolContext<'a> {
    pool_id: AccountId,
    pool: &'a PoolDefinition,
}

impl<'a> PoolContext<'a> {
    /// Validates the stored pool identity fields against canonical AMM PDA derivation.
    pub fn new(
        context: &AmmContext,
        pool_id: AccountId,
        pool: &'a PoolDefinition,
    ) -> Result<Self, ClientError> {
        if pool.definition_token_a_id == pool.definition_token_b_id {
            return Err(ClientError::IdenticalTokenDefinitions);
        }

        validate_account_id(
            "pool",
            compute_pool_pda(
                context.amm_program_id,
                pool.definition_token_a_id,
                pool.definition_token_b_id,
            ),
            pool_id,
        )?;
        validate_account_id(
            "vault_a",
            compute_vault_pda(context.amm_program_id, pool_id, pool.definition_token_a_id),
            pool.vault_a_id,
        )?;
        validate_account_id(
            "vault_b",
            compute_vault_pda(context.amm_program_id, pool_id, pool.definition_token_b_id),
            pool.vault_b_id,
        )?;
        validate_account_id(
            "pool_definition_lp",
            compute_liquidity_token_pda(context.amm_program_id, pool_id),
            pool.liquidity_pool_id,
        )?;

        Ok(Self { pool_id, pool })
    }

    #[must_use]
    pub const fn pool_id(&self) -> AccountId {
        self.pool_id
    }

    #[must_use]
    pub const fn pool(&self) -> &PoolDefinition {
        self.pool
    }
}

/// Semantic name of an account in an AMM instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AccountRole {
    Config,
    Authority,
    Pool,
    VaultA,
    VaultB,
    PoolDefinitionLp,
    LpLockHolding,
    UserHoldingA,
    UserHoldingB,
    UserHoldingLp,
    UserInputHolding,
    UserOutputHolding,
    CurrentTickAccount,
    PriceObservations,
    OraclePriceAccount,
    Clock,
}

impl AccountRole {
    /// Exact role name emitted by the AMM IDL.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Authority => "authority",
            Self::Pool => "pool",
            Self::VaultA => "vault_a",
            Self::VaultB => "vault_b",
            Self::PoolDefinitionLp => "pool_definition_lp",
            Self::LpLockHolding => "lp_lock_holding",
            Self::UserHoldingA => "user_holding_a",
            Self::UserHoldingB => "user_holding_b",
            Self::UserHoldingLp => "user_holding_lp",
            Self::UserInputHolding => "user_input_holding",
            Self::UserOutputHolding => "user_output_holding",
            Self::CurrentTickAccount => "current_tick_account",
            Self::PriceObservations => "price_observations",
            Self::OraclePriceAccount => "oracle_price_account",
            Self::Clock => "clock",
        }
    }
}

/// Ordered account row required by an AMM instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlannedAccount {
    id: AccountId,
    role: AccountRole,
    writable: bool,
    signer: bool,
    init: bool,
}

impl PlannedAccount {
    #[must_use]
    pub const fn id(&self) -> AccountId {
        self.id
    }

    #[must_use]
    pub const fn role(&self) -> AccountRole {
        self.role
    }

    #[must_use]
    pub const fn writable(&self) -> bool {
        self.writable
    }

    #[must_use]
    pub const fn signer(&self) -> bool {
        self.signer
    }

    #[must_use]
    pub const fn init(&self) -> bool {
        self.init
    }
}

/// Canonical instruction plus ordered accounts for wallet submission.
pub struct TransactionPlan {
    program_id: ProgramId,
    instruction: Instruction,
    accounts: Vec<PlannedAccount>,
}

impl TransactionPlan {
    fn new(program_id: ProgramId, instruction: Instruction, accounts: Vec<PlannedAccount>) -> Self {
        Self {
            program_id,
            instruction,
            accounts,
        }
    }

    #[must_use]
    pub const fn program_id(&self) -> ProgramId {
        self.program_id
    }

    #[must_use]
    pub const fn instruction(&self) -> &Instruction {
        &self.instruction
    }

    /// Exact guest-compatible RISC Zero Serde instruction words.
    pub fn instruction_data(&self) -> risc0_zkvm::serde::Result<InstructionData> {
        encode_instruction(&self.instruction)
    }

    #[must_use]
    pub fn accounts(&self) -> &[PlannedAccount] {
        &self.accounts
    }

    #[must_use]
    pub fn account_ids(&self) -> Vec<AccountId> {
        self.accounts.iter().map(PlannedAccount::id).collect()
    }

    /// One signer requirement for each ordered account ID.
    #[must_use]
    pub fn signer_flags(&self) -> Vec<bool> {
        self.accounts.iter().map(PlannedAccount::signer).collect()
    }

    /// Signer IDs in their original account-list order.
    #[must_use]
    pub fn signer_account_ids(&self) -> Vec<AccountId> {
        self.accounts
            .iter()
            .filter(|account| account.signer())
            .map(PlannedAccount::id)
            .collect()
    }

    /// Writable account IDs in first-occurrence instruction order.
    #[must_use]
    pub fn writable_account_ids(&self) -> Vec<AccountId> {
        self.accounts
            .iter()
            .filter(|account| account.writable())
            .map(PlannedAccount::id)
            .fold(Vec::new(), |mut ids, id| {
                if !ids.contains(&id) {
                    ids.push(id);
                }
                ids
            })
    }

    /// Account IDs whose state may change if the instruction succeeds.
    #[must_use]
    pub fn affected_account_ids(&self) -> Vec<AccountId> {
        self.writable_account_ids()
    }

    /// Guest instruction name, kept exhaustive over the canonical enum.
    #[must_use]
    pub const fn instruction_name(&self) -> &'static str {
        match &self.instruction {
            Instruction::Initialize { .. } => "initialize",
            Instruction::UpdateConfig { .. } => "update_config",
            Instruction::CreatePriceObservations { .. } => "create_price_observations",
            Instruction::CreateOraclePriceAccount { .. } => "create_oracle_price_account",
            Instruction::NewDefinition { .. } => "new_definition",
            Instruction::AddLiquidity { .. } => "add_liquidity",
            Instruction::RemoveLiquidity { .. } => "remove_liquidity",
            Instruction::SwapExactInput { .. } => "swap_exact_input",
            Instruction::SwapExactOutput { .. } => "swap_exact_output",
            Instruction::SyncReserves => "sync_reserves",
        }
    }
}

/// Encode the actual instruction enum through the codec consumed by the AMM guest.
pub fn encode_instruction(instruction: &Instruction) -> risc0_zkvm::serde::Result<InstructionData> {
    risc0_zkvm::serde::to_vec(instruction)
}

pub struct InitializePlanInput {
    pub amm_program_id: ProgramId,
    pub token_program_id: ProgramId,
    pub twap_oracle_program_id: ProgramId,
    pub authority: AccountId,
}

pub struct UpdateConfigPlanInput<'a> {
    pub context: &'a AmmContext,
    pub token_program_id: Option<ProgramId>,
    pub twap_oracle_program_id: Option<ProgramId>,
    pub new_authority: Option<AccountId>,
}

pub struct CreatePriceObservationsPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool_id: AccountId,
    pub window_duration: u64,
}

pub struct CreateOraclePriceAccountPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool_id: AccountId,
    pub window_duration: u64,
}

pub struct CreatePoolPlanInput<'a> {
    pub context: &'a AmmContext,
    pub token_a_definition_id: AccountId,
    pub token_b_definition_id: AccountId,
    pub user_holding_a: AccountId,
    pub user_holding_b: AccountId,
    pub user_holding_lp: AccountId,
    pub token_a_amount: u128,
    pub token_b_amount: u128,
    pub fees: u128,
    pub deadline: u64,
}

pub struct AddLiquidityPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool: PoolContext<'a>,
    pub user_holding_a: AccountId,
    pub user_holding_b: AccountId,
    pub user_holding_lp: AccountId,
    pub min_amount_liquidity: u128,
    pub max_amount_to_add_token_a: u128,
    pub max_amount_to_add_token_b: u128,
    pub deadline: u64,
}

pub struct RemoveLiquidityPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool: PoolContext<'a>,
    pub user_holding_a: AccountId,
    pub user_holding_b: AccountId,
    pub user_holding_lp: AccountId,
    pub remove_liquidity_amount: u128,
    pub min_amount_to_remove_token_a: u128,
    pub min_amount_to_remove_token_b: u128,
    pub deadline: u64,
}

pub struct SwapExactInputPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool: PoolContext<'a>,
    pub user_input_holding: AccountId,
    pub user_output_holding: AccountId,
    pub swap_amount_in: u128,
    pub min_amount_out: u128,
    pub deadline: u64,
}

pub struct SwapExactOutputPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool: PoolContext<'a>,
    pub user_input_holding: AccountId,
    pub user_output_holding: AccountId,
    pub exact_amount_out: u128,
    pub max_amount_in: u128,
    pub deadline: u64,
}

pub struct SyncReservesPlanInput<'a> {
    pub context: &'a AmmContext,
    pub pool: PoolContext<'a>,
}

#[must_use]
pub fn plan_initialize(input: InitializePlanInput) -> TransactionPlan {
    TransactionPlan::new(
        input.amm_program_id,
        Instruction::Initialize {
            token_program_id: input.token_program_id,
            twap_oracle_program_id: input.twap_oracle_program_id,
            authority: input.authority,
        },
        vec![planned(
            compute_config_pda(input.amm_program_id),
            AccountRole::Config,
            true,
            false,
            true,
        )],
    )
}

#[must_use]
pub fn plan_update_config(input: UpdateConfigPlanInput<'_>) -> TransactionPlan {
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::UpdateConfig {
            token_program_id: input.token_program_id,
            twap_oracle_program_id: input.twap_oracle_program_id,
            new_authority: input.new_authority,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                true,
                false,
                false,
            ),
            planned(
                input.context.config.authority,
                AccountRole::Authority,
                false,
                true,
                false,
            ),
        ],
    )
}

#[must_use]
pub fn plan_create_price_observations(
    input: CreatePriceObservationsPlanInput<'_>,
) -> TransactionPlan {
    let oracle_program_id = input.context.twap_oracle_program_id();
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::CreatePriceObservations {
            window_duration: input.window_duration,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(input.pool_id, AccountRole::Pool, false, false, false),
            planned(
                compute_current_tick_account_pda(oracle_program_id, input.pool_id),
                AccountRole::CurrentTickAccount,
                false,
                false,
                false,
            ),
            planned(
                compute_price_observations_pda(
                    oracle_program_id,
                    input.pool_id,
                    input.window_duration,
                ),
                AccountRole::PriceObservations,
                true,
                false,
                true,
            ),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

#[must_use]
pub fn plan_create_oracle_price_account(
    input: CreateOraclePriceAccountPlanInput<'_>,
) -> TransactionPlan {
    let oracle_program_id = input.context.twap_oracle_program_id();
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::CreateOraclePriceAccount {
            window_duration: input.window_duration,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(input.pool_id, AccountRole::Pool, false, false, false),
            planned(
                compute_oracle_price_account_pda(
                    oracle_program_id,
                    input.pool_id,
                    input.window_duration,
                ),
                AccountRole::OraclePriceAccount,
                true,
                false,
                true,
            ),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

pub fn plan_create_pool(input: CreatePoolPlanInput<'_>) -> Result<TransactionPlan, ClientError> {
    if input.token_a_definition_id == input.token_b_definition_id {
        return Err(ClientError::IdenticalTokenDefinitions);
    }

    let program_id = input.context.amm_program_id;
    let pool_id = compute_pool_pda(
        program_id,
        input.token_a_definition_id,
        input.token_b_definition_id,
    );
    let vault_a = compute_vault_pda(program_id, pool_id, input.token_a_definition_id);
    let vault_b = compute_vault_pda(program_id, pool_id, input.token_b_definition_id);
    let liquidity_token = compute_liquidity_token_pda(program_id, pool_id);
    let lock_holding = compute_lp_lock_holding_pda(program_id, pool_id);
    let current_tick =
        compute_current_tick_account_pda(input.context.twap_oracle_program_id(), pool_id);

    Ok(TransactionPlan::new(
        program_id,
        Instruction::NewDefinition {
            token_a_amount: input.token_a_amount,
            token_b_amount: input.token_b_amount,
            fees: input.fees,
            deadline: input.deadline,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(pool_id, AccountRole::Pool, true, false, true),
            planned(vault_a, AccountRole::VaultA, true, false, false),
            planned(vault_b, AccountRole::VaultB, true, false, false),
            planned(
                liquidity_token,
                AccountRole::PoolDefinitionLp,
                true,
                false,
                true,
            ),
            planned(lock_holding, AccountRole::LpLockHolding, true, false, true),
            planned(
                input.user_holding_a,
                AccountRole::UserHoldingA,
                true,
                true,
                false,
            ),
            planned(
                input.user_holding_b,
                AccountRole::UserHoldingB,
                true,
                true,
                false,
            ),
            planned(
                input.user_holding_lp,
                AccountRole::UserHoldingLp,
                true,
                true,
                false,
            ),
            planned(
                current_tick,
                AccountRole::CurrentTickAccount,
                true,
                false,
                true,
            ),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    ))
}

#[must_use]
pub fn plan_add_liquidity(input: AddLiquidityPlanInput<'_>) -> TransactionPlan {
    let tick = current_tick(input.context, input.pool.pool_id);
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::AddLiquidity {
            min_amount_liquidity: input.min_amount_liquidity,
            max_amount_to_add_token_a: input.max_amount_to_add_token_a,
            max_amount_to_add_token_b: input.max_amount_to_add_token_b,
            deadline: input.deadline,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(input.pool.pool_id, AccountRole::Pool, true, false, false),
            planned(
                input.pool.pool.vault_a_id,
                AccountRole::VaultA,
                true,
                false,
                false,
            ),
            planned(
                input.pool.pool.vault_b_id,
                AccountRole::VaultB,
                true,
                false,
                false,
            ),
            planned(
                input.pool.pool.liquidity_pool_id,
                AccountRole::PoolDefinitionLp,
                true,
                false,
                false,
            ),
            planned(
                input.user_holding_a,
                AccountRole::UserHoldingA,
                true,
                true,
                false,
            ),
            planned(
                input.user_holding_b,
                AccountRole::UserHoldingB,
                true,
                true,
                false,
            ),
            planned(
                input.user_holding_lp,
                AccountRole::UserHoldingLp,
                true,
                false,
                false,
            ),
            planned(tick, AccountRole::CurrentTickAccount, true, false, false),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

#[must_use]
pub fn plan_remove_liquidity(input: RemoveLiquidityPlanInput<'_>) -> TransactionPlan {
    let tick = current_tick(input.context, input.pool.pool_id);
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::RemoveLiquidity {
            remove_liquidity_amount: input.remove_liquidity_amount,
            min_amount_to_remove_token_a: input.min_amount_to_remove_token_a,
            min_amount_to_remove_token_b: input.min_amount_to_remove_token_b,
            deadline: input.deadline,
        },
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(input.pool.pool_id, AccountRole::Pool, true, false, false),
            planned(
                input.pool.pool.vault_a_id,
                AccountRole::VaultA,
                true,
                false,
                false,
            ),
            planned(
                input.pool.pool.vault_b_id,
                AccountRole::VaultB,
                true,
                false,
                false,
            ),
            planned(
                input.pool.pool.liquidity_pool_id,
                AccountRole::PoolDefinitionLp,
                true,
                false,
                false,
            ),
            planned(
                input.user_holding_a,
                AccountRole::UserHoldingA,
                true,
                false,
                false,
            ),
            planned(
                input.user_holding_b,
                AccountRole::UserHoldingB,
                true,
                false,
                false,
            ),
            planned(
                input.user_holding_lp,
                AccountRole::UserHoldingLp,
                true,
                true,
                false,
            ),
            planned(tick, AccountRole::CurrentTickAccount, true, false, false),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

#[must_use]
pub fn plan_swap_exact_input(input: SwapExactInputPlanInput<'_>) -> TransactionPlan {
    swap_plan(
        input.context,
        input.pool,
        input.user_input_holding,
        input.user_output_holding,
        Instruction::SwapExactInput {
            swap_amount_in: input.swap_amount_in,
            min_amount_out: input.min_amount_out,
            deadline: input.deadline,
        },
    )
}

#[must_use]
pub fn plan_swap_exact_output(input: SwapExactOutputPlanInput<'_>) -> TransactionPlan {
    swap_plan(
        input.context,
        input.pool,
        input.user_input_holding,
        input.user_output_holding,
        Instruction::SwapExactOutput {
            exact_amount_out: input.exact_amount_out,
            max_amount_in: input.max_amount_in,
            deadline: input.deadline,
        },
    )
}

#[must_use]
pub fn plan_sync_reserves(input: SyncReservesPlanInput<'_>) -> TransactionPlan {
    TransactionPlan::new(
        input.context.amm_program_id,
        Instruction::SyncReserves,
        vec![
            planned(
                input.context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(input.pool.pool_id, AccountRole::Pool, true, false, false),
            planned(
                input.pool.pool.vault_a_id,
                AccountRole::VaultA,
                false,
                false,
                false,
            ),
            planned(
                input.pool.pool.vault_b_id,
                AccountRole::VaultB,
                false,
                false,
                false,
            ),
            planned(
                current_tick(input.context, input.pool.pool_id),
                AccountRole::CurrentTickAccount,
                true,
                false,
                false,
            ),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

fn swap_plan(
    context: &AmmContext,
    pool: PoolContext<'_>,
    input_holding: AccountId,
    output_holding: AccountId,
    instruction: Instruction,
) -> TransactionPlan {
    TransactionPlan::new(
        context.amm_program_id,
        instruction,
        vec![
            planned(
                context.config_id(),
                AccountRole::Config,
                false,
                false,
                false,
            ),
            planned(pool.pool_id, AccountRole::Pool, true, false, false),
            planned(
                pool.pool.vault_a_id,
                AccountRole::VaultA,
                true,
                false,
                false,
            ),
            planned(
                pool.pool.vault_b_id,
                AccountRole::VaultB,
                true,
                false,
                false,
            ),
            planned(
                input_holding,
                AccountRole::UserInputHolding,
                true,
                true,
                false,
            ),
            planned(
                output_holding,
                AccountRole::UserOutputHolding,
                true,
                false,
                false,
            ),
            planned(
                current_tick(context, pool.pool_id),
                AccountRole::CurrentTickAccount,
                true,
                false,
                false,
            ),
            planned(
                CLOCK_01_PROGRAM_ACCOUNT_ID,
                AccountRole::Clock,
                false,
                false,
                false,
            ),
        ],
    )
}

fn current_tick(context: &AmmContext, pool_id: AccountId) -> AccountId {
    compute_current_tick_account_pda(context.twap_oracle_program_id(), pool_id)
}

fn validate_account_id(
    account: &'static str,
    expected: AccountId,
    actual: AccountId,
) -> Result<(), ClientError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ClientError::AccountIdMismatch {
            account,
            expected,
            actual,
        })
    }
}

const fn planned(
    id: AccountId,
    role: AccountRole,
    writable: bool,
    signer: bool,
    init: bool,
) -> PlannedAccount {
    PlannedAccount {
        id,
        role,
        writable,
        signer,
        init,
    }
}
