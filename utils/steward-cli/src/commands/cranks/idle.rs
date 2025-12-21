use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{anyhow, Result};
use jito_steward::StewardStateEnum;
use solana_client::{nonblocking::rpc_client::RpcClient, tpu_client::TpuClientConfig};
use solana_connection_cache::connection_cache::NewConnectionConfig;
use solana_program::instruction::Instruction;

use solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool};
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use solana_tpu_client::nonblocking::tpu_client::TpuClient;

use crate::commands::command_args::CrankIdle;
use stakenet_sdk::utils::{
    accounts::get_all_steward_accounts,
    transactions::{configure_instruction, print_base58_tx},
};

type QuicTpuClient = TpuClient<QuicPool, QuicConnectionManager, QuicConfig>;

pub async fn command_crank_idle(
    args: CrankIdle,
    rpc_client: &Arc<RpcClient>,
    ws_url: &str,
    program_id: Pubkey,
) -> Result<()> {
    let args = args.permissionless_parameters;

    // Creates config account
    let payer = read_keypair_file(args.payer_keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file ( Payer ): {e}"))?;

    let steward_config = args.steward_config;

    let steward_accounts =
        get_all_steward_accounts(rpc_client, &program_id, &steward_config).await?;

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::Idle => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Idle state: {}",
                steward_accounts.state_account.state.state_tag
            );
            return Ok(());
        }
    }

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::Idle {
            config: steward_config,
            state_account: steward_accounts.state_address,
            validator_list: steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let blockhash = rpc_client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[ix],
        args.transaction_parameters.priority_fee,
        args.transaction_parameters.compute_limit,
        args.transaction_parameters.heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    if args.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let quic_config = QuicConfig::new()?;
        let connection_manager = QuicConnectionManager::new_with_connection_config(quic_config);
        let tpu_config = TpuClientConfig::default();

        let tpu_client: QuicTpuClient = TpuClient::new(
            "tpu-client",
            rpc_client.clone(),
            ws_url,
            tpu_config,
            connection_manager,
        )
        .await?;

        let signature = transaction.signatures[0];
        let errors = tpu_client
            .send_and_confirm_messages_with_spinner(&[transaction.message], &[&payer])
            .await?;

        let mut has_error = false;
        for error in errors {
            if let Some(error) = error {
                println!("Error: {error:?}");
                has_error = true;
            }
        }
        if !has_error {
            println!("Transaction confirmed!: {signature}");
        }
    }

    Ok(())
}
