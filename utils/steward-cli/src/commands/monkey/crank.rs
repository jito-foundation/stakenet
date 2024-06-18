use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{Ok, Result};
use jito_steward::{StewardStateAccount, StewardStateEnum};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
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
    steward_config: &Pubkey,
    priority_fee: Option<u64>,
) -> Result<()> {
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
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

        let (updated_state_account, _) =
            get_steward_state_account(client, &program_id, &all_steward_accounts.config_address)
                .await?;

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_to_remove;
        state_epoch = updated_state_account.state.current_epoch;
        current_epoch = client.get_epoch_info().await?.epoch;
    }

    Ok(())
}

pub async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    steward_config: &Pubkey,
    priority_fee: Option<u64>,
) -> Result<()> {
    let state_address = get_steward_state_address(&program_id, steward_config);
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: *steward_config,
            state_account: state_address,
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

// Only runs one set of commands per "crank"
pub async fn command_crank_monkey(
    args: CrankMonkey,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissionless_parameters.steward_config;
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let priority_fee = args
        .permissionless_parameters
        .transaction_parameters
        .priority_fee;

    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------
        let (state_account, _) =
            get_steward_state_account(client, &program_id, &steward_config).await?;
        let epoch = client.get_epoch_info().await?.epoch;
        if state_account.state.current_epoch != epoch {
            _handle_epoch_maintenance(&payer, client, &program_id, &steward_config, priority_fee)
                .await?;

            // End the crank such that state does not need to be updated?
            return Ok(());
        }
    }

    {
        // --------- CHECK AND HANDLE STATE -----------
        let (state_account, _) =
            get_steward_state_account(client, &program_id, &steward_config).await?;

        // State
        let _result = match state_account.state.state_tag {
            StewardStateEnum::ComputeScores => todo!("ComputeScores"),
            StewardStateEnum::ComputeDelegations => {
                _handle_compute_delegations(
                    &payer,
                    client,
                    &program_id,
                    &steward_config,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::Idle => todo!("Idle"),
            StewardStateEnum::ComputeInstantUnstake => todo!("ComputeInstantUnstake"),
            StewardStateEnum::Rebalance => todo!("Rebalance"),
        };
    }

    // Rebound from any errors

    Ok(())
}
