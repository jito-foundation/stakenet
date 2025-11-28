pub mod init_directed_stake_meta;
pub mod init_directed_stake_ticket;
pub mod init_directed_stake_whitelist;
pub mod init_steward;
pub mod realloc_directed_stake_meta;
pub mod realloc_directed_stake_whitelist;
pub mod realloc_state;

pub(crate) const REALLOCS_PER_TX: usize = 10;