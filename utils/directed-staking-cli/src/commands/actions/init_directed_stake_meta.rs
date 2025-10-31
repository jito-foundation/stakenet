use crate::commands::command_args::InitDirectedStakeMeta;
use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::state::directed_stake::DirectedStakeMeta;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use stakenet_sdk::utils::transactions::{configure_instruction, print_base58_tx};
use std::sync::Arc;

pub async fn command_init_directed_stake_meta(
    args: InitDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority_keypair = read_keypair_file(&args.authority_keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair: {}", e))?;
    let authority_pubkey = authority_keypair.pubkey();

    let (directed_stake_meta_pda, _bump) = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, args.steward_config.as_ref()],
        &program_id,
    );

    println!("Initializing DirectedStakeMeta...");
    println!("  Authority: {}", authority_pubkey);
    println!("  Steward Config: {}", args.steward_config);
    // println!("  Total Stake Targets: {}", args.total_stake_targets);
    println!("  DirectedStakeMeta PDA: {}", directed_stake_meta_pda);

    let instruction = Instruction {
        program_id,
        accounts: jito_steward::accounts::InitializeDirectedStakeMeta {
            config: args.steward_config,
            directed_stake_meta: directed_stake_meta_pda,
            clock: solana_program::sysvar::clock::ID,
            system_program: solana_sdk::system_program::ID,
            authority: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeDirectedStakeMeta {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[instruction],
        args.transaction_parameters.priority_fee,
        args.transaction_parameters.compute_limit,
        args.transaction_parameters.heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority_pubkey),
        &[&authority_keypair],
        blockhash,
    );

    if args.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("✅ DirectedStakeMeta initialized successfully!");
        println!("  Transaction signature: {}", signature);
        println!("  DirectedStakeMeta account: {}", directed_stake_meta_pda);
    }

    Ok(())
}
