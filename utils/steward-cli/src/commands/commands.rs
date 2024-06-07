use clap::{arg, command, Parser, Subcommand};
use jito_steward::UpdateParametersArgs;
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "CLI for validator history program")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub json_rpc_url: String,

    /// Steward Program ID
    #[arg(
        long,
        env,
        default_value_t = jito_steward::id()
    )]
    pub program_id: Pubkey,

    #[command(subcommand)]
    pub commands: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    InitConfig(InitConfig),
    ViewConfig(ViewConfig),
    UpdateConfig(UpdateConfig),

    InitState(InitState),
    ViewState(ViewState),

    CrankComputeScore(CrankComputeScore),
    CrankComputeDelegations(CrankComputeDelegations),
    CrankIdle(CrankIdle),
    CrankComputeInstantUnstake(CrankComputeInstantUnstake),
    CrankRebalance(CrankRebalance),
}

#[derive(Parser)]
#[command(about = "Initialize config account")]
pub struct InitState {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    /// Stake pool pubkey
    #[arg(long, env)]
    pub stake_pool: Pubkey,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,
}

#[derive(Parser)]
#[command(about = "View the current config account parameters")]
pub struct ViewState {
    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,
}

#[derive(Parser)]
#[command(about = "Initialize config account")]
pub struct InitConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    /// Defaults to authority keypair
    #[arg(short, long, env)]
    pub staker_keypair_path: Option<PathBuf>,

    /// Optional path to Steward Config keypair
    #[arg(long, env)]
    pub steward_config_keypair_path: Option<PathBuf>,

    /// Stake pool pubkey
    #[arg(long, env)]
    pub stake_pool: Pubkey,

    #[command(flatten)]
    pub config_parameters: ConfigParameters,
}

#[derive(Parser)]
#[command(about = "Updates Config account parameters")]
pub struct UpdateConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    #[command(flatten)]
    pub config_parameters: ConfigParameters,
}

#[derive(Parser)]
#[command(about = "View the current config account parameters")]
pub struct ViewConfig {
    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,
}

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
    pub compute_score_slot_range: Option<usize>,

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

impl ConfigParameters {
    pub fn to_update_parameters_args(&self) -> UpdateParametersArgs {
        UpdateParametersArgs {
            mev_commission_range: self.mev_commission_range,
            epoch_credits_range: self.epoch_credits_range,
            commission_range: self.commission_range,
            scoring_delinquency_threshold_ratio: self.scoring_delinquency_threshold_ratio,
            instant_unstake_delinquency_threshold_ratio: self
                .instant_unstake_delinquency_threshold_ratio,
            mev_commission_bps_threshold: self.mev_commission_bps_threshold,
            commission_threshold: self.commission_threshold,
            historical_commission_threshold: self.historical_commission_threshold,
            num_delegation_validators: self.num_delegation_validators,
            scoring_unstake_cap_bps: self.scoring_unstake_cap_bps,
            instant_unstake_cap_bps: self.instant_unstake_cap_bps,
            stake_deposit_unstake_cap_bps: self.stake_deposit_unstake_cap_bps,
            compute_score_slot_range: self.compute_score_slot_range,
            instant_unstake_epoch_progress: self.instant_unstake_epoch_progress,
            instant_unstake_inputs_epoch_progress: self.instant_unstake_inputs_epoch_progress,
            num_epochs_between_scoring: self.num_epochs_between_scoring,
            minimum_stake_lamports: self.minimum_stake_lamports,
            minimum_voting_epochs: self.minimum_voting_epochs,
        }
    }
}

#[derive(Parser)]
#[command(about = "Cranks the compute score state")]
pub struct CrankComputeScore {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// priority fee in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fee: u64,
}

#[derive(Parser)]
#[command(about = "Cranks the compute delegation")]
pub struct CrankComputeDelegations {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// priority fee in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fee: u64,
}

#[derive(Parser)]
#[command(about = "Crank idle state")]
pub struct CrankIdle {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// priority fee in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fee: u64,
}

#[derive(Parser)]
#[command(about = "Cranks the compute instant")]
pub struct CrankComputeInstantUnstake {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// priority fee in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fee: u64,
}

#[derive(Parser)]
#[command(about = "Cranks rebalance")]
pub struct CrankRebalance {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub payer_keypair_path: PathBuf,

    /// Steward account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// priority fee in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fee: u64,
}
