use std::collections::HashMap;

use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

/// Basic integration test
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    AnchorDeserialize, InstructionData, ToAccountMetas,
};
use jito_steward::{
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    utils::{StakePool, ValidatorList},
    Config, Delegation, StewardStateAccount, StewardStateEnum, UpdateParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, epoch_schedule::EpochSchedule,
    signer::Signer, stake::state::StakeStateV2, transaction::Transaction,
};
use spl_stake_pool::{
    minimum_delegation,
    state::{AccountType, ValidatorListHeader, ValidatorStakeInfo},
};
use tests::steward_fixtures::{validator_history_default, FixtureDefaultAccounts, TestFixture};

#[tokio::test]
async fn test_cycle() {
    let fixture_accounts = FixtureDefaultAccounts::default();

    let fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let mut ctx = &fixture.ctx;

    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    println!("Steward state: {}", steward_state_account.state.state_tag);

    fixture.advance_num_epochs(2, 4).await;

    let pause_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[pause_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
    fixture.advance_num_epochs(2, 4).await;

    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    println!("Steward state: {}", steward_state_account.state.state_tag);
    println!("Bump: {}", steward_state_account.bump);
    println!("Paused: {}", config.is_paused());

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    println!("Clock: {:?}", clock);

    drop(fixture);
}
