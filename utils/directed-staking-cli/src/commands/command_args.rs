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

    /// Filepath to a keypair, or "ledger" for Ledger hardware wallet
    #[arg(long, global = true, env)]
    pub signer: Option<String>,

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

    /// Minimum epoch progress for computing directed stake meta
    #[arg(long, env)]
    pub min_epoch_progress_for_compute_directed_stake_meta: Option<f64>,

    /// Maximum epoch progress for directed rebalance
    #[arg(long, env)]
    pub max_epoch_progress_for_directed_rebalance: Option<f64>,

    /// Epoch progress for computing score
    #[arg(long, env)]
    pub compute_score_epoch_progress: Option<f64>,

    /// Minimum lamports for undirected stake floor
    #[arg(long, env)]
    pub undirected_stake_floor_lamports: Option<u64>,

    /// Percent of total pool lamports that can be unstaked due to directed stake requests
    #[arg(long, env)]
    pub directed_stake_unstake_cap_bps: Option<u16>,
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
            compute_score_epoch_progress: config.compute_score_epoch_progress,
            undirected_stake_floor_lamports: config.undirected_stake_floor_lamports,
            directed_stake_unstake_cap_bps: config.directed_stake_unstake_cap_bps,
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

    /// Optional authority pubkey to use when printing transactions (no keypair required)
    #[arg(long, env)]
    pub authority_pubkey: Option<Pubkey>,

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
    ViewConfig(ViewConfig),
    ViewDirectedStakeTickets(ViewDirectedStakeTickets),
    ViewDirectedStakeTicket(ViewDirectedStakeTicket),
    ViewDirectedStakeWhitelist(ViewDirectedStakeWhitelist),
    ViewDirectedStakeMeta(ViewDirectedStakeMeta),
    ComputeDirectedStakeMeta(ComputeDirectedStakeMeta),
    InitDirectedStakeMeta(InitDirectedStakeMeta),
    ReallocDirectedStakeMeta(ReallocDirectedStakeMeta),
    InitDirectedStakeWhitelist(InitDirectedStakeWhitelist),
    ReallocDirectedStakeWhitelist(ReallocDirectedStakeWhitelist),
    InitDirectedStakeTicket(InitDirectedStakeTicket),
    UpdateDirectedStakeTicket(UpdateDirectedStakeTicket),
    AddToDirectedStakeWhitelist(AddToDirectedStakeWhitelist),
}

#[derive(Parser)]
#[command(about = "Updates directed stake ticket account")]
pub struct UpdateDirectedStakeTicket {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    #[arg(long, value_delimiter = ',', value_parser = parse_pubkey)]
    pub vote_pubkeys: Vec<Pubkey>,

    /// Vote accounts of validators to blacklist (comma separated)
    #[arg(long, env, value_delimiter = ',', value_parser = parse_u16)]
    pub stake_share_bps: Vec<u16>,
}

fn parse_u16(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse()
}

// Add helper to parse a Pubkey from string
fn parse_pubkey(s: &str) -> Result<Pubkey, solana_sdk::pubkey::ParsePubkeyError> {
    use std::str::FromStr;
    Pubkey::from_str(s)
}

#[derive(Parser)]
#[command(about = "View Config")]
pub struct ViewConfig {
    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub config: Pubkey,
}

#[derive(Parser)]
#[command(about = "View DirectedStakeTickets using memcmp filter for discriminator")]
pub struct ViewDirectedStakeTickets {
    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,
}

#[derive(Parser)]
#[command(about = "View DirectedStakeTicket account")]
pub struct ViewDirectedStakeTicket {
    /// Directed stake ticket address
    #[arg(long)]
    pub ticket_signer: Pubkey,
}

#[derive(Parser)]
#[command(about = "View DirectedStakeWhitelist account contents")]
pub struct ViewDirectedStakeWhitelist {
    /// Steward config account
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

#[derive(Parser)]
#[command(about = "View DirectedStakeMeta account contents")]
pub struct ViewDirectedStakeMeta {
    /// Steward config account
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

#[derive(Parser)]
#[command(about = "Initialize DirectedStakeWhitelist account")]
pub struct InitDirectedStakeWhitelist {
    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
#[command(about = "Reallocate Directed Stake Whitelist account")]
pub struct ReallocDirectedStakeWhitelist {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Reallocate Directed Stake Meta account")]
pub struct ReallocDirectedStakeMeta {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

#[derive(Parser)]
#[command(about = "Initialize DirectedStakeMeta account")]
pub struct InitDirectedStakeMeta {
    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    // Total number of stake targets to be uploaded
    // #[arg(long, env)]
    // pub total_stake_targets: u16,
    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
#[command(about = "Initialize DirectedStakeTicket account")]
pub struct InitDirectedStakeTicket {
    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    /// Ticket update authority pubkey
    #[arg(long, env)]
    pub ticket_update_authority: Pubkey,

    /// Whether the ticket holder is a protocol (default: false)
    #[arg(long, env, default_value = "false")]
    pub ticket_holder_is_protocol: bool,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
#[command(about = "Add to Directed stake whitelist")]
pub struct AddToDirectedStakeWhitelist {
    /// Steward config account
    #[arg(long)]
    pub steward_config: Pubkey,

    /// Record type
    #[arg(long)]
    pub record_type: String,

    /// Record
    #[arg(long)]
    pub record: Pubkey,

    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

#[derive(Parser)]
#[command(about = "Compute directed stake metadata including tickets and JitoSOL balances")]
pub struct ComputeDirectedStakeMeta {
    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,

    /// Copy stake targets on chain
    #[arg(
        long,
        default_value = "false",
        help = "Whether to copy the computed stake targets on-chain"
    )]
    pub copy_targets: bool,

    /// Display progress bar
    #[arg(long, env, default_value = "false")]
    pub progress_bar: bool,

    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,
}
