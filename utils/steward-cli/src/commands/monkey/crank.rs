use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{Ok, Result};
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

use crate::{
    commands::command_args::CrankMonkey,
    utils::{
        accounts::{
            get_all_steward_accounts, get_steward_state_account, get_steward_state_address,
            UsefulStewardAccounts,
        },
        transactions::configure_instruction,
    },
};

pub async fn _handle_epoch_maintenance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    let mut current_epoch = client.get_epoch_info().await?.epoch;
    let mut state_epoch = all_steward_accounts.state_account.state.current_epoch;
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_to_remove;

    while state_epoch != current_epoch {
        let mut validator_index_to_remove = None;
        for i in 0..num_validators {
            if validators_to_remove.get(i)? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        let ix = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::EpochMaintenance {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::EpochMaintenance {
                validator_index_to_remove,
            }
            .data(),
        };

        let blockhash = client.get_latest_blockhash().await?;
        let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

        let transaction = Transaction::new_signed_with_payer(
            &configured_ix,
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        );

        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!(
            "EPOCH MAINTENANCE: Removed validator index {:?} : {:?}",
            validator_index_to_remove, signature
        );

        let updated_state_account =
            get_steward_state_account(client, &program_id, &all_steward_accounts.config_address)
                .await?;

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_to_remove;
        state_epoch = updated_state_account.state.current_epoch;
        current_epoch = client.get_epoch_info().await?.epoch;
    }

    Ok(())
}

pub async fn _handle_compute_score(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    // println!("COMPUTE SCORE: {:?}", signature);

    Ok(())
}

pub async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            signer: payer.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("COMPUTE DELEGATION: {:?}", signature);

    Ok(())
}

pub async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    // println!("IDLE: {:?}", signature);

    Ok(())
}

pub async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    // println!("IDLE: {:?}", signature);

    Ok(())
}

pub async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<()> {
    // println!("IDLE: {:?}", signature);

    Ok(())
}

// Only runs one set of commands per "crank"
pub async fn command_crank_monkey(
    args: CrankMonkey,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // ----------- Collect Accounts -------------
    let steward_config = args.permissionless_parameters.steward_config;
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let priority_fee = args
        .permissionless_parameters
        .transaction_parameters
        .priority_fee;

    // ----------- Triage -------------
    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------

        let epoch = client.get_epoch_info().await?.epoch;
        if all_steward_accounts.state_account.state.current_epoch != epoch {
            return _handle_epoch_maintenance(
                &payer,
                client,
                &program_id,
                &all_steward_accounts,
                priority_fee,
            )
            .await;
        }
    }

    {
        // --------- CHECK AND HANDLE STATE -----------

        // State
        match all_steward_accounts.state_account.state.state_tag {
            StewardStateEnum::ComputeScores => {
                return _handle_compute_score(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::ComputeDelegations => {
                return _handle_compute_delegations(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::Idle => {
                return _handle_idle(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::ComputeInstantUnstake => {
                return _handle_compute_score(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::Rebalance => {
                return _handle_rebalance(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
        };
    }
}
