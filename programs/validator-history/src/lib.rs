use anchor_lang::prelude::*;

pub mod constants;
pub mod crds_value;
pub mod errors;
pub mod instructions;
pub mod serde_varint;
pub mod state;
pub mod utils;

pub use instructions::*;
pub use state::*;

cfg_if::cfg_if! {
    if #[cfg(feature = "mainnet-beta")] {
        declare_id!("HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa");
    } else if #[cfg(feature = "testnet")] {
        declare_id!("HisTBTgDnsdxfMp3m63fgKxCx9xVQE17MhA9BWRdrAP");
    } else {
        declare_id!("HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa");
    }
}

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    // Required fields
    name: "Jito Validator History V1",
    project_url: "https://jito.network/",
    contacts: "email:team@jito.network",
    policy: "https://github.com/jito-foundation/stakenet/blob/master/README.md",
    // Optional Fields
    preferred_languages: "en",
    source_code: "https://github.com/jito-foundation/stakenet"
}

#[program]
pub mod validator_history {

    use super::*;

    pub fn initialize_validator_history_account(
        ctx: Context<InitializeValidatorHistoryAccount>,
    ) -> Result<()> {
        instructions::initialize_validator_history_account::handler(ctx)
    }

    pub fn realloc_validator_history_account(
        ctx: Context<ReallocValidatorHistoryAccount>,
    ) -> Result<()> {
        instructions::realloc_validator_history_account::handler(ctx)
    }

    pub fn initialize_cluster_history_account(
        ctx: Context<InitializeClusterHistoryAccount>,
    ) -> Result<()> {
        instructions::initialize_cluster_history_account::handler(ctx)
    }

    pub fn realloc_cluster_history_account(
        ctx: Context<ReallocClusterHistoryAccount>,
    ) -> Result<()> {
        instructions::realloc_cluster_history_account::handler(ctx)
    }

    pub fn copy_vote_account(ctx: Context<CopyVoteAccount>) -> Result<()> {
        instructions::copy_vote_account::handler(ctx)
    }

    pub fn update_mev_commission(ctx: Context<UpdateMevCommission>) -> Result<()> {
        instructions::update_mev_commission::handler(ctx)
    }

    pub fn initialize_config(ctx: Context<InitializeConfig>, authority: Pubkey) -> Result<()> {
        instructions::initialize_config::handler(ctx, authority)
    }

    pub fn set_new_tip_distribution_program(
        ctx: Context<SetNewTipDistributionProgram>,
    ) -> Result<()> {
        instructions::set_new_tip_distribution_program::handler(ctx)
    }

    pub fn set_new_admin(ctx: Context<SetNewAdmin>) -> Result<()> {
        instructions::set_new_admin::handler(ctx)
    }

    pub fn set_new_oracle_authority(ctx: Context<SetNewOracleAuthority>) -> Result<()> {
        instructions::set_new_oracle_authority::handler(ctx)
    }

    pub fn update_stake_history(
        ctx: Context<UpdateStakeHistory>,
        epoch: u64,
        lamports: u64,
        rank: u32,
        is_superminority: bool,
    ) -> Result<()> {
        instructions::update_stake_history::handler(ctx, epoch, lamports, rank, is_superminority)
    }

    pub fn copy_gossip_contact_info(ctx: Context<CopyGossipContactInfo>) -> Result<()> {
        instructions::copy_gossip_contact_info::handler(ctx)
    }

    pub fn copy_cluster_info(ctx: Context<CopyClusterInfo>) -> Result<()> {
        instructions::copy_cluster_info::handler(ctx)
    }

    pub fn backfill_total_blocks(
        ctx: Context<BackfillTotalBlocks>,
        epoch: u64,
        blocks_in_epoch: u32,
    ) -> Result<()> {
        instructions::backfill_total_blocks::handler(ctx, epoch, blocks_in_epoch)
    }
}
