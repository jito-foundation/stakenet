use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::{constants::MAX_ALLOC_BYTES, StewardStateAccount, StewardStateAccountV2};
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

use stakenet_sdk::utils::{
    accounts::{
        get_directed_stake_whitelist_address, get_stake_pool_account, get_steward_config_account,
        get_steward_state_address,
    },
    transactions::{configure_instruction, print_base58_tx},
};

use crate::commands::command_args::ReallocDirectedStakeWhitelist;

const REALLOCS_PER_TX: usize = 10;

pub async fn command_realloc_directed_stake_whitelist(
    args: ReallocDirectedStakeWhitelist,
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

    let directed_staking_whitelist =
        get_directed_stake_whitelist_address(&steward_config, &program_id);

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
    let whats_left = StewardStateAccountV2::SIZE - data_length.min(StewardStateAccountV2::SIZE);

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
            directed_staking_whitelist,
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
            args.permissioned_parameters.transaction_parameters.print_tx,
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
    directed_stake_whitelist: Pubkey,
    steward_config: &Pubkey,
    validator_list: &Pubkey,
    count: usize,
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    heap_size: Option<u32>,
    print_tx: bool,
) -> Result<Signature> {
    let ixs = vec![
        Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ReallocDirectedStakeWhitelist {
                directed_stake_whitelist,
                config: *steward_config,
                validator_list: *validator_list,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: authority.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ReallocDirectedStakeWhitelist {}.data(),
        };
        count
    ];

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(&ixs, priority_fee, compute_limit, heap_size);

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let mut signature = Signature::default();
    if print_tx {
        print_base58_tx(&configured_ix);
    } else {
        signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(signature)
}
