use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};

use crate::{
    commands::command_args::ViewStatePerValidator,
    utils::accounts::{get_all_steward_accounts, UsefulStewardAccounts},
};

pub async fn command_view_state_per_validator(
    args: ViewStatePerValidator,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    let steward_state_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    _print_verbose_state(&steward_state_accounts);

    Ok(())
}

fn _print_verbose_state(steward_state_accounts: &UsefulStewardAccounts) {
    let mut formatted_string;

    for (index, validator) in steward_state_accounts
        .validator_list_account
        .validators
        .iter()
        .enumerate()
    {
        let vote_account = validator.vote_account_address;
        let (stake_address, _) = find_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account,
            &steward_state_accounts.stake_pool_address,
            None,
        );

        let (transient_stake_address, _) = find_transient_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account,
            &steward_state_accounts.stake_pool_address,
            validator.transient_seed_suffix.into(),
        );

        let score_index = steward_state_accounts
            .state_account
            .state
            .sorted_score_indices
            .iter()
            .position(|&i| i == index as u16);
        let yield_score_index = steward_state_accounts
            .state_account
            .state
            .sorted_yield_score_indices
            .iter()
            .position(|&i| i == index as u16);

        formatted_string = String::new();

        formatted_string += &format!("Vote Account: {:?}\n", vote_account);
        formatted_string += &format!("Stake Account: {:?}\n", stake_address);
        formatted_string += &format!("Transient Stake Account: {:?}\n", transient_stake_address);
        formatted_string += &format!(
            "Validator Lamports: {:?}\n",
            u64::from(validator.active_stake_lamports)
        );
        formatted_string += &format!("Index: {:?}\n", index);
        formatted_string += &format!(
            "Is Blacklisted: {:?}\n",
            steward_state_accounts.config_account.blacklist.get(index)
        );
        formatted_string += &format!(
            "Is Instant Unstake: {:?}\n",
            steward_state_accounts
                .state_account
                .state
                .instant_unstake
                .get(index)
        );
        formatted_string += &format!(
            "Score: {:?}\n",
            steward_state_accounts.state_account.state.scores.get(index)
        );
        formatted_string += &format!(
            "Yield Score: {:?}\n",
            steward_state_accounts
                .state_account
                .state
                .yield_scores
                .get(index)
        );
        formatted_string += &format!("Score Index: {:?}\n", score_index);
        formatted_string += &format!("Yield Score Index: {:?}\n", yield_score_index);

        println!("{}", formatted_string);
    }
}
