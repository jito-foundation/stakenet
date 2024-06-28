use clap::{arg, command, Parser, Subcommand};
use jito_steward::UpdateParametersArgs;
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "CLI for the steward program")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub json_rpc_url: String,

    /// Steward program ID
    #[arg(
        long,
        env,
        default_value_t = jito_steward::id()
    )]
    pub program_id: Pubkey,

    #[command(subcommand)]
    pub commands: Commands,
}

// ---------- Meta Parameters ------------
#[derive(Parser)]
pub struct ConfigParameters {
    /// Number of recent epochs used to evaluate MEV commissions and running Jito for scoring
    #[arg(long, env)]
    pub mev_commission_range: Option<u16>,

    /// Number of recent epochs used to evaluate yield
    #[arg(long, env)]
    pub epoch_credits_range: Option<u16>,

    /// Number of recent epochs used to evaluate commissions for scoring
    #[arg(long, env)]
    pub commission_range: Option<u16>,

    /// Minimum ratio of slots voted on for each epoch for a validator to be eligible for stake. Used as proxy for validator reliability/restart timeliness. Ratio is number of epoch_credits / blocks_produced
    #[arg(long, env)]
    pub scoring_delinquency_threshold_ratio: Option<f64>,

    /// Same as scoring_delinquency_threshold_ratio but evaluated every epoch
    #[arg(long, env)]
    pub instant_unstake_delinquency_threshold_ratio: Option<f64>,

    /// Maximum allowable MEV commission in mev_commission_range (stored in basis points)
    #[arg(long, env)]
    pub mev_commission_bps_threshold: Option<u16>,

    /// Maximum allowable validator commission in commission_range (stored in percent)
    #[arg(long, env)]
    pub commission_threshold: Option<u8>,

    /// Maximum allowable validator commission in all history (stored in percent)
    #[arg(long, env)]
    pub historical_commission_threshold: Option<u8>,

    /// Number of validators who are eligible for stake (validator set size)
    #[arg(long, env)]
    pub num_delegation_validators: Option<u32>,

    /// Percent of total pool lamports that can be unstaked due to new delegation set (in basis points)
    #[arg(long, env)]
    pub scoring_unstake_cap_bps: Option<u32>,

    /// Percent of total pool lamports that can be unstaked due to instant unstaking (in basis points)
    #[arg(long, env)]
    pub instant_unstake_cap_bps: Option<u32>,

    /// Percent of total pool lamports that can be unstaked due to stake deposits above target lamports (in basis points)
    #[arg(long, env)]
    pub stake_deposit_unstake_cap_bps: Option<u32>,

    /// Scoring window such that the validators are all scored within a similar timeframe (in slots)
    #[arg(long, env)]
    pub compute_score_slot_range: Option<u64>,

    /// Point in epoch progress before instant unstake can be computed
    #[arg(long, env)]
    pub instant_unstake_epoch_progress: Option<f64>,

    /// Inputs to “Compute Instant Unstake” need to be updated past this point in epoch progress
    #[arg(long, env)]
    pub instant_unstake_inputs_epoch_progress: Option<f64>,

    /// Cycle length - Number of epochs to run the Monitor->Rebalance loop
    #[arg(long, env)]
    pub num_epochs_between_scoring: Option<u64>,

    /// Minimum number of stake lamports for a validator to be considered for the pool
    #[arg(long, env)]
    pub minimum_stake_lamports: Option<u64>,

    /// Minimum number of consecutive epochs a validator has to vote before it can be considered for the pool
    #[arg(long, env)]
    pub minimum_voting_epochs: Option<u64>,
}

impl From<ConfigParameters> for UpdateParametersArgs {
    fn from(config: ConfigParameters) -> Self {
        UpdateParametersArgs {
            mev_commission_range: config.mev_commission_range,
            epoch_credits_range: config.epoch_credits_range,
            commission_range: config.commission_range,
            scoring_delinquency_threshold_ratio: config.scoring_delinquency_threshold_ratio,
            instant_unstake_delinquency_threshold_ratio: config
                .instant_unstake_delinquency_threshold_ratio,
            mev_commission_bps_threshold: config.mev_commission_bps_threshold,
            commission_threshold: config.commission_threshold,
            historical_commission_threshold: config.historical_commission_threshold,
            num_delegation_validators: config.num_delegation_validators,
            scoring_unstake_cap_bps: config.scoring_unstake_cap_bps,
            instant_unstake_cap_bps: config.instant_unstake_cap_bps,
            stake_deposit_unstake_cap_bps: config.stake_deposit_unstake_cap_bps,
            compute_score_slot_range: config.compute_score_slot_range,
            instant_unstake_epoch_progress: config.instant_unstake_epoch_progress,
            instant_unstake_inputs_epoch_progress: config.instant_unstake_inputs_epoch_progress,
            num_epochs_between_scoring: config.num_epochs_between_scoring,
            minimum_stake_lamports: config.minimum_stake_lamports,
            minimum_voting_epochs: config.minimum_voting_epochs,
        }
    }
}

#[derive(Parser)]
pub struct TransactionParameters {
    /// priority fee in microlamports
    #[arg(long, env)]
    pub priority_fee: Option<u64>,

    /// CUs per transaction
    #[arg(long, env)]
    pub compute_limit: Option<u32>,

    /// Heap size for heap frame
    #[arg(long, env)]
    pub heap_size: Option<u32>,

