use anchor_lang::{
    prelude::{AccountInfo, Pubkey},
    solana_program::native_token::lamports_to_sol,
};

pub fn cast_epoch(epoch: u64) -> u16 {
    (epoch % u16::MAX as u64).try_into().unwrap()
}

pub fn cast_epoch_start_timestamp(start_timestamp: i64) -> u32 {
    start_timestamp.try_into().unwrap()
}

pub fn fixed_point_sol(lamports: u64) -> u32 {
    // convert to sol
    let mut sol = lamports_to_sol(lamports);
    // truncate to 2 decimal points by rounding up, technically we can combine this line and the next
    sol = f64::round(sol * 100.0) / 100.0;
    // return a 4byte unsigned fixed point number with a 1/100 scaling factor
    // this will internally represent a max value of 42949672.95 SOL
    (sol * 100.0) as u32
}

pub fn get_vote_account(validator_history_account_info: &AccountInfo) -> Pubkey {
    let pubkey_bytes = &validator_history_account_info.data.borrow()[8..32 + 8];
    let mut data = [0; 32];
    data.copy_from_slice(pubkey_bytes);
    Pubkey::from(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_point_sol() {
        assert_eq!(fixed_point_sol(1_000_000_000), 100);
        assert_eq!(fixed_point_sol(4_294_967_295_000_000_000), 4294967295);

        assert_eq!(fixed_point_sol(429_496_729_600_000_000), 4294967295)
    }
}
