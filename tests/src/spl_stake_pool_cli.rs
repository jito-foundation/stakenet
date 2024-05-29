#![allow(deprecated)]
// Copied and from solana-program-library/stake-pool/cli/src/main.rs
// Modified to not make any RPC calls
use anchor_lang::solana_program::system_instruction;
use solana_program::{
    borsh0_10::{get_instance_packed_len, get_packed_len},
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    stake,
};
use solana_sdk::signature::{Keypair, Signer};
use spl_stake_pool::{
    find_withdraw_authority_program_address,
    state::{Fee, StakePool, ValidatorList},
};

// use instruction::create_associated_token_account once ATA 1.0.5 is released
#[allow(deprecated)]
use spl_associated_token_account::{
    create_associated_token_account, get_associated_token_address_with_program_id,
};

const STAKE_STATE_LEN: usize = 200;
macro_rules! unique_signers {
    ($vec:ident) => {
        $vec.sort_by_key(|l| l.pubkey());
        $vec.dedup();
    };
}

type Error = Box<dyn std::error::Error>;
pub struct CliConfig {
    pub manager: Keypair,
    pub staker: Keypair,
    pub funding_authority: Option<Keypair>,
    pub token_owner: Keypair,
    pub fee_payer: Keypair,
    pub dry_run: bool,
    pub no_update: bool,
}

pub type TransactionAndSigners = (Vec<Instruction>, Vec<Keypair>);

#[allow(clippy::too_many_arguments)]
pub fn command_create_pool(
    config: &CliConfig,
    deposit_authority: Option<Keypair>,
    epoch_fee: Fee,
    withdrawal_fee: Fee,
    deposit_fee: Fee,
    referral_fee: u8,
    max_validators: u32,
    stake_pool_keypair: Keypair,
    validator_list_keypair: Keypair,
    mint_keypair: Keypair,
    reserve_keypair: Keypair,
    unsafe_fees: bool,
    stake_pool_program_id: Pubkey,
) -> Result<Vec<TransactionAndSigners>, Error> {
    if !unsafe_fees {
        check_stake_pool_fees(&epoch_fee, &withdrawal_fee, &deposit_fee)?;
    }
    println!("Creating reserve stake {}", reserve_keypair.pubkey());

    println!("Creating mint {}", mint_keypair.pubkey());

    // let reserve_stake_balance = config
    //     .rpc_client
    //     .get_minimum_balance_for_rent_exemption(STAKE_STATE_LEN)?
    //     + MINIMUM_RESERVE_LAMPORTS;
    // let mint_account_balance = config
    //     .rpc_client
    //     .get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)?;
    // let pool_fee_account_balance = config
    //     .rpc_client
    //     .get_minimum_balance_for_rent_exemption(spl_token::state::Account::LEN)?;
    // let stake_pool_account_lamports = config
    //     .rpc_client
    //     .get_minimum_balance_for_rent_exemption(get_packed_len::<StakePool>())?;
    let empty_validator_list = ValidatorList::new(max_validators);
    let validator_list_size = get_instance_packed_len(&empty_validator_list)?;
    // let validator_list_balance = config
    //     .rpc_client
    //     .get_minimum_balance_for_rent_exemption(validator_list_size)?;

    let default_decimals = spl_token::native_mint::DECIMALS;

    // Calculate withdraw authority used for minting pool tokens
    let (withdraw_authority, _) = find_withdraw_authority_program_address(
        &stake_pool_program_id,
        &stake_pool_keypair.pubkey(),
    );

    let mut setup_instructions = vec![
        // Account for the stake pool reserve - initial pool balance needed to set up all stake accounts
        system_instruction::create_account(
            &config.fee_payer.pubkey(),
            &reserve_keypair.pubkey(),
            // reserve_stake_balance,
            50_000_000_000, // 50 SOL
            STAKE_STATE_LEN as u64,
            &stake::program::id(),
        ),
        stake::instruction::initialize(
            &reserve_keypair.pubkey(),
            &stake::state::Authorized {
                staker: withdraw_authority,
                withdrawer: withdraw_authority,
            },
            &stake::state::Lockup::default(),
        ),
        // Account for the stake pool mint
        system_instruction::create_account(
            &config.fee_payer.pubkey(),
            &mint_keypair.pubkey(),
            // mint_account_balance,
            1_000_000_000,
            spl_token::state::Mint::LEN as u64,
            &spl_token::id(),
        ),
        // Initialize pool token mint account
        spl_token::instruction::initialize_mint(
            &spl_token::id(),
            &mint_keypair.pubkey(),
            &withdraw_authority,
            None,
            default_decimals,
        )?,
    ];

    let pool_fee_account = add_associated_token_account(
        config,
        &mint_keypair.pubkey(),
        &config.manager.pubkey(),
        &mut setup_instructions,
    );

    let initialize_instructions = vec![
        // Validator stake account list storage
        system_instruction::create_account(
            &config.fee_payer.pubkey(),
            &validator_list_keypair.pubkey(),
            // validator_list_balance,
            10_000_000_000,
            validator_list_size as u64,
            &stake_pool_program_id,
        ),
        // Account for the stake pool
        system_instruction::create_account(
            &config.fee_payer.pubkey(),
            &stake_pool_keypair.pubkey(),
            // stake_pool_account_lamports,
            1_000_000_000,
            get_packed_len::<StakePool>() as u64,
            &stake_pool_program_id,
        ),
        // Initialize stake pool
        spl_stake_pool::instruction::initialize(
            &stake_pool_program_id,
            &stake_pool_keypair.pubkey(),
            &config.manager.pubkey(),
            &config.staker.pubkey(),
            &withdraw_authority,
            &validator_list_keypair.pubkey(),
            &reserve_keypair.pubkey(),
            &mint_keypair.pubkey(),
            &pool_fee_account,
            &spl_token::id(),
            deposit_authority.as_ref().map(|x| x.pubkey()),
            epoch_fee,
            withdrawal_fee,
            deposit_fee,
            referral_fee,
            max_validators,
        ),
    ];

    let mut result = vec![];

    let mut setup_signers = vec![
        config.fee_payer.insecure_clone(),
        mint_keypair,
        reserve_keypair,
    ];
    unique_signers!(setup_signers);
    result.push((setup_instructions, setup_signers));
    let mut initialize_signers = vec![
        config.fee_payer.insecure_clone(),
        stake_pool_keypair,
        validator_list_keypair,
        config.manager.insecure_clone(),
    ];
    if let Some(deposit_authority) = deposit_authority {
        println!(
            "Deposits will be restricted to {} only, this can be changed using the set-funding-authority command.",
            deposit_authority.pubkey()
        );
        let mut initialize_signers = initialize_signers
            .iter()
            .map(|x| x.insecure_clone())
            .collect::<Vec<_>>();
        initialize_signers.push(deposit_authority);
        unique_signers!(initialize_signers);
    } else {
        unique_signers!(initialize_signers);
    };
    result.push((initialize_instructions.clone(), initialize_signers));

    Ok(result)
}

