use anchor_lang::{
    prelude::*,
    solana_program::{clock::Clock, vote},
};
use validator_history_vote_state::VoteStateVersions;

use crate::{errors::ValidatorHistoryError, state::ValidatorHistory, utils::cast_epoch};

/// Bulk version of `copy_vote_account`.
///
/// Instead of one instruction (and its full Anchor account-validation context) per
/// validator, this instruction copies vote-account data for an arbitrary number of
/// validators in a single instruction. The (validator_history_account, vote_account)
/// pairs are passed via `remaining_accounts`, laid out as:
///
///   [vh_0, vote_0, vh_1, vote_1, ..., vh_n, vote_n]
///
/// where each `vh_i` is the writable `ValidatorHistory` PDA for `vote_i`.
///
/// This reduces the keeper's transaction count and per-transaction compute:
/// - `Clock`/epoch are fetched and cast once for the whole batch rather than per
///   validator, and the per-instruction dispatch + account-context overhead is paid
///   once instead of once per validator.
/// - Fewer, larger transactions are needed since the fixed per-instruction message
///   bytes (program-id index + instruction data + account-index array framing) are no
///   longer repeated for every validator.
///
/// Every account is validated exactly as the single-validator `copy_vote_account`
/// instruction validates them, so behavior and safety are preserved. If any account in
/// the batch is invalid the entire instruction fails atomically (no partial writes).
#[derive(Accounts)]
pub struct CopyVoteAccounts<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    // remaining_accounts: repeating [validator_history_account (writable), vote_account] pairs
}

pub fn handle_copy_vote_accounts<'info>(
    ctx: Context<'_, '_, 'info, 'info, CopyVoteAccounts<'info>>,
) -> Result<()> {
    let clock = Clock::get()?;
    let epoch = cast_epoch(clock.epoch)?;

    let accounts = ctx.remaining_accounts;
    // Must be an even number of accounts: one validator_history_account and one
    // vote_account per validator.
    require!(
        accounts.len() % 2 == 0,
        ValidatorHistoryError::InvalidBulkVoteAccounts
    );

    for pair in accounts.chunks(2) {
        let validator_history_account_info = &pair[0];
        let vote_account_info = &pair[1];

        // Mirror `copy_vote_account`'s `#[account(owner = vote::program::ID)]` on the
        // vote account. Checked before reading any bytes from the account.
        require_keys_eq!(
            *vote_account_info.owner,
            vote::program::ID,
            ValidatorHistoryError::InvalidBulkVoteAccounts
        );

        // Mirror `#[account(seeds = [ValidatorHistory::SEED, vote_account.key()], bump)]`:
        // the validator_history_account must be the canonical PDA derived from the
        // provided vote account.
        let (expected_validator_history, _bump) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account_info.key.as_ref()],
            ctx.program_id,
        );
        require_keys_eq!(
            *validator_history_account_info.key,
            expected_validator_history,
            ValidatorHistoryError::InvalidBulkVoteAccounts
        );

        // `AccountLoader::try_from` enforces program ownership and the account
        // discriminator; `load_mut` enforces the account is writable. Together these
        // mirror the `mut` + typed-account constraints of `copy_vote_account`.
        let validator_history_loader: AccountLoader<ValidatorHistory> =
            AccountLoader::try_from(validator_history_account_info)?;
        let mut validator_history_account = validator_history_loader.load_mut()?;

        // Mirror `has_one = vote_account`: the stored vote account must match the
        // provided one.
        require_keys_eq!(
            validator_history_account.vote_account,
            *vote_account_info.key,
            ValidatorHistoryError::InvalidBulkVoteAccounts
        );

        // Identical copy logic to `handle_copy_vote_account`.
        let commission = VoteStateVersions::deserialize_commission(vote_account_info)?;
        validator_history_account.set_commission_and_slot(epoch, commission, clock.slot)?;

        let epoch_credits = VoteStateVersions::deserialize_epoch_credits(vote_account_info)?;
        validator_history_account.insert_missing_entries(&epoch_credits)?;
        validator_history_account.set_epoch_credits(&epoch_credits)?;

        validator_history_account.update_validator_age(epoch)?;

        // `validator_history_account` (RefMut) is dropped here at the end of the
        // iteration, releasing the borrow before the next pair is processed. Zero-copy
        // writes go straight to the account's data buffer, so no explicit serialization
        // is needed.
    }

    Ok(())
}
