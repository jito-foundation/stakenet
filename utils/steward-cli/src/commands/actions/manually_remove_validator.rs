use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
    transaction::Transaction,
};

use crate::{
    commands::command_args::ManuallyRemoveValidator,
    utils::{accounts::get_all_steward_accounts, transactions::configure_instruction},
};

pub async fn command_manually_remove_validator(
    args: ManuallyRemoveValidator,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = Arc::new(
        read_keypair_file(args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let steward_config = args.permissioned_parameters.steward_config;
    let index_to_remove = args.validator_index_to_remove;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let validator_to_remove = steward_accounts.validator_list_account.validators[index_to_remove];
    let vote_account = validator_to_remove.vote_account_address;

    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        &vote_account,
        &steward_accounts.stake_pool_address,
        None,
    );

    let (transient_stake_address, _) = find_transient_stake_program_address(
        &spl_stake_pool::id(),
        &vote_account,
        &steward_accounts.stake_pool_address,
        steward_accounts.validator_list_account.validators[index_to_remove]
            .transient_seed_suffix
            .into(),
    );

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::RemoveValidatorFromPool {
            signer: authority.pubkey(),
            config: steward_config,
            steward_state: steward_accounts.state_address,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: steward_accounts.stake_pool_address,
            staker: steward_accounts.stake_pool_account.staker,
            withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
            validator_list: steward_accounts.validator_list_address,
            stake_account: stake_address,
            transient_stake_account: transient_stake_address,
            clock: sysvar::clock::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorFromPool {
            validator_list_index: index_to_remove,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[ix],
        args.permissioned_parameters
            .transaction_parameters
            .priority_fee,
        args.permissioned_parameters
            .transaction_parameters
            .compute_limit,
        args.permissioned_parameters
            .transaction_parameters
            .heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {}", signature);

    Ok(())
}
