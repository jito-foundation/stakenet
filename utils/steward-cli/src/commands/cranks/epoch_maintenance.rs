use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{anyhow, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::commands::command_args::CrankEpochMaintenance;
use stakenet_sdk::utils::{
    accounts::{
        get_all_steward_accounts, get_directed_stake_meta_address, get_steward_state_account,
    },
    transactions::{configure_instruction, print_base58_tx},
};

pub async fn command_crank_epoch_maintenance(
    args: CrankEpochMaintenance,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let explicit_index = args.validator_index_to_remove;
    let args = args.permissionless_parameters;

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let steward_config = args.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let directed_stake_meta = get_directed_stake_meta_address(&steward_config, &program_id);

    let mut state_account = all_steward_accounts.state_account;
    let mut epoch = client.get_epoch_info().await?.epoch;

    loop {
        if epoch == state_account.state.current_epoch {
            println!("Epoch is the same as the current epoch: {epoch}");
            return Ok(());
        }

        // Epoch maintenance only advances the epoch once both removal bitmasks are empty, and it
        // drains them one index per instruction. `InstantRemoveValidator` cannot drain
        // `validators_for_immediate_removal` in the meantime because it requires the epoch to
        // have already advanced, so both masks have to be drained through this instruction.
        let validator_index_to_remove = match explicit_index {
            Some(index) => Some(index),
            None => {
                // `remove_validator` accepts any index below
                // `num_pool_validators + validators_added`
                let num_validators = state_account.state.num_pool_validators
                    + state_account.state.validators_added as u64;

                let mut marked_index = None;
                for i in 0..num_validators {
                    let index = i as usize;
                    let marked = state_account
                        .state
                        .validators_to_remove
                        .get(index)
                        .map_err(|e| {
                            anyhow!("Error fetching validators_to_remove index {index}: {e}")
                        })?
                        || state_account
                            .state
                            .validators_for_immediate_removal
                            .get(index)
                            .map_err(|e| {
                                anyhow!(
                                "Error fetching validators_for_immediate_removal index {index}: {e}"
                            )
                            })?;

                    if marked {
                        marked_index = Some(i);
                        break;
                    }
                }
                marked_index
            }
        };

        println!(
            "Running epoch maintenance state_epoch={} current_epoch={epoch} validators_to_remove={} validators_for_immediate_removal={} validator_index_to_remove={validator_index_to_remove:?}",
            state_account.state.current_epoch,
            state_account.state.validators_to_remove.count(),
            state_account.state.validators_for_immediate_removal.count(),
        );

        let ix = Instruction {
            program_id,
            accounts: jito_steward::accounts::EpochMaintenance {
                config: steward_config,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
                directed_stake_meta,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::EpochMaintenance {
                validator_index_to_remove,
            }
            .data(),
        };

        // Removing a validator shifts the whole state array, so it needs more than the default
        let compute_limit = args
            .transaction_parameters
            .compute_limit
            .or_else(|| validator_index_to_remove.map(|_| 1_400_000));

        let configured_ix = configure_instruction(
            &[ix],
            args.transaction_parameters.priority_fee,
            compute_limit,
            args.transaction_parameters.heap_size,
        );

        if args.transaction_parameters.print_tx {
            print_base58_tx(&configured_ix);
            return Ok(());
        }

        let blockhash = client.get_latest_blockhash().await?;

        let transaction = Transaction::new_signed_with_payer(
            &configured_ix,
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        );

        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {signature}");

        // An explicitly requested index stays a single-shot operation
        if explicit_index.is_some() {
            return Ok(());
        }

        let previous_state_epoch = state_account.state.current_epoch;
        state_account = get_steward_state_account(client, &program_id, &steward_config).await?;
        epoch = client.get_epoch_info().await?.epoch;

        // Looping is only productive while there is something left to drain: each pass with an
        // index shrinks a bitmask. Without one, the epoch should have advanced, so a repeat is a
        // wedge and would otherwise spin submitting transactions indefinitely.
        if validator_index_to_remove.is_none()
            && state_account.state.current_epoch == previous_state_epoch
        {
            return Err(anyhow!(
                "Epoch maintenance did not advance the epoch and nothing is marked for removal: state_epoch={} current_epoch={epoch}",
                state_account.state.current_epoch
            ));
        }
    }
}
