use anchor_lang::prelude::*;

#[error_code]
pub enum StewardError {
    #[msg("Invalid set authority type: 0: SetAdmin, 1: SetBlacklistAuthority, 2: SetParametersAuthority")]
    InvalidAuthorityType,
    #[msg("Scoring must be completed before any other steps can be taken")]
    ScoringNotComplete,
    #[msg("Validator does not exist at the ValidatorList index provided")]
    ValidatorNotInList,
    #[msg("Add validators step must be completed before any other steps can be taken")]
    AddValidatorsNotComplete,
    #[msg("Cannot reset state before epoch is over")]
    EpochNotOver,
    #[msg("Unauthorized to perform this action")]
    Unauthorized,
    #[msg("Bitmask index out of bounds")]
    BitmaskOutOfBounds,
    #[msg("Epoch state not reset")]
    StateNotReset,
    #[msg("Validator History created after epoch start, out of range")]
    ValidatorOutOfRange,
    // Use invalid_state_error method to ensure expected and actual are logged
    InvalidState,
    #[msg("Validator not eligible to be added to the pool. Must meet stake minimum")]
    ValidatorBelowStakeMinimum,
    #[msg("Validator not eligible to be added to the pool. Must meet recent voting minimum")]
    ValidatorBelowLivenessMinimum,
    #[msg("Validator History vote data not recent enough to be used for scoring. Must be updated this epoch")]
    VoteHistoryNotRecentEnough,
    #[msg("Validator History stake data not recent enough to be used for scoring. Must be updated this epoch")]
    StakeHistoryNotRecentEnough,
    #[msg(
        "Cluster History data not recent enough to be used for scoring. Must be updated this epoch"
    )]
    ClusterHistoryNotRecentEnough,
    #[msg("Steward State Machine is paused. No state machine actions can be taken")]
    StateMachinePaused,
    #[msg("Config parameter is out of range or otherwise invalid")]
    InvalidParameterValue,
    #[msg("Instant unstake cannot be performed yet.")]
    InstantUnstakeNotReady,
    #[msg("Validator index out of bounds of state machine")]
    ValidatorIndexOutOfBounds,
    #[msg("ValidatorList account type mismatch")]
    ValidatorListTypeMismatch,
    #[msg("An operation caused an overflow/underflow")]
    ArithmeticError,
    #[msg("Validator not eligible for removal. Must be delinquent or have closed vote account")]
    ValidatorNotRemovable,
    #[msg("Validator was marked active when it should be deactivating")]
    ValidatorMarkedActive,
    #[msg("Max validators reached")]
    MaxValidatorsReached,
    #[msg("Validator history account does not match vote account")]
    ValidatorHistoryMismatch,
    #[msg("Epoch Maintenance must be called before continuing")]
    EpochMaintenanceNotComplete,
    #[msg("The stake pool must be updated before continuing")]
    StakePoolNotUpdated,
    #[msg("Epoch Maintenance has already been completed")]
    EpochMaintenanceAlreadyComplete,
    #[msg("Validators are marked for immediate removal")]
    ValidatorsNeedToBeRemoved,
    #[msg("No validators are marked for immediate removal")]
    NoValidatorsNeedToBeRemoved,
    #[msg("Validator not marked for removal")]
    ValidatorNotMarkedForRemoval,
    #[msg("Validators have not been removed")]
    ValidatorsHaveNotBeenRemoved,
    #[msg("Validator List count does not match state machine")]
    ListStateMismatch,
}
