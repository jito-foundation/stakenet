use crate::{
    constants::MAX_ALLOC_BYTES,
    state::{Config, ValidatorHistory, ValidatorHistoryEntry, ValidatorHistoryVersion},
};
use anchor_lang::{prelude::*, solana_program::vote};

fn get_realloc_size(account_info: &AccountInfo) -> usize {
    let account_size = account_info.data_len();

    // If account is already over-allocated, don't try to shrink
    if account_size < ValidatorHistory::SIZE {
        ValidatorHistory::SIZE.min(account_size + MAX_ALLOC_BYTES)
    } else {
        account_size
    }
}

fn is_initialized(account_info: &AccountInfo) -> Result<bool> {
    let account_data = account_info.as_ref().try_borrow_data()?;
    // discriminator + version_number
    let vote_account_pubkey_bytes = account_data[(8 + 4)..(8 + 4 + 32)].to_vec();

    // If pubkey is all zeroes, then it's not initialized
    Ok(vote_account_pubkey_bytes.iter().any(|&x| x != 0))
}

#[derive(Accounts)]
pub struct ReallocValidatorHistoryAccount<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(validator_history_account.as_ref()),
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
    #[account(mut, seeds = [Config::SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// CHECK: Safe because we check the vote program is the owner before deserialization.
    /// Used to read validator commission.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_realloc_validator_history_account(
    ctx: Context<ReallocValidatorHistoryAccount>,
) -> Result<()> {
    let account_size = ctx.accounts.validator_history_account.as_ref().data_len();
    if account_size >= ValidatorHistory::SIZE
        && !is_initialized(ctx.accounts.validator_history_account.as_ref())?
    {
        // Can actually initialize values now that the account is proper size
        let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

        validator_history_account.index = ctx.accounts.config.counter;
        ctx.accounts.config.counter += 1;
        validator_history_account.bump = ctx.bumps.validator_history_account;
        validator_history_account.vote_account = *ctx.accounts.vote_account.key;
        validator_history_account.struct_version = ValidatorHistoryVersion::V0 as u32;
        validator_history_account.history.idx =
            (validator_history_account.history.arr.len() - 1) as u64;
        for _ in 0..validator_history_account.history.arr.len() {
            validator_history_account
                .history
                .push(ValidatorHistoryEntry::default());
        }
        validator_history_account.history.is_empty = 1;
    }

    Ok(())
}
