use anchor_lang::prelude::*;

use crate::{
    Config, DirectedStakeTicket, errors::StewardError,
};

#[derive(Accounts)]
pub struct CloseDirectedStakeTicket<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority,
        seeds = [DirectedStakeTicket::SEED, authority.key().as_ref()],
        bump
    )]
    pub ticket_account: AccountLoader<'info, DirectedStakeTicket>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(ticket: &DirectedStakeTicket, authority_pubkey: &Pubkey) -> Result<()> {
        if authority_pubkey != &ticket.ticket_close_authority && authority_pubkey != &ticket.ticket_update_authority {
            msg!("Error: Only the ticket close authority or update authority can close the ticket");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(
    ctx: Context<CloseDirectedStakeTicket>,
) -> Result<()> {
    let ticket = ctx.accounts.ticket_account.load()?;
    CloseDirectedStakeTicket::auth(&ticket, ctx.accounts.authority.key)?;
    Ok(())
}