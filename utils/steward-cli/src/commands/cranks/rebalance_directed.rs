use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
#[allow(deprecated)]
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, stake, system_program};
use stakenet_sdk::utils::{
    accounts::{
        get_all_steward_accounts, get_directed_stake_meta, get_directed_stake_meta_address,
        get_stake_address, get_transient_stake_address,
    },
    helpers::DirectedRebalanceProgressionInfo,
    transactions::{package_instructions, print_base58_tx, submit_packaged_transactions},
};

use crate::commands::command_args::PermissionlessParameters;

#[derive(Parser)]
#[command(about = "Crank `rebalance_directed` state")]
pub struct CrankRebalanceDirected {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

pub async fn command_crank_rebalance_directed(
    args: CrankRebalanceDirected,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    if !matches!(
        steward_accounts.state_account.state.state_tag,
        StewardStateEnum::RebalanceDirected
    ) {
        println!(
            "State account is not in RebalanceDirected state: {}",
            steward_accounts.state_account.state.state_tag
        );
        return Ok(());
    }

    let directed_stake_meta_address =
        get_directed_stake_meta_address(&steward_accounts.config_address, &program_id);
    let directed_stake_meta_account = get_directed_stake_meta(
        client.clone(),
        &steward_accounts.config_address,
        &program_id,
    )
    .await?;
    let validators_to_run = DirectedRebalanceProgressionInfo::get_directed_staking_validators(
        &steward_accounts,
        &directed_stake_meta_account,
    );

    let ixs_to_run: Vec<Instruction> = validators_to_run
        .iter()
        .map(|validator_info| {
            let validator_index = validator_info.validator_list_index;
            let vote_account = &validator_info.vote_account;

            let stake_address =
                get_stake_address(vote_account, &steward_accounts.stake_pool_address);

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &steward_accounts.stake_pool_address,
                &steward_accounts.validator_list_account,
                validator_index,
            )
            .unwrap_or(Pubkey::new_unique());

            Instruction {
                program_id,
                accounts: jito_steward::accounts::RebalanceDirected {
                    config: steward_accounts.config_address,
                    state_account: steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: steward_accounts.stake_pool_address,
                    withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
                    validator_list: steward_accounts.validator_list_address,
                    reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                    directed_stake_meta: directed_stake_meta_address,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::RebalanceDirected {
                    directed_stake_meta_index: validator_info.directed_stake_meta_index as u64,
                }
                .data(),
            }
        })
        .collect();

    let txs_to_run = package_instructions(
        &ixs_to_run,
        args.permissionless_parameters
            .transaction_parameters
            .chunk_size
            .unwrap_or(1),
        args.permissionless_parameters
            .transaction_parameters
            .priority_fee,
        args.permissionless_parameters
            .transaction_parameters
            .compute_limit
            .or(Some(1_400_000)),
        None,
    );

    if args
        .permissionless_parameters
        .transaction_parameters
        .print_tx
    {
        txs_to_run.iter().for_each(|tx| print_base58_tx(tx));
    } else {
        println!("Submitting {} instructions", ixs_to_run.len());
        println!("Submitting {} transactions", txs_to_run.len());

        let submit_stats =
            submit_packaged_transactions(client, txs_to_run, &payer, None, None).await?;

        println!("Submit stats: {submit_stats:?}");
    }

    Ok(())
}
