use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::cast_epoch,
};
use anchor_lang::{prelude::*, solana_program::vote};

#[derive(Accounts)]
pub struct UpdateStakeHistory<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: fine since we are not deserializing account
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub oracle_authority: Signer<'info>,
}

pub fn handler(
    ctx: Context<UpdateStakeHistory>,
    epoch: u64,
    lamports: u64,
    rank: u32,
    is_superminority: bool,
) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    // Cannot set stake for future epochs
    if epoch > Clock::get()?.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    let epoch = cast_epoch(epoch);

    validator_history_account.set_stake(epoch, lamports, rank, is_superminority)?;

    Ok(())
}
