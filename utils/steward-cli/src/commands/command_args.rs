use clap::{arg, command, Parser, Subcommand};
use jito_steward::{UpdateParametersArgs, UpdatePriorityFeeParametersArgs};
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "CLI for the steward program", version)]
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

    /// Inputs to "Compute Instant Unstake" need to be updated past this point in epoch progress
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
pub struct ConfigPriorityFeeParameters {
    /// The number of epochs the priority fee distribution check should lookback
    #[arg(long, env)]
    pub priority_fee_lookback_epochs: Option<u8>,

    /// The offset of epochs for the priority fee distribution
    #[arg(long, env)]
    pub priority_fee_lookback_offset: Option<u8>,

    /// The maximum validator commission before the validator scores 0
    #[arg(long, env)]
    pub priority_fee_max_commission_bps: Option<u16>,

    /// An error of margin for priority fee commission calculations
    #[arg(long, env)]
    pub priority_fee_error_margin_bps: Option<u16>,

    /// The epoch for when priority fee commission scoring starts
    #[arg(long, env)]
    pub priority_fee_scoring_start_epoch: Option<u16>,
}

impl From<ConfigPriorityFeeParameters> for UpdatePriorityFeeParametersArgs {
    fn from(config: ConfigPriorityFeeParameters) -> Self {
        UpdatePriorityFeeParametersArgs {
            priority_fee_lookback_epochs: config.priority_fee_lookback_epochs,
            priority_fee_lookback_offset: config.priority_fee_lookback_offset,
            priority_fee_max_commission_bps: config.priority_fee_max_commission_bps,
            priority_fee_error_margin_bps: config.priority_fee_error_margin_bps,
            priority_fee_scoring_start_epoch: config.priority_fee_scoring_start_epoch,
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

    /// This will print out the raw TX instead of running it
    #[arg(long, env, default_value = "false")]
    pub print_tx: bool,

    /// When enabled, prints the transaction as a spl-governance encoded InstructionData (Base64)
    #[arg(long, env, default_value_t = false)]
    pub print_gov_tx: bool,
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

    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,
}

// ---------- COMMANDS ------------
#[derive(Subcommand)]
pub enum Commands {
    // Views
    ViewState(ViewState),
    ViewConfig(ViewConfig),
    ViewNextIndexToRemove(ViewNextIndexToRemove),
    AnalyzeScores(super::info::analyze_scores::AnalyzeScores),

    // Actions
    InitSteward(InitSteward),
    ReallocState(ReallocState),

    SetStaker(SetStaker),
    RevertStaker(RevertStaker),

    UpdateAuthority(UpdateAuthority),
    UpdateConfig(UpdateConfig),
    UpdatePriorityFeeConfig(UpdatePriorityFeeConfig),
    ResetState(ResetState),
    ResetValidatorLamportBalances(ResetValidatorLamportBalances),

    Pause(Pause),
    Resume(Resume),

    AddToBlacklist(AddToBlacklist),
    RemoveFromBlacklist(RemoveFromBlacklist),

    CloseSteward(CloseSteward),
    RemoveBadValidators(RemoveBadValidators),
    ManuallyCopyVoteAccount(ManuallyCopyVoteAccount),
    ManuallyCopyAllVoteAccounts(ManuallyCopyAllVoteAccounts),
    ManuallyRemoveValidator(ManuallyRemoveValidator),
    AutoRemoveValidatorFromPool(AutoRemoveValidatorFromPool),
    AutoAddValidatorFromPool(AutoAddValidatorFromPool),
    InstantRemoveValidator(InstantRemoveValidator),
    UpdateValidatorListBalance(UpdateValidatorListBalance),

    // Cranks
    CrankSteward(CrankSteward),
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

    /// Optional vote account to view the state of
    #[arg(long)]
    pub vote_account: Option<Pubkey>,
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

    #[command(flatten)]
    pub config_priority_fee_parameters: ConfigPriorityFeeParameters,
}

#[derive(Parser)]
#[command(about = "Updates authority account parameters")]
pub struct UpdateAuthority {
    #[command(subcommand)]
    pub command: AuthoritySubcommand,
}

#[derive(Subcommand)]
pub enum AuthoritySubcommand {
    /// Manages blacklist authority
    Blacklist {
        #[command(flatten)]
        permissioned_parameters: PermissionedParameters,
        #[arg(long, env)]
        new_authority: Pubkey,
    },
    /// Manages admin authority
    Admin {
        #[command(flatten)]
        permissioned_parameters: PermissionedParameters,
        #[arg(long, env)]
        new_authority: Pubkey,
    },
    /// Manages parameters authority
    Parameters {
        #[command(flatten)]
        permissioned_parameters: PermissionedParameters,
        #[arg(long, env)]
        new_authority: Pubkey,
    },
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
#[command(about = "Updates config priority fee parameters")]
pub struct UpdatePriorityFeeConfig {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    #[command(flatten)]
    pub config_parameters: ConfigPriorityFeeParameters,
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
#[command(about = "Reset steward state")]
pub struct ResetValidatorLamportBalances {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Add to the blacklist")]
pub struct AddToBlacklist {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Validator indices of validator list to blacklist (comma separated)
    #[arg(long, env, value_delimiter = ',', num_args = 1.., value_parser = parse_u32)]
    pub validator_history_indices_to_blacklist: Vec<u32>,

    /// Vote accounts of validators to blacklist (comma separated)
    #[arg(long, env, value_delimiter = ',', num_args = 1.., value_parser = parse_pubkey)]
    pub vote_accounts_to_blacklist: Vec<Pubkey>,
}

fn parse_u32(s: &str) -> Result<u32, std::num::ParseIntError> {
    s.parse()
}

// Add helper to parse a Pubkey from string
fn parse_pubkey(s: &str) -> Result<Pubkey, solana_sdk::pubkey::ParsePubkeyError> {
    use std::str::FromStr;
    Pubkey::from_str(s)
}

#[derive(Parser)]
#[command(about = "Remove from the blacklist")]
pub struct RemoveFromBlacklist {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Validator indices of validator list to remove (comma separated)
    #[arg(long, env, value_delimiter = ',', num_args = 1.., value_parser = parse_u32)]
    pub validator_history_indices_to_deblacklist: Vec<u32>,
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
#[command(about = "Transfers the Staker to the Steward State Account")]
pub struct SetStaker {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Transfers the Staker to the calling authority")]
pub struct RevertStaker {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Pause the steward program")]
pub struct Pause {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Resume the steward program")]
pub struct Resume {
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
    pub validator_index_to_update: u64,
}

#[derive(Parser)]
#[command(about = "Manually updates all vote accounts")]
pub struct ManuallyCopyAllVoteAccounts {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
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
#[command(about = "Instantly removes validator from pool")]
pub struct InstantRemoveValidator {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

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

#[derive(Parser)]
#[command(about = "Updates validator list balance (spl_stake_pool command) for a single validator")]
pub struct UpdateValidatorListBalance {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,

    /// Validator index in the validator list
    #[arg(long, env)]
    pub validator_list_index: u32,

    #[arg(long, env, default_value_t = false)]
    pub no_merge: bool,
}

// ---------- CRANKS ------------

#[derive(Parser)]
#[command(about = "Crank the entire Steward program")]
pub struct CrankSteward {
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
