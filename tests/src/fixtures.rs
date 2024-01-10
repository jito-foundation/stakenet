#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{
    solana_program::{
        clock::Clock,
        pubkey::Pubkey,
        vote::state::{VoteInit, VoteState, VoteStateVersions},
    },
    AccountSerialize, InstructionData, ToAccountMetas,
};
use solana_program_test::*;
use solana_sdk::{
    account::Account, epoch_schedule::EpochSchedule, instruction::Instruction, signature::Keypair,
    signer::Signer, transaction::Transaction,
};
use std::{cell::RefCell, rc::Rc};

use jito_tip_distribution::{
    sdk::derive_tip_distribution_account_address, state::TipDistributionAccount,
};
use validator_history::{self, constants::MAX_ALLOC_BYTES, ValidatorHistory};

pub struct TestFixture {
    pub ctx: Rc<RefCell<ProgramTestContext>>,
    pub vote_account: Pubkey,
    pub identity_keypair: Keypair,
    pub validator_history_account: Pubkey,
    pub validator_history_config: Pubkey,
    pub tip_distribution_account: Pubkey,
    pub keypair: Keypair,
}

impl TestFixture {
    pub async fn new() -> Self {
        /*
           Initializes test context with ValidatorHistory and TipDistribution programs loaded, as well as
           a vote account and a system account for signing transactions.

           Returns a fixture with relevant account addresses and keypairs.
        */
        let mut program = ProgramTest::new(
            "validator-history",
            validator_history::ID,
            processor!(validator_history::entry),
        );
        program.add_program(
            "jito-tip-distribution",
            jito_tip_distribution::id(),
            processor!(jito_tip_distribution::entry),
        );

        let epoch = 0;
        let vote_account = Pubkey::new_unique();
        let identity_keypair = Keypair::new();
        let identity_pubkey = identity_keypair.pubkey();
        let tip_distribution_account = derive_tip_distribution_account_address(
            &jito_tip_distribution::id(),
            &vote_account,
            epoch,
        )
        .0;
        let validator_history_config = Pubkey::find_program_address(
            &[validator_history::state::Config::SEED],
            &validator_history::id(),
        )
        .0;
        let validator_history_account = Pubkey::find_program_address(
            &[
                validator_history::state::ValidatorHistory::SEED,
                vote_account.as_ref(),
            ],
            &validator_history::id(),
        )
        .0;
        let keypair = Keypair::new();

        program.add_account(
            vote_account,
            new_vote_account(identity_pubkey, vote_account, 1, Some(vec![(0, 0, 0); 10])),
        );
        program.add_account(keypair.pubkey(), system_account(100_000_000_000));
        program.add_account(identity_pubkey, system_account(100_000_000_000));

        let ctx = Rc::new(RefCell::new(program.start_with_context().await));

        Self {
            ctx,
            validator_history_config,
            validator_history_account,
            identity_keypair,
            vote_account,
            tip_distribution_account,
            keypair,
        }
    }

