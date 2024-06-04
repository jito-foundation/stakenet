/// Basic integration test
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    InstructionData, ToAccountMetas,
};
use jito_steward::{utils::ValidatorList, Config, StewardStateAccount};
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer, transaction::Transaction};
use tests::steward_fixtures::{
    closed_vote_account, new_vote_account, serialized_steward_state_account,
    serialized_validator_history_account, system_account, validator_history_default, TestFixture,
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

async fn _auto_add_validator_to_pool(fixture: &TestFixture, vote_account: &Pubkey) {
    let ctx = &fixture.ctx;
    let vote_account = *vote_account;
    let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];
    let validator_history_account = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .0;
    fixture.ctx.borrow_mut().set_account(
        &vote_account,
        &new_vote_account(Pubkey::new_unique(), vote_account, 1, Some(epoch_credits)).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history_default(vote_account, 0)).into(),
    );

    let (validator_history_account, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    );

    let (stake_account_address, _, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let add_validator_to_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account,
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[add_validator_to_pool_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx.clone(), "ValidatorBelowLivenessMinimum")
        .await;

    // fixture.
    let mut validator_history = validator_history_default(vote_account, 0);
    for i in 0..20 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch: i,
            epoch_credits: 400000,
            vote_account_last_update_slot: 100,
            ..ValidatorHistoryEntry::default()
        });
    }
    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history).into(),
    );
    fixture.submit_transaction_assert_success(tx).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let validator_stake_info_idx = validator_list
        .validators
        .iter()
        .position(|&v| v.vote_account_address == vote_account)
        .unwrap();
    assert!(
        validator_list.validators[validator_stake_info_idx].vote_account_address == vote_account
    );
}

#[tokio::test]
async fn test_auto_add_validator_to_pool() {
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    _auto_add_validator_to_pool(&fixture, &Pubkey::new_unique()).await;

    drop(fixture);
}

#[tokio::test]
async fn test_auto_remove() {
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let vote_account = Pubkey::new_unique();

    let (validator_history_account, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    );

    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    // Add vote account
    _auto_add_validator_to_pool(&fixture, &vote_account).await;

    let auto_remove_validator_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoRemoveValidator {
            validator_history_account,
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoRemoveValidatorFromPool {
            validator_list_index: 0,
        }
        .data(),
    };

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    // Fake add vote account to state
    steward_state_account.state.num_pool_validators = 1;
    steward_state_account.state.sorted_score_indices[0] = 0;
    steward_state_account.state.sorted_yield_score_indices[0] = 0;

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let tx = Transaction::new_signed_with_payer(
        &[auto_remove_validator_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx.clone(), "ValidatorNotRemovable")
        .await;

    // "Close" vote account
    fixture
        .ctx
        .borrow_mut()
        .set_account(&vote_account, &closed_vote_account().into());

    fixture.submit_transaction_assert_success(tx).await;

    drop(fixture);
}

#[tokio::test]
async fn test_pause() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    assert!(config.is_paused());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ResumeSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ResumeSteward {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(!config.is_paused());

    drop(fixture);
}

#[tokio::test]
async fn test_blacklist() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AddValidatorToBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorToBlacklist { index: 0 }.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.blacklist.get(0).unwrap());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorFromBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorFromBlacklist { index: 0 }.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(!config.blacklist.get(0).unwrap());

    drop(fixture);
}

#[tokio::test]
async fn test_set_new_authority() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;

    // Regular test
    let new_authority = Keypair::new();
    fixture
        .ctx
        .borrow_mut()
        .set_account(&new_authority.pubkey(), &system_account(1_000_000).into());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: new_authority.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.authority == new_authority.pubkey());

    // Try to transfer back with original authority
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "Unauthorized")
        .await;

    drop(fixture);
}
