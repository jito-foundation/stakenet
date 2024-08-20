---
layout: default
title: Steward API Documentation
---

# Steward API Documentation

## Events Endpoint

This endpoint allows you to retrieve various events related to the Steward program. Events are emitted by the Steward program when certain actions occur, such as when a validator is scored, or when rebalancing takes place.

### Endpoint

`GET https://kobe.mainnet.jito.network/api/v1/steward_events`

### Parameters

| Parameter    | Type    | Required | Description                                         |
| ------------ | ------- | -------- | --------------------------------------------------- |
| vote_account | String  | No       | Filter events by validator vote account public key  |
| epoch        | Integer | No       | Filter events by epoch number                       |
| event_type   | String  | No       | Filter events by type (see Valid Event Types below) |
| page         | Integer | No       | Page number for pagination (default: 1)             |
| limit        | Integer | No       | Number of events per page (default: 100, max: 1000) |

### Valid Event Types

Most relevant to validators:

- `ScoreComponents`: Validator scoring details. Emitted once every 10 epochs
- `InstantUnstakeComponents`: Information about instant unstaking events
- `RebalanceEvent`: Details about stake rebalancing operations

Updates to overall state:

- `StateTransition`: Changes in the Steward program's state machine
- `DecreaseComponents`: Included in RebalanceEvent, provides specifics on stake decreases
- `AutoRemoveValidatorEvent`: Automatic removal of offline validators from the pool
- `AutoAddValidatorEvent`: Automatic addition of new validators to the pool
- `EpochMaintenanceEvent`: Epoch maintenance operations

### Example Request: Get a validator's scores

`GET https://kobe.mainnet.jito.network/api/v1/steward_events?vote_account=J1to3PQfXidUUhprQWgdKkQAMWPJAEqSJ7amkBDE9qhF&event_type=ScoreComponents`

```json
{
  "events": [
  {
    "signature": "5N3hVRpuqsiXCiChrm3GuaWRfi2zZMYAkx6jnM3YTocAC5RBTsrukk4ghFHeCyZawC7Ca72i7fo8TNg2MsG1zXP7",
    "event_type": "ScoreComponents",
    "vote_account": "J1to3PQfXidUUhprQWgdKkQAMWPJAEqSJ7amkBDE9qhF",
    "timestamp": "2024-08-20T06:18:46Z",
    "data": {
      "score": 0.9763466522227435,
      "yield_score": 0.9763466522227435,
      "mev_commission_score": 1.0,
      "blacklisted_score": 1.0,
      "superminority_score": 1.0,
      "delinquency_score": 1.0,
      "running_jito_score": 1.0,
      "commission_score": 1.0,
      "historical_commission_score": 1.0,
      "vote_credits_ratio": 0.9763466522227435
    },
    "epoch": 659
  },
  ...
  ]
}
```

### Example Request: See stake movements by epoch

`GET https://kobe.mainnet.jito.network/api/v1/steward_events?event_type=RebalanceEvent&epoch=657&limit=2000`

```json
{
  "events": [
    {
      "signature": "64GGjM2QtrKw4SPocR5hmw17Kenf9qBdRRM5KrrM9gkr8XUgyyjXNQkuzfxq3ZhDJgHU8jvhUKxaAfMnnGp85Uss",
      "event_type": "RebalanceEvent",
      "vote_account": "7emL18Bnve7wbYE9Az7vYJjikxN6YPU81igf6rVU5FN8",
      "timestamp": "2024-08-17T20:20:57Z",
      "data": {
        "rebalance_type_tag": "Increase",
        "increase_lamports": 2762842586176,
          "decrease_components": {
          "scoring_unstake_lamports": 0,
          "instant_unstake_lamports": 0,
          "stake_deposit_unstake_lamports": 0,
          "total_unstake_lamports": 0
        }
      },
      "epoch": 657
    },
    ...
  ]
}
```
