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
    commands::commands::InitState,
    utils::accounts::{get_stake_pool_account, get_steward_state_address},
};

const MAX_REALLOCS: usize = (StewardStateAccount::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
const REALLOCS_PER_TX: usize = 10;

pub async fn command_init_state(
    args: InitState,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.steward_config;

    let steward_state = get_steward_state_address(&program_id, &steward_config);

    let stake_pool_account = get_stake_pool_account(&client, &args.stake_pool).await?;

    let validator_list = stake_pool_account.validator_list;

    let mut reallocs_left_to_run = MAX_REALLOCS;
    let mut should_create = true;

    match client.get_account(&steward_state).await {
        Ok(steward_state_account_raw) => {
            if steward_state_account_raw.data.len() == StewardStateAccount::SIZE {
                match StewardStateAccount::try_deserialize(
                    &mut steward_state_account_raw.data.as_slice(),
                ) {
                    Ok(steward_state_account) => {
                        if steward_state_account.is_initialized.into() {
                            println!("State account already exists");
                            return Ok(());
                        }
                    }
                    Err(_) => { /* Account is not initialized, continue */ }
                };
            }

            // if it already exists, we don't need to create it
            should_create = false;

            let data_length = steward_state_account_raw.data.len();
            let whats_left = StewardStateAccount::SIZE - data_length.min(StewardStateAccount::SIZE);

            reallocs_left_to_run =
                (whats_left.max(MAX_ALLOC_BYTES) - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        }
        Err(_) => { /* Account does not exist, continue */ }
    }

    if should_create {
        let signature = _create_state(
            &client,
            &program_id,
            &authority,
            &steward_state,
            &steward_config,
        )
        .await?;

        println!("Created Steward State: {}", signature);
    }

    let reallocs_to_run = reallocs_left_to_run;
    let mut reallocs_ran = 0;

    while reallocs_left_to_run > 0 {
        let reallocs_per_transaction = reallocs_left_to_run.min(REALLOCS_PER_TX);

        let signature = _realloc_x_times(
            &client,
            &program_id,
            &authority,
            &steward_state,
            &steward_config,
            &validator_list,
            reallocs_per_transaction,
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

async fn _create_state(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    steward_state: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Signature> {
    let init_ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::InitializeState {
            state_account: *steward_state,
            config: *steward_config,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeState {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let transaction = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    Ok(signature)
}

async fn _realloc_x_times(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    steward_state: &Pubkey,
    steward_config: &Pubkey,
    validator_list: &Pubkey,
    count: usize,
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

    let transaction = Transaction::new_signed_with_payer(
        &ixs,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    Ok(signature)
}
