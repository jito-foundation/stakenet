use anchor_lang::prelude::*;

#[error_code]
pub enum StewardError {
    #[msg("Invalid set authority type: 0: SetAdmin, 1: SetBlacklistAuthority, 2: SetParametersAuthority")]
    InvalidAuthorityType,
    #[msg("Scoring must be completed before any other steps can be taken")]
    ScoringNotComplete,
    #[msg("Validator does not exist at the ValidatorList index provided")]
    ValidatorNotInList,
    #[msg("Unauthorized to perform this action")]
    Unauthorized,
    #[msg("Bitmask index out of bounds")]
    BitmaskOutOfBounds,
    #[msg("Invalid state")]
    InvalidState,
    #[msg("Stake state is not Stake")]
    StakeStateIsNotStake,
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
    #[msg("Epoch Maintenance must be called before continuing")]
    EpochMaintenanceNotComplete,
    #[msg("The stake pool must be updated before continuing")]
    StakePoolNotUpdated,
    #[msg("Epoch Maintenance has already been completed")]
    EpochMaintenanceAlreadyComplete,
    #[msg("Validators are marked for immediate removal")]
    ValidatorsNeedToBeRemoved,
    #[msg("Validator not marked for removal")]
    ValidatorNotMarkedForRemoval,
    #[msg("Validators have not been removed")]
    ValidatorsHaveNotBeenRemoved,
    #[msg("Validator List count does not match state machine")]
    ListStateMismatch,
    #[msg("Vote account does not match")]
    VoteAccountDoesNotMatch,
    #[msg("Validator needs to be marked for removal")]
    ValidatorNeedsToBeMarkedForRemoval,
    #[msg("Invalid stake state")]
    InvalidStakeState,
    #[msg("Arithmetic casting error")]
    ArithmeticCastError,

    #[msg("Directed stake validator whitelist is full")]
    DirectedStakeValidatorListFull,
    #[msg("Directed stake stakers list is full")]
    DirectedStakeStakerListFull,
    #[msg("Already permissioned")]
    AlreadyPermissioned,
}
