use jito_steward::StewardStateEnum;

pub fn state_tag_to_string(tag: StewardStateEnum) -> String {
    match tag {
        StewardStateEnum::ComputeScores => "Compute Scores".to_string(),
        StewardStateEnum::ComputeDelegations => "Compute Delegations".to_string(),
        StewardStateEnum::Idle => "Idle".to_string(),
        StewardStateEnum::ComputeInstantUnstake => "Compute Instant Unstake".to_string(),
        StewardStateEnum::Rebalance => "Rebalance".to_string(),
    }
}
