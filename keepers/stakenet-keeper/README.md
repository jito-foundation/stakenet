# StakeNet Keeper Bot

## Overview

This is a service that permissionlessly maintains the StakeNet Validator History and Steward programs. It ensures that Validator History data is kept up to date, and that the Steward program is properly moving through its regular operations to maintain the targeted stake pool. Operations, program addresses, and transaction submission details, can be configured via arguments or environment variables.

## Usage

See [keeper-bot-quick-start.md](../../keeper-bot-quick-start.md) for instructions on how to build and run this service.

## Program Layout

### Keeper State

The `KeeperState` keeps track of:

- current epoch data
- running tally of all operations successes and failures for the given epoch
- all accounts fetched from the RPC that are needed downstream

Note: All maps are key'd by the `vote_account`

### Initialize

Gather all needed arguments, and initialize the global `KeeperState`.

### Loop

The forever loop consists of three parts: **Fetch**, **Fire** and **Emit**. There is only ever one **Fetch** and **Emit** section, and there can be several **Fire** sections.

The **Fire** sections can run on independent cadences - say we want the Validator History functions to run every 300sec and we want to emit metrics every 60sec.

The **Fetch** section is run _before_ and **Fire** section.
The **Emit** section is _one tick_ after any **Fire** section.

#### Fetch

The **Fetch** section is in charge of three operations:

- Keeping track of the current epoch and resetting the runs and error counts for each operation
- Creating any missing accounts needed for the downstream **Fires** to run
- Fetching and updating all of the needed accounts needed downstream

This is accomplished is by running three functions within the **Fetch** section

- `pre_create_update` - Updates epoch, and fetches all needed accounts that are not dependant on any missing accounts.
- `create_missing_accounts` - Creates the missing accounts, which can be determined by the accounts fetched in the previous step
- `post_create_update` - Fetches any last accounts that needed the missing accounts

Since this is run before every **FIRE** section, some accounts will be fetched that are not needed. This may seem wasteful but the simplicity of having a synchronized global is worth the cost.

Notes:

- The **Fetch** section is the only section that will mutate the `KeeperState`.
- If anything in the **Fetch** section fails, no **Fires** will run

#### Fire

There are several **Fire** sections running at their own cadence. Before any **Fire** section is run, the **Fetch** section will be called.

Each **Fire** is a call to `operations::operation_name::fire` which will fire off the operation and return the new count of runs and errors for that operation to be saved in the `KeeperState`

Notes:

- Each **Fire** is self contained, one should not be dependant on another.
- No \*_Fire_ will fetch any accounts, if there are needs for them, they should be added to the `KeeperState`

#### Emit

This section emits the state of the Keeper one tick after any operation has been called. This is because we want to emit a failure of any **Fetch** operation, which on failure advances the tick.
