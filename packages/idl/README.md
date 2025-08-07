# @jito-foundation/stakenet-idl

IDL files for Jito Stakenet Solana programs - Steward and Validator History

## Installation

```bash
npm install @jito-foundation/stakenet-idl
```

```bash
yarn add @jito-foundation/stakenet-idl
```

```bash
pnpm add @jito-foundation/stakenet-idl
```


## Usage

Import the IDL files you need:

```typescript
import { stewardIdl } from '@jito-foundation/stakenet-idl';
import { validatorHistoryIdl } from '@jito-foundation/stakenet-idl';

// Or import both at once
import { stewardIdl, validatorHistoryIdl } from '@jito-foundation/stakenet-idl';
```

### With Anchor

```typescript
import { Program } from '@coral-xyz/anchor';
import { stewardIdl, validatorHistoryIdl } from '@jito-foundation/stakenet-idl';

// Create program instances
const stewardProgram = new Program(stewardIdl, provider);
const historyProgram = new Program(validatorHistoryIdl, provider);
```


## Available IDL Files

### `stewardIdl`
Interface Definition Language for the Steward program, which handles validator set management and MEV rewards distribution.

### `validatorHistoryIdl` 
Interface Definition Language for the Validator History program, which tracks validator performance metrics and historical data.


## Repository

This package is part of the [Jito Stakenet](https://github.com/jito-foundation/stakenet) project.


## License

Apache-2.0
