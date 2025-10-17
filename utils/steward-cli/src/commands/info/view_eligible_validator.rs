use std::{ops::Add, sync::Arc};

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::pubkey::Pubkey;
use validator_history::ValidatorHistory;

use crate::{commands::command_args::ViewConfig, utils::accounts::get_validator_history_address};
use stakenet_sdk::utils::accounts::get_all_steward_accounts;

pub async fn command_view_eligible_validators(
    args: ViewConfig,
    client: &Arc<RpcClient>,
    steward_program_id: Pubkey,
    validator_history_program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &steward_program_id, &steward_config).await?;

    let mut eligible_validators: u16 = 0;
    let mut solana_labs: u16 = 0;
    let mut jito_labs: u16 = 0;
    let mut firedancers: u16 = 0;
    let mut agaves: u16 = 0;
    let mut bams: u16 = 0;
    let mut others: u16 = 0;

    for validator_index in 0..all_steward_accounts.state_account.state.num_pool_validators {
        let score = all_steward_accounts.state_account.state.scores[validator_index as usize];
        if score > 0 {
            let vote_account = all_steward_accounts.validator_list_account.validators
                [validator_index as usize]
                .vote_account_address;
            let history_address =
                get_validator_history_address(&vote_account, &validator_history_program_id);

            let history_acc_data = client.get_account_data(&history_address).await?;
            let validator_history =
                ValidatorHistory::try_deserialize(&mut history_acc_data.as_slice())?;

            if let Some(latest_history) = validator_history.history.last() {
                match latest_history.client_type {
                    0 => solana_labs = solana_labs.add(1),
                    1 => jito_labs = jito_labs.add(1),
                    2 => firedancers = firedancers.add(1),
                    3 => agaves = agaves.add(1),
                    6 => bams = bams.add(1),
                    _ => others = others.add(1),
                }
            }

            eligible_validators = eligible_validators.add(1);
        }
    }

    println!(
        "Number of validators: {}",
        all_steward_accounts.state_account.state.num_pool_validators
    );
    println!("Eligible Validators: {eligible_validators}");
    println!("Solana Lab: {solana_labs}");
    println!("Jito Lab: {jito_labs}");
    println!("Firedancer: {firedancers}");
    println!("Agave: {agaves}");
    println!("Bam: {bams}");
    println!("Other: {others}");

    Ok(())
}
