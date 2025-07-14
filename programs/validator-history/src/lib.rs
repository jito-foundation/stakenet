use anchor_lang::prelude::*;

mod allocator;
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
        handle_initialize_validator_history_account(ctx)
    }

    pub fn realloc_validator_history_account(
        ctx: Context<ReallocValidatorHistoryAccount>,
    ) -> Result<()> {
        handle_realloc_validator_history_account(ctx)
    }

    pub fn initialize_cluster_history_account(
        ctx: Context<InitializeClusterHistoryAccount>,
    ) -> Result<()> {
        handle_initialize_cluster_history_account(ctx)
    }

    pub fn realloc_cluster_history_account(
        ctx: Context<ReallocClusterHistoryAccount>,
    ) -> Result<()> {
        handle_realloc_cluster_history_account(ctx)
    }

    pub fn initialize_validator_stake_buffer_account(
        ctx: Context<InitializeValidatorStakeBufferAccount>,
    ) -> Result<()> {
        handle_initialize_validator_stake_buffer_account(ctx)
    }

    pub fn realloc_validator_stake_buffer_account(
        ctx: Context<ReallocValidatorStakeBufferAccount>,
    ) -> Result<()> {
        handle_realloc_validator_stake_buffer_account(ctx)
    }

    pub fn copy_vote_account(ctx: Context<CopyVoteAccount>) -> Result<()> {
        handle_copy_vote_account(ctx)
    }

    pub fn copy_tip_distribution_account(
        ctx: Context<CopyTipDistributionAccount>,
        epoch: u64,
    ) -> Result<()> {
        handle_copy_tip_distribution_account(ctx, epoch)
    }

    pub fn initialize_config(ctx: Context<InitializeConfig>, authority: Pubkey) -> Result<()> {
        handle_initialize_config(ctx, authority)
    }

    pub fn set_new_tip_distribution_program(
        ctx: Context<SetNewTipDistributionProgram>,
    ) -> Result<()> {
        handle_set_new_tip_distribution_program(ctx)
    }

    pub fn set_new_admin(ctx: Context<SetNewAdmin>) -> Result<()> {
        handle_set_new_admin(ctx)
    }

    pub fn set_new_oracle_authority(ctx: Context<SetNewOracleAuthority>) -> Result<()> {
        handle_set_new_oracle_authority(ctx)
    }

    pub fn update_stake_history(
        ctx: Context<UpdateStakeHistory>,
        epoch: u64,
        lamports: u64,
        rank: u32,
        is_superminority: bool,
    ) -> Result<()> {
        handle_update_stake_history(ctx, epoch, lamports, rank, is_superminority)
    }

    pub fn copy_gossip_contact_info(ctx: Context<CopyGossipContactInfo>) -> Result<()> {
        handle_copy_gossip_contact_info(ctx)
    }

    pub fn copy_cluster_info(ctx: Context<CopyClusterInfo>) -> Result<()> {
        handle_copy_cluster_info(ctx)
    }

    pub fn backfill_total_blocks(
        ctx: Context<BackfillTotalBlocks>,
        epoch: u64,
        blocks_in_epoch: u32,
    ) -> Result<()> {
        handle_backfill_total_blocks(ctx, epoch, blocks_in_epoch)
    }

    pub fn update_priority_fee_history(
        ctx: Context<UpdatePriorityFeeHistory>,
        epoch: u64,
        total_priority_fees: u64,
        total_leader_slots: u32,
        blocks_produced: u32,
        highest_oracle_recorded_slot: u64,
    ) -> Result<()> {
        handle_update_priority_fee_history(
            ctx,
            epoch,
            total_priority_fees,
            total_leader_slots,
            blocks_produced,
            highest_oracle_recorded_slot,
        )
    }

    pub fn copy_priority_fee_distribution(
        ctx: Context<CopyPriorityFeeDistribution>,
        epoch: u64,
    ) -> Result<()> {
        handle_copy_priority_fee_distribution_account(ctx, epoch)
    }

    pub fn realloc_config_account(ctx: Context<ReallocConfigAccount>) -> Result<()> {
        handle_realloc_config_account(ctx)
    }

    pub fn set_new_priority_fee_distribution_program(
        ctx: Context<SetNewPriorityFeeDistributionProgram>,
    ) -> Result<()> {
        handle_set_new_priority_fee_distribution_program(ctx)
    }

    pub fn set_new_priority_fee_oracle_authority(
        ctx: Context<SetNewPriorityFeeOracleAuthority>,
    ) -> Result<()> {
        handle_set_new_priority_fee_oracle_authority(ctx)
    }
}
