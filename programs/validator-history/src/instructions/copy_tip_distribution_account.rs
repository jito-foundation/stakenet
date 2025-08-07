use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::{cast_epoch, fixed_point_sol},
    MerkleRootUploadAuthority, ValidatorHistoryEntry,
};
use anchor_lang::{prelude::*, solana_program::vote};

use jito_tip_distribution::state::TipDistributionAccount;

#[derive(Accounts)]
#[instruction(epoch: u64)]
pub struct CopyTipDistributionAccount<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump,
        has_one = vote_account
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: Safe because we check the vote program is the owner before deserialization.
    /// Used to read validator commission.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: Avoiding struct deserialization here to avoid default Owner trait check.
    /// `owner = config.tip_distribution_program.key()` here is sufficient.
    #[account(
        seeds = [
            TipDistributionAccount::SEED,
            vote_account.key().as_ref(),
            epoch.to_le_bytes().as_ref(),
        ],
        bump,
        seeds::program = config.tip_distribution_program.key(),
        owner = config.tip_distribution_program.key()
    )]
    pub tip_distribution_account: UncheckedAccount<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_copy_tip_distribution_account(
    ctx: Context<CopyTipDistributionAccount>,
    epoch: u64,
) -> Result<()> {
    // cant set data in validator history for future epochs
    if epoch > Clock::get()?.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    let epoch = cast_epoch(epoch)?;
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    let mut tda_data: &[u8] = &ctx.accounts.tip_distribution_account.try_borrow_data()?;

    let tip_distribution_account = TipDistributionAccount::try_deserialize(&mut tda_data)?;
    let mev_commission_bps = tip_distribution_account.validator_commission_bps;

    // if the merkle_root has been uploaded pull the mev_earned for the epoch
    let mev_earned = if let Some(merkle_root) = tip_distribution_account.merkle_root {
        fixed_point_sol(merkle_root.max_total_claim)
    } else {
        ValidatorHistoryEntry::default().mev_earned
    };

    let merkle_root_upload_authority = MerkleRootUploadAuthority::from_pubkey(
        &tip_distribution_account.merkle_root_upload_authority,
    );

    validator_history_account.set_mev_commission(
        epoch,
        mev_commission_bps,
        mev_earned,
        merkle_root_upload_authority,
    )?;

    Ok(())
}
