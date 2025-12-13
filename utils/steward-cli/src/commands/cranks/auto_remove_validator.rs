use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, stake, system_program};
use stakenet_sdk::utils::{
    accounts::{
        get_all_steward_accounts, get_all_validator_accounts, get_stake_address,
        get_transient_stake_address,
    },
    helpers::check_stake_accounts,
};

use crate::{
    commands::command_args::PermissionlessParameters,
    utils::{
        accounts::get_validator_history_address,
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

#[derive(Parser)]
#[command(about = "Crank `rebalance_directed` state")]
pub struct CrankAutoRemoveValidator {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

pub async fn command_crank_auto_remove_validator(
    args: CrankAutoRemoveValidator,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );
    let epoch = client.get_epoch_info().await?.epoch;
    let all_steward_accounts = get_all_steward_accounts(
        client,
        &jito_steward::id(),
        &args.permissionless_parameters.steward_config,
    )
    .await?;
    let all_vote_accounts = client.get_vote_accounts().await?;
    let all_validator_accounts =
        get_all_validator_accounts(client, &all_vote_accounts.current, &validator_history::id())
            .await?;
    let checks = check_stake_accounts(&all_validator_accounts, epoch);

    let bad_vote_accounts = checks
        .iter()
        .filter_map(|(vote_account, check)| {
            if !check.has_history || check.is_deactivated || !check.has_vote_account {
                Some(*vote_account)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

    let ixs_to_run = bad_vote_accounts
        .iter()
        .filter_map(|vote_account| {
            let validator_index = all_steward_accounts
                .validator_list_account
                .validators
                .iter()
                .position(|v| v.vote_account_address == *vote_account)?;

            let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &all_steward_accounts.stake_pool_address,
                &all_steward_accounts.validator_list_account,
                validator_index,
            )?;

            if all_steward_accounts
                .state_account
                .state
                .validators_to_remove
                .get(validator_index)
                .expect("Could not find validator index in validators_to_remove")
            {
                return None;
            }

            Some(Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::AutoRemoveValidator {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    validator_history_account: history_account,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::AutoRemoveValidatorFromPool {
                    validator_list_index: validator_index as u64,
                }
                .data(),
            })
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, None, Some(1_400_000), None);

    log::info!("Submitting {} instructions", ixs_to_run.len());
    log::info!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, &payer, Some(50), None).await?;

    Ok(())
}
