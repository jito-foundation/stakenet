---
layout: default
title: StakeNet Terminology
---

**Steward Program**: An Anchor program designed to manage the staking authority for a SPL Stake Pool, selecting and managing validators based on performance metrics.

**SPL Stake Pool**: A Solana program that allows for the creation of stake pools, enabling users to delegate their stake to multiple validators through a single pool and receive a Liquid Staking Token (LST) representing their stake.

**Validator History Program**:
A Solana program that maintains historical performance data for validators. It records and stores various metrics over time on-chain.

**Delegation**: The amount of stake targeted to a specific validator by the Steward Program.

**State Machine**: The core logic of the Steward Program that manages different operational states and transitions between them.

**Cycle**: A period of time (currently 10 epochs) in the Steward Program during which validators are selected and delegations are managed. The scores and validator selections are fixed for the duration of a cycle.

**Validator Score**: A numerical representation of a validator's performance and desirability within the Steward Program.

**Yield Score**: A component of the validator score that represents the validator's efficiency in generating rewards for delegators, taking into account factors like epoch credits and commission.

**Rebalancing**: The process of adjusting stake allocations among validators to maintain desired proportions or react to performance changes.

**Instant Unstaking**: A process that allows for immediate removal of staked SOL under certain conditions from a validator, bypassing usual rebalancing.

**Blacklist**: A list of validators that are excluded from receiving delegations through the Steward Program.

**Delinquency**: A state where a validator is not its duties adequately, often measured by missed vote opportunities. In the context of the Steward Program, this is measured by voting less than a specific threshold in a given epoch.

**Epoch Credits**: A measure of a validator's performance, representing the number of times it has correctly voted on blocks in a given epoch. Directly impacts validator staking rewards. Also known as Vote Credits.

**Commission**: The percentage of rewards that a validator keeps for itself before distributing the remainder to its delegators.

**Superminority**: The highest-staked validators in the network who collectively hold more than 33.3% of the stake.
