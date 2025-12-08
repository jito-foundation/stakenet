use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use stakenet_sdk::{
    models::{errors::JitoTransactionError, submit_stats::SubmitStats},
    utils::{
        accounts::{get_all_steward_accounts, get_directed_stake_meta_address},
        transactions::configure_instruction,
    },
};

use crate::{
    commands::command_args::PermissionlessParameters,
    utils::transactions::submit_packaged_transactions,
};

#[derive(Parser)]
#[command(about = "Crank `idle` state")]
pub struct CrankInstantRemoveValidators {
    #[command(flatten)]
    pub permissionless_parameters: PermissionlessParameters,
}

pub async fn command_crank_instant_remove_validators(
    args: CrankInstantRemoveValidators,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let args = args.permissionless_parameters;

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");
    let payer = Arc::new(payer);

    let steward_config = args.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_for_immediate_removal;

    let mut stats = SubmitStats::default();

    while validators_to_remove.count() != 0 {
        let mut validator_index_to_remove = None;
        for i in 0..all_steward_accounts.validator_list_account.validators.len() as u64 {
            if validators_to_remove.get(i as usize).map_err(|e| {
                JitoTransactionError::Custom(format!(
                    "Error fetching bitmask index for immediate removed validator: {}/{} - {}",
                    i, num_validators, e
                ))
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        log::info!("Validator Index to Remove: {:?}", validator_index_to_remove);

        let directed_stake_meta =
            get_directed_stake_meta_address(&all_steward_accounts.config_address, &program_id);

        let ix = Instruction {
            program_id,
            accounts: jito_steward::accounts::InstantRemoveValidator {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
                directed_stake_meta,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InstantRemoveValidator {
                validator_index_to_remove: validator_index_to_remove.unwrap(),
            }
            .data(),
        };

        let configured_ix = configure_instruction(&[ix], None, Some(1_400_000), None);

        log::info!("Submitting Instant Removal");
        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], &payer, Some(50), None)
                .await?;

        stats.combine(&new_stats);
    }

    Ok(())
}
