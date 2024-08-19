---
layout: default
title: Steward Program Documentation
---

# Steward Program

The Steward Program is an Anchor program designed to manage the staking authority for a SPL Stake Pool. Using on-chain validator history, the steward selects a set of high-performing validators to delegate to, maintains the desired level of stake on those validators over time, and continuously monitors and re-evaluates the validator set at a set cadence.

## Purpose

The Steward Program was created to automatically manage the Jito Stake Pool. Using on-chain validator history data, the steward chooses who to stake to and how much by way of its staking algorithm. Additionally, the steward surfaces this staking algorithm through variable parameters to be decided by Jito DAO. In turn, this greatly decentralizes the stake pool operations.

## Table of Contents

1. [Program Overview](program-overview.md)
2. [State Machine](state-machine.md)
3. [Validator Management](validator-management.md)
4. [Admin Abilities](admin-abilities.md)
5. [Parameters](parameters.md)

6. Validators

   - [Scoring System](validators/scoring-system.md)
   - [Instant Unstaking](validators/instant-unstaking.md)
   - [Eligibility Criteria](validators/eligibility-criteria.md)

7. Developers

   - [SPL Stake Pool Internals](developers/spl-stake-pool-internals.md)
   - [Validator States](developers/validator-states.md)
   - [Program Architecture](developers/program-architecture.md)
   - [Integration Guide](developers/integration-guide.md)

8. [Appendix](appendix.md)

For validators interested in understanding how they are scored and managed within the Steward Program, please refer to the Validators section.

For developers interested in the technical details of the Steward Program, please refer to the Developers section.
