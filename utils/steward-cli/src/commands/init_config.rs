use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{Staker, UpdateParametersArgs};
use solana_client::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

use super::commands::InitConfig;

pub fn command_init_config(args: InitConfig, client: RpcClient) {
    // Creates config account
    let keypair =
        read_keypair_file(args.keypair_path).expect("Failed reading keypair file ( Keypair )");
    let steward_config =
        read_keypair_file(args.steward_config_keypair_path).unwrap_or(Keypair::new());
    let stake_pool = read_keypair_file(args.stake_pool_keypair_path).unwrap_or(Keypair::new());

    let (staker, _) = Pubkey::find_program_address(
        &[Staker::SEED, steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    );

    // Default parameters from JIP
    let update_parameters_args: UpdateParametersArgs =
        args.config_parameters.to_update_parameters_args();

    let init_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::InitializeConfig {
            config: steward_config.pubkey(),
            stake_pool: stake_pool.pubkey(),
            staker: staker,
            stake_pool_program: spl_stake_pool::id(),
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeConfig {
            authority: keypair.pubkey(),
            update_parameters_args,
        }
        .data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&keypair.pubkey()),
        &[&keypair, &steward_config],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
}