    /// Amount of instructions to process in a single transaction
    #[arg(long, env)]
    pub chunk_size: Option<usize>,
}

#[derive(Parser)]
pub struct PermissionlessParameters {
    /// Path to keypair used to pay for the transaction
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
pub struct PermissionedParameters {
    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    // Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
pub struct ViewParameters {
    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,
}

// ---------- COMMANDS ------------
#[derive(Subcommand)]
pub enum Commands {
    // Views
    ViewState(ViewState),
    ViewConfig(ViewConfig),
    ViewNextIndexToRemove(ViewNextIndexToRemove),

    // Actions
    InitSteward(InitSteward),
    ReallocState(ReallocState),

    UpdateConfig(UpdateConfig),
    ResetState(ResetState),

    CloseSteward(CloseSteward),
    RemoveBadValidators(RemoveBadValidators),
    ManuallyCopyVoteAccount(ManuallyCopyVoteAccount),
    ManuallyRemoveValidator(ManuallyRemoveValidator),
    AutoRemoveValidatorFromPool(AutoRemoveValidatorFromPool),
    AutoAddValidatorFromPool(AutoAddValidatorFromPool),

    // Cranks
    CrankMonkey(CrankMonkey),
    CrankEpochMaintenance(CrankEpochMaintenance),
    CrankComputeScore(CrankComputeScore),
    CrankComputeDelegations(CrankComputeDelegations),
    CrankIdle(CrankIdle),
    CrankComputeInstantUnstake(CrankComputeInstantUnstake),
    CrankRebalance(CrankRebalance),
}

// ---------- VIEWS ------------
#[derive(Parser)]
#[command(about = "View the steward state")]
pub struct ViewState {
    #[command(flatten)]
    pub view_parameters: ViewParameters,

    /// Views the steward state for all validators in the pool
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Parser)]
#[command(about = "View the current steward config account")]
pub struct ViewConfig {
    #[command(flatten)]
    pub view_parameters: ViewParameters,
}

#[derive(Parser)]
#[command(about = "View the next index to remove in in the `epoch_maintenance` call")]
pub struct ViewNextIndexToRemove {
    #[command(flatten)]
    pub view_parameters: ViewParameters,
}

// ---------- ACTIONS ------------

#[derive(Parser)]
#[command(about = "Initialize config account")]
pub struct InitSteward {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    /// The current staker keypair path, defaults to the authority keypair path
    #[arg(short, long, env)]
    pub staker_keypair_path: Option<PathBuf>,

    /// Optional path to Steward Config keypair, if not provided, a new keypair will be created
    #[arg(long, env)]
    pub steward_config_keypair_path: Option<PathBuf>,

    /// Stake pool pubkey
    #[arg(long, env)]
    pub stake_pool: Pubkey,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,

    #[command(flatten)]
    pub config_parameters: ConfigParameters,
}

#[derive(Parser)]
#[command(about = "Updates config account parameters")]
pub struct UpdateConfig {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    #[command(flatten)]
    pub config_parameters: ConfigParameters,
}

#[derive(Parser)]
#[command(about = "Initialize state account")]
pub struct ReallocState {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Reset steward state")]
pub struct ResetState {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(
    about = "Closes the steward accounts and returns the staker to the authority calling this function"
)]
pub struct CloseSteward {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Manually updates vote account per validator index")]
pub struct ManuallyCopyVoteAccount {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

    /// Validator index of validator list to update
    #[arg(long, env)]
    pub validator_index_to_update: usize,
}

#[derive(Parser)]
#[command(about = "Removes validator from pool")]
pub struct ManuallyRemoveValidator {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Validator index of validator list to remove
    #[arg(long, env)]
    pub validator_index_to_remove: u64,
}

#[derive(Parser)]
#[command(about = "Removes bad validators from the pool")]
pub struct RemoveBadValidators {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Calls `auto_remove_validator_from_pool`")]
pub struct AutoRemoveValidatorFromPool {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

    /// Validator index of validator list to remove
    #[arg(long, env)]
    pub validator_index_to_remove: u64,
}

#[derive(Parser)]
#[command(about = "Calls `auto_add_validator_from_pool`")]
pub struct AutoAddValidatorFromPool {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

    /// Validator vote account to add
    #[arg(long, env)]
    pub vote_account: Pubkey,
}

// ---------- CRANKS ------------

#[derive(Parser)]
#[command(about = "Crank `compute_score` state")]
pub struct CrankMonkey {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

#[derive(Parser)]
#[command(about = "Run epoch maintenance - needs to be run at the start of each epoch")]
pub struct CrankEpochMaintenance {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

    /// Validator index to remove, gotten from `validators_to_remove` Bitmask
    #[arg(long, env)]
    pub validator_index_to_remove: Option<u64>,
}

#[derive(Parser)]
#[command(about = "Crank `compute_score` state")]
pub struct CrankComputeScore {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

#[derive(Parser)]
#[command(about = "Crank `compute_delegations` state")]
pub struct CrankComputeDelegations {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

#[derive(Parser)]
#[command(about = "Crank `idle` state")]
pub struct CrankIdle {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

#[derive(Parser)]
#[command(about = "Crank `compute_instant_unstake` state")]
pub struct CrankComputeInstantUnstake {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

#[derive(Parser)]
#[command(about = "Crank `rebalance` state")]
pub struct CrankRebalance {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}
