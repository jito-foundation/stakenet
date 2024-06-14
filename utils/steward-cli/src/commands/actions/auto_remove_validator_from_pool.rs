use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::UpdateParametersArgs;

use keeper_core::{get_multiple_accounts_batched, submit_instructions, submit_transactions};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    find_withdraw_authority_program_address,
};
use validator_history::id as validator_history_id;

use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    pubkey::Pubkey,
    signature::read_keypair_file,
    signer::Signer,
    stake, stake_history, system_program,
    sysvar::{self, rent},
    transaction::Transaction,
};
use validator_history::id;

use crate::{
    commands::commands::AutoRemoveValidatorFromPool,
    utils::{
        accounts::{
            get_all_steward_accounts, get_validator_history_address, UsefulStewardAccounts,
        },
        print,
        transactions::debug_send_single_transaction,
    },
};

pub async fn command_auto_remove_validator_from_pool(
    args: AutoRemoveValidatorFromPool,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    let validator_index = args.validator_index_to_remove;
    let args = args.permissionless_parameters;
    let arc_client = Arc::new(client);

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");
    let arc_payer = Arc::new(payer);

    let validator_history_program_id = validator_history_id();
    let steward_config = args.steward_config;

    let steward_accounts =
        get_all_steward_accounts(&arc_client, &program_id, &steward_config).await?;

    let vote_account =
        steward_accounts.validator_list_account.validators[validator_index].vote_account_address;
    let history_account =
        get_validator_history_address(&vote_account, &validator_history_program_id);

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
        steward_accounts.validator_list_account.validators[validator_index]
            .transient_seed_suffix
            .into(),
    );

    let remove_ix = Instruction {
        program_id: program_id,
        accounts: jito_steward::accounts::AutoRemoveValidator {
            validator_history_account: history_account,
            config: args.steward_config,
            state_account: steward_accounts.state_address,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: steward_accounts.stake_pool_address,
            staker: steward_accounts.staker_address,
            reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
            withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
            validator_list: steward_accounts.validator_list_address,
            stake_account: stake_address,
            transient_stake_account: transient_stake_address,
            vote_account: vote_account,
            rent: solana_sdk::sysvar::rent::id(),
            clock: solana_sdk::sysvar::clock::id(),
            stake_history: solana_sdk::sysvar::stake_history::id(),
            stake_config: stake::config::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: arc_payer.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoRemoveValidatorFromPool {
            validator_list_index: validator_index,
        }
        .data(),
    };

    let blockhash = arc_client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[remove_ix],
        Some(&arc_payer.pubkey()),
        &[&arc_payer],
        blockhash,
    );

    let signature = arc_client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);

    // let submit_stats = submit_transactions(&arc_client, txs_to_run, &arc_payer).await?;

    // println!("Submit stats: {:?}", submit_stats);

    Ok(())
}

fn _package_remove_bad_validator_instructions(
    ixs: &Vec<Instruction>,
    priority_fee: u64,
) -> Vec<Vec<Instruction>> {
    ixs.chunks(1)
        .map(|chunk: &[Instruction]| {
            let mut chunk_vec = chunk.to_vec();
            chunk_vec.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            );
            chunk_vec.insert(0, ComputeBudgetInstruction::request_heap_frame(256 * 1024));
            chunk_vec.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
            );

            chunk_vec
        })
        .collect::<Vec<Vec<Instruction>>>()
}
