use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use validator_history::id as validator_history_id;

use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};

use crate::{
    commands::command_args::CrankComputeInstantUnstake,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
        },
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

pub async fn command_crank_compute_instant_unstake(
    args: CrankComputeInstantUnstake,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let validator_history_program_id = validator_history_id();
    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::ComputeInstantUnstake => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Compute Instant Unstake state: {}",
                steward_accounts.state_account.state.state_tag
            );
            return Ok(());
        }
    }

    let validators_to_run = (0..steward_accounts.state_account.state.num_pool_validators)
        .filter_map(|validator_index| {
            let has_been_scored = steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_scored {
                None
            } else {
                let vote_account = steward_accounts.validator_list_account.validators
                    [validator_index]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                Some((validator_index, vote_account, history_account))
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let cluster_history = get_cluster_history_address(&validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|(validator_index, _, history_account)| Instruction {
            program_id,
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: steward_config,
                state_account: steward_accounts.state_address,
                validator_history: *history_account,
                validator_list: steward_accounts.validator_list_address,
                cluster_history,
                signer: payer.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: *validator_index,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(
        &ixs_to_run,
        args.permissionless_parameters
            .transaction_parameters
            .chunk_size
            .unwrap_or(15),
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

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let submit_stats = submit_packaged_transactions(client, txs_to_run, &payer, None, None).await?;

    println!("Submit stats: {:?}", submit_stats);

    Ok(())
}
