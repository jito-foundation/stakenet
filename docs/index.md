---
layout: default
title: Steward Program Documentation
---

_Note: documentation for the Validator History program is a work in progress. Please see the top level [README](https://github.com/jito-foundation/stakenet/blob/master/README.md) for more information._

# Steward Program

The Steward Program is an Anchor program designed to manage the staking authority for a SPL Stake Pool. Using on-chain [validator history](https://github.com/jito-foundation/stakenet) the steward selects a set of high-performing validators to delegate to, maintains the desired level of stake on those validators over time, and continuously monitors and re-evaluates the validator set at a set cadence. Initially, the validator selection is customized for the JitoSOL stake pool criteria and will be deployed to manage that stake pool. Additionally, the steward surfaces this staking algorithm through variable parameters to be decided by [Jito DAO](https://gov.jito.network/dao/Jito). In turn, this greatly decentralizes the stake pool operations.

The core operations of the Steward Program are permissionless such that any cranker can operate the system. However there are some [admin/management functions](#admin-abilities) that allow for tweaking parameters and system maintenance.

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
