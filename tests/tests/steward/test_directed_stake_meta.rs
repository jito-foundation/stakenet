use jito_steward::{
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
    DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_VALIDATORS,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_get_target_index() {
    let epoch = 1;
    let mut meta = DirectedStakeMeta {
        total_stake_targets: 0,
        padding0: [0; 64],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validator1 = Pubkey::new_from_array([0u8; 32]);
    let validator2 = Pubkey::new_from_array([1u8; 32]);
    let validator3 = Pubkey::new_from_array([2u8; 32]);

    // Test copying targets one by one
    let target1 = DirectedStakeTarget {
        vote_pubkey: validator1,
        total_target_lamports: 1000,
        total_staked_lamports: 0,
        target_last_updated_epoch: 0,
        staked_last_updated_epoch: 0,
        _padding0: [0; 64],
    };

    meta.targets[0] = target1;

    let target2 = DirectedStakeTarget {
        vote_pubkey: validator2,
        total_target_lamports: 2000,
        total_staked_lamports: 0,
        target_last_updated_epoch: 0,
        staked_last_updated_epoch: 0,
        _padding0: [0; 64],
    };

    meta.targets[1] = target2;

    let target3 = DirectedStakeTarget {
        vote_pubkey: validator3,
        total_target_lamports: 3000,
        total_staked_lamports: 0,
        target_last_updated_epoch: 0,
        staked_last_updated_epoch: 0,
        _padding0: [0; 64],
    };

    meta.targets[2] = target3;

    // Test getting the target index
    assert_eq!(meta.get_target_index(&validator1), Some(0));
    assert_eq!(meta.get_target_index(&validator2), Some(1));
    assert_eq!(meta.get_target_index(&validator3), Some(2));

    // Test getting the target lamports
    assert_eq!(meta.get_target_lamports(&validator1), Some(1000));
    assert_eq!(meta.get_target_lamports(&validator2), Some(2000));
    assert_eq!(meta.get_target_lamports(&validator3), Some(3000));

    // Test getting the total staked lamports
    assert_eq!(meta.get_total_staked_lamports(&validator1), Some(0));
    assert_eq!(meta.get_total_staked_lamports(&validator2), Some(0));
    assert_eq!(meta.get_total_staked_lamports(&validator3), Some(0));

    // Test adding to the total staked lamports
    meta.add_to_total_staked_lamports(&validator1, 100, epoch);
    meta.add_to_total_staked_lamports(&validator2, 200, epoch);
    meta.add_to_total_staked_lamports(&validator3, 300, epoch);

    // Test getting the total staked lamports
    assert_eq!(meta.get_total_staked_lamports(&validator1), Some(100));
    assert_eq!(meta.get_total_staked_lamports(&validator2), Some(200));
    assert_eq!(meta.get_total_staked_lamports(&validator3), Some(300));

    // Test subtracting from the total staked lamports
    meta.subtract_from_total_staked_lamports(&validator1, 50, epoch);
    meta.subtract_from_total_staked_lamports(&validator2, 100, epoch);
    meta.subtract_from_total_staked_lamports(&validator3, 150, epoch);

    // Test getting the total staked lamports
    assert_eq!(meta.get_total_staked_lamports(&validator1), Some(50));
    assert_eq!(meta.get_total_staked_lamports(&validator2), Some(100));
    assert_eq!(meta.get_total_staked_lamports(&validator3), Some(150));

    // Test all targets rebalanced for epoch
    assert!(meta.all_targets_rebalanced_for_epoch(epoch));
}
