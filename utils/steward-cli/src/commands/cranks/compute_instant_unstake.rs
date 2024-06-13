use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use keeper_core::{submit_instructions, submit_transactions};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use validator_history::id as validator_history_id;

use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::InstructionError, pubkey::Pubkey,
    signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::commands::CrankComputeInstantUnstake,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
        },
        print::state_tag_to_string,
        transactions::{debug_send_single_transaction, package_instructions},
    },
};

pub async fn command_crank_compute_instant_unstake(
    args: CrankComputeInstantUnstake,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    let args = args.permissionless_parameters;

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let arc_client = Arc::new(client);
    let arc_payer = Arc::new(payer);

    let validator_history_program_id = validator_history_id();
    let steward_config = args.steward_config;

    let steward_accounts =
        get_all_steward_accounts(&arc_client, &program_id, &steward_config).await?;

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::ComputeInstantUnstake => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Compute Instant Unstake state: {}",
                state_tag_to_string(steward_accounts.state_account.state.state_tag)
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
                return None;
            } else {
                let vote_account = steward_accounts.validator_list_account.validators
                    [validator_index]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                return Some((validator_index, vote_account, history_account));
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let cluster_history = get_cluster_history_address(&validator_history_program_id);

    println!(
        "Validator List Length: {}",
        steward_accounts.validator_list_account.validators.len()
    );
    println!(
        "Validators in state {}",
        steward_accounts.state_account.state.num_pool_validators
    );

    let mut dne_error_count = 0;
    let mut outdated_error_count = 0;
    let mut other_error_count = 0;
    for i in 0..validators_to_run.len() {
        let (validator_index, vote_account, history_account) = &validators_to_run[i];

        // println!("index: {}", validator_index);
        // println!("vote_account: {}", vote_account);
        // println!("config: {}", steward_config);
        // println!("state: {}", steward_accounts.state_address);
        // println!("history: {}", history_account);
        // println!(
        //     "validator_list: {}",
        //     steward_accounts.validator_list_address
        // );
        // println!("cluster_history: {}", cluster_history);
        // println!("signer: {}", arc_payer.pubkey());

        let ix_to_run = Instruction {
            program_id: program_id,
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: steward_config,
                state_account: steward_accounts.state_address,
                validator_history: *history_account,
                validator_list: steward_accounts.validator_list_address,
                cluster_history: cluster_history,
                signer: arc_payer.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: *validator_index,
            }
            .data(),
        };

        let ixs_to_run = vec![ix_to_run];

        let txs_to_run =
            package_instructions(&ixs_to_run, 1, Some(args.priority_fee), Some(200_000), None);

        let result =
            debug_send_single_transaction(&arc_client, &arc_payer, &txs_to_run[0], None).await;

        match result {
            Ok(signature) => {
                println!("Signature: {}", signature);
            }
            Err(e) => {
                let error_string = &format!("{:?}", e.get_transaction_error());

                let mut output = String::new();

                if error_string.contains("6011") {
                    output += &format!("\nError: 6011\n");
                    output += &format!("Index: {}\n", validator_index);
                    output += &format!("Vote Account: {}\n", vote_account);
                    output += &format!("History Account: {}\n", history_account);
                    output += &format!("Message: Validator History vote data not recent enough to be used for scoring. Must be updated this epoch.");
                    outdated_error_count += 1;
                } else if error_string.contains("3007") {
                    output += &format!("\nError: 3007\n");
                    output += &format!("Index: {}\n", validator_index);
                    output += &format!("Vote Account: {}\n", vote_account);
                    output += &format!("History Account: {}\n", history_account);
                    output += &format!("Message: Validator History Account DNE.");
                    dne_error_count += 1;
                } else if !error_string.contains("BlockhashNotFound") {
                    output += &format!("\nError: Unknown\n");
                    output += &format!("Index: {}\n", validator_index);
                    output += &format!("Vote Account: {}\n", vote_account);
                    output += &format!("History Account: {}\n", history_account);
                    output += &format!("Message: {:?}", e);
                    other_error_count += 1;
                }

                if output.len() > 0 {
                    println!("{}", output);
                }
            }
        }
    }

    println!("6011 Count (Old): {}", outdated_error_count);
    println!("3007 Count (DNE): {}", dne_error_count);
    println!("???? Count (???): {}", other_error_count);

    // let ixs_to_run = validators_to_run
    //     .iter()
    //     .map(|(validator_index, _, history_account)| Instruction {
    //         program_id: program_id,
    //         accounts: jito_steward::accounts::ComputeInstantUnstake {
    //             config: steward_config,
    //             state_account: steward_accounts.state_address,
    //             validator_history: *history_account,
    //             validator_list: steward_accounts.validator_list_address,
    //             cluster_history: cluster_history,
    //             signer: arc_payer.pubkey(),
    //         }
    //         .to_account_metas(None),
    //         data: jito_steward::instruction::ComputeInstantUnstake {
    //             validator_list_index: *validator_index,
    //         }
    //         .data(),
    //     })
    //     .collect::<Vec<Instruction>>();

    // let txs_to_run =
    //     package_instructions(&ixs_to_run, 1, Some(args.priority_fee), Some(200_000), None);

    // println!("Submitting {} instructions", ixs_to_run.len());
    // println!("Submitting {} transactions", txs_to_run.len());

    // let submit_stats = submit_transactions(&arc_client, txs_to_run, &arc_payer).await?;

    // println!("Submit stats: {:?}", submit_stats);

    Ok(())
}

fn _package_compute_instant_unstake(
    ixs: &Vec<Instruction>,
    priority_fee: u64,
) -> Vec<Vec<Instruction>> {
    ixs.chunks(15)
        .map(|chunk: &[Instruction]| {
            let mut chunk_vec = chunk.to_vec();
            chunk_vec.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            );
            chunk_vec.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
            );

            chunk_vec
        })
        .collect::<Vec<Vec<Instruction>>>()
}
