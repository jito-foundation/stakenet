use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::{constants::MAX_ALLOC_BYTES, StewardStateAccount};
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

use crate::{
    commands::command_args::InitState,
    utils::{
        accounts::{get_stake_pool_account, get_steward_config_account, get_steward_state_address},
        transactions::configure_instruction,
    },
};

const REALLOCS_PER_TX: usize = 10;

pub async fn command_init_state(
    args: InitState,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.permissioned_parameters.steward_config;
    let steward_config_account =
        get_steward_config_account(client, &args.permissioned_parameters.steward_config).await?;

    let steward_state = get_steward_state_address(&program_id, &steward_config);

    let stake_pool_account =
        get_stake_pool_account(client, &steward_config_account.stake_pool).await?;

    let validator_list = stake_pool_account.validator_list;

    let steward_state_account_raw = client.get_account(&steward_state).await?;

    if steward_state_account_raw.data.len() == StewardStateAccount::SIZE {
        match StewardStateAccount::try_deserialize(&mut steward_state_account_raw.data.as_slice()) {
            Ok(steward_state_account) => {
                if steward_state_account.is_initialized.into() {
                    println!("State account already exists");
                    return Ok(());
                }
            }
            Err(_) => { /* Account is not initialized, continue */ }
        };
    }

    let data_length = steward_state_account_raw.data.len();
    let whats_left = StewardStateAccount::SIZE - data_length.min(StewardStateAccount::SIZE);

    let mut reallocs_left_to_run =
        (whats_left.max(MAX_ALLOC_BYTES) - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;

    let reallocs_to_run = reallocs_left_to_run;
    let mut reallocs_ran = 0;

    while reallocs_left_to_run > 0 {
        let reallocs_per_transaction = reallocs_left_to_run.min(REALLOCS_PER_TX);

        let signature = _realloc_x_times(
            client,
            &program_id,
            &authority,
            &steward_state,
            &steward_config,
            &validator_list,
            reallocs_per_transaction,
            args.permissioned_parameters
                .transaction_parameters
                .priority_fee,
            args.permissioned_parameters
                .transaction_parameters
                .compute_limit,
            args.permissioned_parameters
                .transaction_parameters
                .heap_size,
        )
        .await?;

        reallocs_left_to_run -= reallocs_per_transaction;
        reallocs_ran += reallocs_per_transaction;

        println!(
            "{}/{}: Signature: {}",
            reallocs_ran, reallocs_to_run, signature
        );
    }

    println!("Steward State: {}", steward_state);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn _realloc_x_times(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    steward_state: &Pubkey,
    steward_config: &Pubkey,
    validator_list: &Pubkey,
    count: usize,
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    heap_size: Option<u32>,
) -> Result<Signature> {
    let ixs = vec![
        Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ReallocState {
                state_account: *steward_state,
                config: *steward_config,
                validator_list: *validator_list,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: authority.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ReallocState {}.data(),
        };
        count
    ];

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ixs = configure_instruction(&ixs, priority_fee, compute_limit, heap_size);

    let transaction = Transaction::new_signed_with_payer(
        &configured_ixs,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    Ok(signature)
}
