use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use keeper_core::submit_instructions_no_packing;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use validator_history::id as validator_history_id;

use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};

use crate::{
    commands::commands::CrankComputeScore,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
            UsefulStewardAccounts,
        },
        print::state_tag_to_string,
    },
};

pub async fn command_crank_compute_score(
    args: CrankComputeScore,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let validator_history_program_id = validator_history_id();
    let steward_config = args.steward_config;

    let UsefulStewardAccounts {
        config_account: _,
        state_account,
        state_address,
        stake_pool_account: _,
        stake_pool_address: _,
        validator_list_account,
        validator_list_address,
    } = get_all_steward_accounts(&client, &program_id, &steward_config).await?;

    match state_account.state.state_tag {
        StewardStateEnum::ComputeScores => { /* Continue */ }
        _ => {
            println!(
                "State account is not in ComputeScores state: {}",
                state_tag_to_string(state_account.state.state_tag)
            );
            return Ok(());
        }
    }

    let validators_to_run = (0..validator_list_account.validators.len())
        .filter_map(|validator_index| {
            let has_been_scored = state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_scored {
                return None;
            } else {
                let vote_account =
                    validator_list_account.validators[validator_index].vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                return Some((validator_index, vote_account, history_account));
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let cluster_history = get_cluster_history_address(&validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|(validator_index, _, history_account)| Instruction {
            program_id: program_id,
            accounts: jito_steward::accounts::ComputeScore {
                config: steward_config,
                state_account: state_address,
                validator_history: *history_account,
                validator_list: validator_list_address,
                cluster_history: cluster_history,
                signer: payer.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: *validator_index,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    println!("Submitting {} instructions", ixs_to_run.len());

    submit_instructions_no_packing(
        &Arc::new(client),
        ixs_to_run,
        &Arc::new(payer),
        args.priority_fee,
        Some(150_000),
    )
    .await?;

    Ok(())
}