---
layout: default
title: Steward Program Documentation
---

_Note: documentation for the Validator History program is a work in progress. Please see the top level [README](https://github.com/jito-foundation/stakenet/blob/master/README.md) for more information._

# Steward Program

The Steward Program is an Anchor program designed to manage the staking authority for a SPL Stake Pool. Using on-chain [validator history](https://github.com/jito-foundation/stakenet) the steward selects a set of high-performing validators to delegate to, maintains the desired level of stake on those validators over time, and continuously monitors and re-evaluates the validator set at a set cadence. Initially, the validator selection is customized for the JitoSOL stake pool criteria and will be deployed to manage that stake pool. Additionally, the steward surfaces this staking algorithm through variable parameters to be decided by [Jito DAO](https://gov.jito.network/dao/Jito). In turn, this greatly decentralizes the stake pool operations.

The core operations of the Steward Program are permissionless such that any cranker can operate the system. However there are some [admin/management functions](#admin-abilities) that allow for tweaking parameters and system maintenance.

## Table of Contents

1. [Terminology](./terminology.md)
2. [Program Overview](program-overview.md)
3. [Parameters](parameters.md)
4. [Command-line interface](./cli.md)
5. [Events API](./api.md)
6. [StakeNet UI](./ui.md)
7. Advanced
   - [SPL Stake Pool Internals](developers/spl-stake-pool-internals.md)
   - [Validator States](developers/validator-states.md)
