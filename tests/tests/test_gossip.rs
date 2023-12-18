use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use bincode::serialize;
use rand::thread_rng;
use solana_gossip::{
    contact_info::ContactInfo,
    crds_value::{CrdsData, NodeInstance, Version},
    legacy_contact_info::LegacyContactInfo,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, ed25519_instruction::new_ed25519_instruction, signer::Signer,
    transaction::Transaction,
};
use solana_version::LegacyVersion2;
use tests::fixtures::TestFixture;
use validator_history::{
    crds_value::{CrdsData as ValidatorHistoryCrdsData, LegacyVersion, LegacyVersion1},
    ValidatorHistory,
};

fn create_gossip_tx(fixture: &TestFixture, crds_data: &CrdsData) -> Transaction {
    let ctx = &fixture.ctx;
    let dalek_keypair =
        ed25519_dalek::Keypair::from_bytes(&fixture.identity_keypair.to_bytes()).unwrap();

    // create ed25519 instruction
    let ed25519_ix = new_ed25519_instruction(&dalek_keypair, &serialize(crds_data).unwrap());

    // create CopyGossipContactInfo instruction
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyGossipContactInfo {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            instructions: anchor_lang::solana_program::sysvar::instructions::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyGossipContactInfo {}.data(),
    };
    Transaction::new_signed_with_payer(
        &[ed25519_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    )
}

#[tokio::test]
async fn test_copy_legacy_contact_info() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // create legacycontactinfo as signed crdsdata struct
    let mut legacy_contact_info = LegacyContactInfo::new_rand(
        &mut rand::thread_rng(),
        Some(fixture.identity_keypair.pubkey()),
    );
    legacy_contact_info.set_wallclock(0);
    let crds_data = CrdsData::LegacyContactInfo(legacy_contact_info.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture.submit_transaction_assert_success(transaction).await;
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    let ip = if let IpAddr::V4(ipv4) = legacy_contact_info.gossip().unwrap().ip() {
        ipv4.octets()
    } else {
        panic!("IPV6 not supported")
    };
    assert!(account.history.arr[0].ip == ip);
    assert!(account.history.arr[0].epoch == 0);
}

#[tokio::test]
async fn test_copy_contact_info() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let wallclock = 0;
    let mut contact_info = ContactInfo::new(fixture.identity_keypair.pubkey(), wallclock, 0);
    let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
    let ip = IpAddr::V4(ipv4);
    contact_info
        .set_socket(0, SocketAddr::new(ip, 1234))
        .expect("could not set socket");

    let crds_data = CrdsData::ContactInfo(contact_info.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    let version = solana_version::Version::default();
    assert!(account.history.arr[0].version.major == version.major as u8);
    assert!(account.history.arr[0].version.minor == version.minor as u8);
    assert!(account.history.arr[0].version.patch == version.patch);
    assert!(account.history.arr[0].ip == ipv4.octets());
    assert!(account.history.arr[0].epoch == 0);
}

#[tokio::test]
async fn test_copy_legacy_version() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // can't import LegacyVersion from gossip cause inner fields are private
    let version = LegacyVersion {
        from: fixture.identity_keypair.pubkey(),
        wallclock: 0,
        version: LegacyVersion1 {
            major: 1,
            minor: 2,
            patch: 3,
            commit: None,
        },
    };

    let crds_data = ValidatorHistoryCrdsData::LegacyVersion(version.clone());
    let dalek_keypair =
        ed25519_dalek::Keypair::from_bytes(&fixture.identity_keypair.to_bytes()).unwrap();

    // create ed25519 instruction
    let ed25519_ix = new_ed25519_instruction(&dalek_keypair, &serialize(&crds_data).unwrap());

    // create CopyGossipContactInfo instruction
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyGossipContactInfo {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            instructions: anchor_lang::solana_program::sysvar::instructions::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyGossipContactInfo {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[ed25519_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.arr[0].version.major == version.version.major as u8);
    assert!(account.history.arr[0].version.minor == version.version.minor as u8);
    assert!(account.history.arr[0].version.patch == version.version.patch);
    assert!(account.history.arr[0].epoch == 0);
}

#[tokio::test]
async fn test_copy_version() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let version = Version {
        from: fixture.identity_keypair.pubkey(),
        wallclock: 0,
        version: LegacyVersion2 {
            major: 1,
            minor: 2,
            patch: 3,
            commit: None,
            feature_set: 0,
        },
    };
    let crds_data = CrdsData::Version(version.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.arr[0].version.major == version.version.major as u8);
    assert!(account.history.arr[0].version.minor == version.version.minor as u8);
    assert!(account.history.arr[0].version.patch == version.version.patch);
    assert!(account.history.arr[0].epoch == 0);
}

#[tokio::test]
async fn test_gossip_wrong_signer() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let version = Version {
        from: fixture.identity_keypair.pubkey(),
        wallclock: 0,
        version: LegacyVersion2 {
            major: 1,
            minor: 2,
            patch: 3,
            commit: None,
            feature_set: 0,
        },
    };
    let crds_data = CrdsData::Version(version.clone());

    // cranker keypair instead of node identity keypair
    let dalek_keypair = ed25519_dalek::Keypair::from_bytes(&fixture.keypair.to_bytes()).unwrap();

    let ed25519_ix = new_ed25519_instruction(&dalek_keypair, &serialize(&crds_data).unwrap());

    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyGossipContactInfo {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            instructions: anchor_lang::solana_program::sysvar::instructions::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyGossipContactInfo {}.data(),
    };

    let transaction = Transaction::new_signed_with_payer(
        &[ed25519_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(transaction, "GossipDataInvalid")
        .await;
}

#[tokio::test]
async fn test_gossip_wrong_node_pubkey() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // vote account instead of identity account
    let version = Version {
        from: fixture.vote_account,
        wallclock: 0,
        version: LegacyVersion2 {
            major: 1,
            minor: 2,
            patch: 3,
            commit: None,
            feature_set: 0,
        },
    };
    let crds_data = CrdsData::Version(version.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture
        .submit_transaction_assert_error(transaction, "GossipDataInvalid")
        .await;
}

#[tokio::test]
async fn test_gossip_missing_sigverify_instruction() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyGossipContactInfo {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            instructions: anchor_lang::solana_program::sysvar::instructions::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyGossipContactInfo {}.data(),
    };

    let dummy_ix = anchor_lang::solana_program::system_instruction::transfer(
        &fixture.keypair.pubkey(),
        &fixture.vote_account,
        1,
    );

    let transaction = Transaction::new_signed_with_payer(
        &[dummy_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "NotSigVerified")
        .await;
}

#[tokio::test]
async fn test_gossip_wrong_message() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // Not a crdsdata that we're expecting
    let node_instance = NodeInstance::new(&mut thread_rng(), fixture.identity_keypair.pubkey(), 0);
    let crds_data = CrdsData::NodeInstance(node_instance);

    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture
        .submit_transaction_assert_error(transaction, "GossipDataInvalid")
        .await;
}

#[tokio::test]
async fn test_gossip_timestamps() {
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;
    let mut banks_client = {
        let ctx = fixture.ctx.borrow_mut();
        ctx.banks_client.clone()
    };

    let clock: Clock = banks_client.get_sysvar().await.unwrap();
    let wallclock = clock.unix_timestamp as u64;
    let mut contact_info = ContactInfo::new(fixture.identity_keypair.pubkey(), wallclock, 0);
    let ipv4 = Ipv4Addr::new(1, 2, 3, 4);
    let ip = IpAddr::V4(ipv4);
    contact_info
        .set_socket(0, SocketAddr::new(ip, 1234))
        .expect("could not set socket");
    let crds_data = CrdsData::ContactInfo(contact_info.clone());

    let transaction = create_gossip_tx(&fixture, &crds_data);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.last_ip_timestamp == wallclock);
    assert!(account.last_version_timestamp == wallclock);

    contact_info.set_wallclock(wallclock + 1);

    let crds_data = CrdsData::ContactInfo(contact_info.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.last_ip_timestamp == wallclock + 1);
    assert!(account.last_version_timestamp == wallclock + 1);

    // LegacyContactInfo with old wallclock
    let mut legacy_contact_info = LegacyContactInfo::new_rand(
        &mut rand::thread_rng(),
        Some(fixture.identity_keypair.pubkey()),
    );
    legacy_contact_info.set_wallclock(wallclock);

    let crds_data = CrdsData::LegacyContactInfo(legacy_contact_info);
    let transaction = create_gossip_tx(&fixture, &crds_data);
    fixture
        .submit_transaction_assert_error(transaction, "GossipDataTooOld")
        .await;

    // LegacyVersion with old wallclock
    let version = Version {
        from: fixture.identity_keypair.pubkey(),
        wallclock,
        version: LegacyVersion2 {
            major: 1,
            minor: 2,
            patch: 3,
            commit: None,
            feature_set: 0,
        },
    };
    let crds_data = CrdsData::Version(version);
    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture
        .submit_transaction_assert_error(transaction, "GossipDataTooOld")
        .await;

    // ContactInfo with 11 minutes in the future wallclock - will fail
    contact_info.set_wallclock(wallclock + 11 * 60);
    let crds_data = CrdsData::ContactInfo(contact_info.clone());
    let transaction = create_gossip_tx(&fixture, &crds_data);

    fixture
        .submit_transaction_assert_error(transaction, "GossipDataInFuture")
        .await;
}
