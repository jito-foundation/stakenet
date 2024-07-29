use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};

use crate::{
    commands::command_args::ManuallyCopyAllVoteAccounts,
    utils::{
        accounts::{get_all_steward_accounts, get_validator_history_address},
        transactions::{package_instructions, print_base58_tx, submit_packaged_transactions},
    },
};

pub async fn command_manually_copy_all_vote_accounts(
    args: ManuallyCopyAllVoteAccounts,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let validator_history_program_id = spl_stake_pool::id();
    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let ixs_to_run = steward_accounts
        .validator_list_account
        .validators
        .iter()
        .enumerate()
        .filter_map(|(index, validator)| {
            let vote_account = validator.vote_account_address;
            let validator_history_account =
                get_validator_history_address(&vote_account, &validator_history_program_id);

            if steward_accounts
                .state_account
                .state
                .progress
                .get(index)
                .expect("Index is not in progress bitmask")
            {
                return None;
            }

            Some(Instruction {
                program_id: validator_history::id(),
                accounts: validator_history::accounts::CopyVoteAccount {
                    validator_history_account,
                    vote_account,
                    signer: payer.pubkey(),
                }
                .to_account_metas(None),
                data: validator_history::instruction::CopyVoteAccount {}.data(),
            })
        })
        .collect::<Vec<Instruction>>();

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
        args.permissionless_parameters
            .transaction_parameters
            .heap_size,
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

        println!("Submit stats: {:?}", submit_stats);
    }

    Ok(())
}