fn add_associated_token_account(
    config: &CliConfig,
    mint: &Pubkey,
    owner: &Pubkey,
    instructions: &mut Vec<Instruction>,
) -> Pubkey {
    // Account for tokens not specified, creating one
    let account = get_associated_token_address(owner, mint);

    #[allow(deprecated)]
    instructions.push(create_associated_token_account(
        &config.fee_payer.pubkey(),
        owner,
        mint,
    ));

    account
}

fn check_stake_pool_fees(
    epoch_fee: &Fee,
    withdrawal_fee: &Fee,
    deposit_fee: &Fee,
) -> Result<(), Error> {
    if epoch_fee.numerator == 0 || epoch_fee.denominator == 0 {
        return Err("Epoch fee should not be 0. ".into());
    }
    let is_withdrawal_fee_zero = withdrawal_fee.numerator == 0 || withdrawal_fee.denominator == 0;
    let is_deposit_fee_zero = deposit_fee.numerator == 0 || deposit_fee.denominator == 0;
    if is_withdrawal_fee_zero && is_deposit_fee_zero {
        return Err("Withdrawal and deposit fee should not both be 0.".into());
    }
    Ok(())
}

/// Derives the associated token account address for the given wallet address and token mint
pub fn get_associated_token_address(
    wallet_address: &Pubkey,
    token_mint_address: &Pubkey,
) -> Pubkey {
    get_associated_token_address_with_program_id(
        wallet_address,
        token_mint_address,
        &spl_token::id(),
    )
}
