use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_program::instruction::Instruction;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::accounts::get_all_steward_accounts;

pub async fn command_process_immediate_removals(
    payer_keypair_path: String,
    steward_config: Pubkey,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
    dry_run: bool,
    _batch_size: usize, // Ignored - always process one at a time
) -> Result<()> {
    let payer =
        read_keypair_file(&payer_keypair_path).expect("Failed reading keypair file (Payer)");

    let mut total_removed = 0;
    let mut iteration = 0;

    loop {
        iteration += 1;

        // Get fresh state with finalized commitment
        let steward_accounts =
            get_all_steward_accounts(client, &program_id, &steward_config).await?;

        // Count validators marked for immediate removal
        let validators_to_remove_count = steward_accounts
            .state_account
            .state
            .validators_for_immediate_removal
            .count();

        println!("\n=== Iteration {} ===", iteration);
        println!(
            "Validators still marked for immediate removal: {}",
            validators_to_remove_count
        );
        println!(
            "Current num_pool_validators: {}",
            steward_accounts.state_account.state.num_pool_validators
        );
        println!(
            "Current validator list length: {}",
            steward_accounts.validator_list_account.validators.len()
        );

        if validators_to_remove_count == 0 {
            println!("\n✅ All validators have been removed!");
            println!("Total validators removed: {}", total_removed);
            break;
        }

        // Find up to 2 validators marked for removal
        let mut validators_to_remove = Vec::new();
        let validator_list_len = steward_accounts.validator_list_account.validators.len();
        let batch_size = 2; // Hardcoded batch size

        for i in 0..validator_list_len {
            if steward_accounts
                .state_account
                .state
                .validators_for_immediate_removal
                .get(i)
                .unwrap_or(false)
            {
                validators_to_remove.push(i);
                if validators_to_remove.len() >= batch_size {
                    break; // We have our batch of 2
                }
            }
        }

        if validators_to_remove.is_empty() {
            println!("❌ No validators found despite count > 0. State may be inconsistent.");
            break;
        }

        if validators_to_remove.len() == 1 {
            println!("Removing validator at index {}...", validators_to_remove[0]);
        } else {
            println!("Removing {} validators at indices: {:?}...",
                     validators_to_remove.len(), validators_to_remove);
        }

        if dry_run {
            println!("Dry run mode - would remove validators at indices: {:?}", validators_to_remove);
            total_removed += validators_to_remove.len();
            println!("Would have removed {} validators total so far", total_removed);

            // In dry run, show estimated iterations
            let remaining_after_batch = validators_to_remove_count - validators_to_remove.len();
            let estimated_iterations = (remaining_after_batch + 1) / 2; // Ceiling division for batch size 2
            println!("Would need approximately {} more iterations to remove all {} remaining validators",
                     estimated_iterations, remaining_after_batch);

            println!("\nDry run complete. First batch would remove indices: {:?}.", validators_to_remove);
            break;
        }

        // Build removal instructions
        let mut instructions = Vec::new();

        // Add compute budget instructions
        let compute_budget_ix =
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                1_400_000u32,
            );
        instructions.push(compute_budget_ix);

        let priority_fee_ix =
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
                10_000,
            );
        instructions.push(priority_fee_ix);

        // Add removal instructions for each validator in the batch
        for (batch_index, validator_index) in validators_to_remove.iter().enumerate() {
            // For the second instruction, we need to adjust the index
            // because removing the first validator shifts all subsequent indices down by 1
            // Since we iterate through indices in ascending order, the second index
            // will always be higher than the first, so we always decrement by 1
            let adjusted_index = if batch_index == 0 {
                *validator_index
            } else {
                // The second validator's index needs to be decremented by 1
                // because the first removal shifts everything down
                validator_index - 1
            };

            let ix = Instruction {
                program_id,
                accounts: jito_steward::accounts::InstantRemoveValidator {
                    config: steward_config,
                    state_account: steward_accounts.state_address,
                    validator_list: steward_accounts.validator_list_address,
                    stake_pool: steward_accounts.stake_pool_address,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::InstantRemoveValidator {
                    validator_index_to_remove: adjusted_index as u64,
                }
                .data(),
            };

            instructions.push(ix);
        }

        let blockhash = client.get_latest_blockhash().await?;

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        );

        match client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::finalized(), // Use finalized to ensure we see the changes
                RpcSendTransactionConfig::default(),
            )
            .await
        {
            Ok(signature) => {
                if validators_to_remove.len() == 1 {
                    println!("✓ Successfully removed validator at index {}", validators_to_remove[0]);
                } else {
                    println!("✓ Successfully removed {} validators at indices: {:?}",
                             validators_to_remove.len(), validators_to_remove);
                }
                println!("  Transaction signature: {}", signature);
                total_removed += validators_to_remove.len();

                // Small delay to avoid rate limiting
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                println!("✗ Failed to remove validators at indices {:?}: {}",
                         validators_to_remove, e);

                // Wait longer on error
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }

        // Safety check to prevent infinite loop
        if iteration > 2000 {
            println!("❌ Too many iterations. Stopping for safety.");
            break;
        }
    }

    println!("\n=========================================");
    println!("Process complete!");
    println!("Total validators successfully removed: {}", total_removed);

    // Final state check
    println!("\nFinal state check:");
    let final_state = get_all_steward_accounts(client, &program_id, &steward_config).await?;
    let remaining = final_state
        .state_account
        .state
        .validators_for_immediate_removal
        .count();

    println!(
        "Validators still marked for immediate removal: {}",
        remaining
    );
    println!(
        "Final num_pool_validators: {}",
        final_state.state_account.state.num_pool_validators
    );
    println!(
        "Final validator list length: {}",
        final_state.validator_list_account.validators.len()
    );

    if remaining > 0 {
        println!("\n⚠️  Some validators still need to be removed. You may need to run this command again.");
    }

    Ok(())
}