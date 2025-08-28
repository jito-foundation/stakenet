use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakePreference, utils::U8Bool, Config,
    DirectedStakeTicket, DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct InitializeDirectedStakeTicket<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub whitelist_account: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(
        init,
        payer = signer,
        space = DirectedStakeTicket::SIZE,
        seeds = [DirectedStakeTicket::SEED, signer.key().as_ref()],
        bump
    )]
    pub ticket_account: AccountLoader<'info, DirectedStakeTicket>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

impl InitializeDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(whitelist: &DirectedStakeWhitelist, signer_pubkey: &Pubkey) -> Result<()> {
        if !whitelist.is_staker_permissioned(signer_pubkey) {
            msg!("Error: Signer must be on the directed stake whitelist to initialize a ticket");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(
    ctx: Context<InitializeDirectedStakeTicket>,
    ticket_update_authority: Pubkey,
    ticket_close_authority: Pubkey,
    ticket_holder_is_protocol: bool,
) -> Result<()> {
    let whitelist = ctx.accounts.whitelist_account.load()?;
    InitializeDirectedStakeTicket::auth(&whitelist, ctx.accounts.signer.key)?;

    let mut ticket = ctx.accounts.ticket_account.load_init()?;
    ticket.num_preferences = 0;
    ticket.staker_preferences =
        [DirectedStakePreference::empty(); crate::MAX_PREFERENCES_PER_TICKET];
    ticket.ticket_update_authority = ticket_update_authority;
    ticket.ticket_close_authority = ticket_close_authority;
    ticket.ticket_holder_is_protocol = U8Bool::from(ticket_holder_is_protocol);
    ticket._padding0 = [0; 125];

    Ok(())
}
