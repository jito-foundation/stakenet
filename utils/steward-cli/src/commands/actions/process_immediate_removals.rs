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

        // Find the FIRST validator marked for removal
        let mut validator_to_remove = None;
        let validator_list_len = steward_accounts.validator_list_account.validators.len();

        for i in 0..validator_list_len {
            if steward_accounts
                .state_account
                .state
                .validators_for_immediate_removal
                .get(i)
                .unwrap_or(false)
            {
                validator_to_remove = Some(i);
                break; // Take the first one we find
            }
        }

        let Some(validator_index) = validator_to_remove else {
            println!("❌ No validators found despite count > 0. State may be inconsistent.");
            break;
        };

        println!("Removing validator at index {}...", validator_index);

        if dry_run {
            println!("Dry run mode - would remove validator at index: {}", validator_index);
            total_removed += 1;
            println!("Would have removed {} validators total so far", total_removed);

            // In dry run, show estimated iterations
            println!("Would need approximately {} more iterations to remove all {} validators",
                     validators_to_remove_count - 1, validators_to_remove_count);

            println!("\nDry run complete. First removal would be index {}.", validator_index);
            break;
        }

        // Build removal instruction
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

        // Add the removal instruction
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
                validator_index_to_remove: validator_index as u64,
            }
            .data(),
        };

        instructions.push(ix);

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
                println!("✓ Successfully removed validator at index {}", validator_index);
                println!("  Transaction signature: {}", signature);
                total_removed += 1;

                // Small delay to avoid rate limiting
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                println!("✗ Failed to remove validator at index {}: {}", validator_index, e);

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