    pub async fn load_and_deserialize<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
    ) -> T {
        let ai = self
            .ctx
            .borrow_mut()
            .banks_client
            .get_account(*address)
            .await
            .unwrap()
            .unwrap();

        T::try_deserialize(&mut ai.data.as_slice()).unwrap()
    }

    pub async fn initialize_config(&self) {
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::InitializeConfig {
                config: self.validator_history_config,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeConfig {
                authority: self.keypair.pubkey(),
            }
            .data(),
        };
        let set_tip_distribution_instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::SetNewTipDistributionProgram {
                config: self.validator_history_config,
                new_tip_distribution_program: jito_tip_distribution::id(),
                admin: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::SetNewTipDistributionProgram {}.data(),
        };
        let transaction = Transaction::new_signed_with_payer(
            &[instruction, set_tip_distribution_instruction],
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );
        if let Err(e) = self
            .ctx
            .borrow_mut()
            .banks_client
            .process_transaction_with_preflight(transaction)
            .await
        {
            panic!("Error: {}", e);
        }
    }

    pub async fn initialize_validator_history_account(&self) {
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
        };

        let mut ixs = vec![instruction];

        // Realloc validator history account
        let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        ixs.extend(vec![
            Instruction {
                program_id: validator_history::id(),
                accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                    validator_history_account: self.validator_history_account,
                    vote_account: self.vote_account,
                    config: self.validator_history_config,
                    system_program: anchor_lang::solana_program::system_program::id(),
                    signer: self.keypair.pubkey(),
                }
                .to_account_metas(None),
                data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
            };
            num_reallocs
        ]);
        let transaction = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );
        self.submit_transaction_assert_success(transaction).await;
    }

    pub async fn advance_num_epochs(&self, num_epochs: u64) {
        let clock: Clock = self
            .ctx
            .borrow_mut()
            .banks_client
            .get_sysvar()
            .await
            .expect("Failed getting clock");
        let epoch_schedule: EpochSchedule = self.ctx.borrow().genesis_config().epoch_schedule;
        let target_epoch = clock.epoch + num_epochs;
        let target_slot = epoch_schedule.get_first_slot_in_epoch(target_epoch);

        self.ctx
            .borrow_mut()
            .warp_to_slot(target_slot)
            .expect("Failed warping to future epoch");
    }

    pub async fn submit_transaction_assert_success(&self, transaction: Transaction) {
        let mut ctx = self.ctx.borrow_mut();
        if let Err(e) = ctx
            .banks_client
            .process_transaction_with_preflight(transaction)
            .await
        {
            panic!("Error: {}", e);
        }
    }

    pub async fn submit_transaction_assert_error(
        &self,
        transaction: Transaction,
        error_message: &str,
    ) {
        if let Err(e) = self
            .ctx
            .borrow_mut()
            .banks_client
            .process_transaction_with_preflight(transaction)
            .await
        {
            assert!(e.to_string().contains(error_message));
        } else {
            panic!("Error: Transaction succeeded. Expected {}", error_message);
        }
    }
}

pub fn system_account(lamports: u64) -> Account {
    Account {
        lamports,
        owner: anchor_lang::system_program::ID,
        executable: false,
        rent_epoch: 0,
        data: vec![],
    }
}

pub fn new_vote_account(
    node_pubkey: Pubkey,
    vote_pubkey: Pubkey,
    commission: u8,
    maybe_epoch_credits: Option<Vec<(u64, u64, u64)>>,
) -> Account {
    let vote_init = VoteInit {
        node_pubkey,
        authorized_voter: vote_pubkey,
        authorized_withdrawer: vote_pubkey,
        commission,
    };
    let clock = Clock {
        epoch: 0,
        slot: 0,
        unix_timestamp: 0,
        leader_schedule_epoch: 0,
        epoch_start_timestamp: 0,
    };
    let mut vote_state = VoteState::new(&vote_init, &clock);
    if let Some(epoch_credits) = maybe_epoch_credits {
        vote_state.epoch_credits = epoch_credits;
    }
    let vote_state_versions = VoteStateVersions::new_current(vote_state);
    let mut data = vec![0; VoteState::size_of()];
    VoteState::serialize(&vote_state_versions, &mut data).unwrap();

    Account {
        lamports: 1000000,
        data,
        owner: anchor_lang::solana_program::vote::program::ID,
        ..Account::default()
    }
}

pub fn new_tip_distribution_account(vote_account: Pubkey, mev_commission_bps: u16) -> Account {
    let tda = TipDistributionAccount {
        validator_vote_account: vote_account,
        validator_commission_bps: mev_commission_bps,
        ..TipDistributionAccount::default()
    };
    let mut data = vec![];
    tda.try_serialize(&mut data).unwrap();
    Account {
        lamports: 1000000,
        data,
        owner: jito_tip_distribution::id(),
        ..Account::default()
    }
}
