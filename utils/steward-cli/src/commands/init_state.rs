use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use jito_steward::{constants::MAX_ALLOC_BYTES, utils::StakePool, StewardStateAccount};
use solana_client::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};

use super::commands::InitState;

const MAX_REALLOCS: usize = (StewardStateAccount::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
const REALLOCS_PER_TX: usize = 10;

pub fn command_init_state(args: InitState, client: RpcClient, program_id: Pubkey) {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.steward_config;

    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        &jito_steward::id(),
    );

    let stake_pool_account_raw = client
        .get_account(&args.stake_pool)
        .expect("Could not load stake pool account");

    let stake_pool_account =
        StakePool::try_deserialize(&mut stake_pool_account_raw.data.as_slice())
            .expect("Could not deserialize stake pool account");

    let validator_list = stake_pool_account.validator_list;

    let mut reallocs_left_to_run = MAX_REALLOCS;
    let mut should_create = true;

    match client.get_account(&steward_state) {
        Ok(steward_state_account_raw) => {
            if steward_state_account_raw.data.len() == StewardStateAccount::SIZE {
                match StewardStateAccount::try_deserialize(
                    &mut steward_state_account_raw.data.as_slice(),
                ) {
                    Ok(steward_state_account) => {
                        if steward_state_account.is_initialized.into() {
                            println!("State account already exists");
                            return;
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
        );

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
        );

        reallocs_left_to_run -= reallocs_per_transaction;
        reallocs_ran += reallocs_per_transaction;

        println!(
            "{}/{}: Signature: {}",
            reallocs_ran, reallocs_to_run, signature
        );
    }

    println!("Steward State: {}", steward_state);
}

fn _create_state(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    steward_state: &Pubkey,
    steward_config: &Pubkey,
) -> Signature {
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

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction")
}

fn _realloc_x_times(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    steward_state: &Pubkey,
    steward_config: &Pubkey,
    validator_list: &Pubkey,
    count: usize,
) -> Signature {
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

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &ixs,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction")
}
