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
    // let (steward_state, _) = Pubkey::find_program_address(
    //     &[StewardStateAccount::SEED, steward_config.pubkey().as_ref()],
    //     &jito_steward::id(),
    // );
    // let (cluster_history_account, _) =
    //     Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id());

    // let (validator_history_config, vhc_bump) = Pubkey::find_program_address(
    //     &[validator_history::state::Config::SEED],
    //     &validator_history::id(),
    // );

    // Default parameters from JIP
    let update_parameters_args = UpdateParametersArgs {
        mev_commission_range: Some(20),
        epoch_credits_range: Some(30),
        commission_range: Some(30),
        scoring_delinquency_threshold_ratio: Some(0.85),
        instant_unstake_delinquency_threshold_ratio: Some(0.70),
        mev_commission_bps_threshold: Some(1000),
        commission_threshold: Some(5),
        historical_commission_threshold: Some(50),
        num_delegation_validators: Some(200),
        scoring_unstake_cap_bps: Some(750),
        instant_unstake_cap_bps: Some(10),
        stake_deposit_unstake_cap_bps: Some(10),
        instant_unstake_epoch_progress: Some(0.90),
        compute_score_slot_range: Some(1000),
        instant_unstake_inputs_epoch_progress: Some(0.50),
        num_epochs_between_scoring: Some(10),
        minimum_stake_lamports: Some(5_000_000_000),
        minimum_voting_epochs: Some(5),
    };

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
