# @stakenet/idl

IDL files for Jito Stakenet Solana programs - Steward and Validator History

## Installation

```bash
npm install @stakenet/idl
```

```bash
yarn add @stakenet/idl
```

```bash
pnpm add @stakenet/idl
```

## Usage

Import the IDL files you need:

```typescript
import { stewardIdl } from '@stakenet/idl';
import { historyIdl } from '@stakenet/idl';

// Or import both at once
import { stewardIdl, historyIdl } from '@stakenet/idl';
```

### With Anchor

```typescript
import { Program } from '@coral-xyz/anchor';
import { stewardIdl, historyIdl } from '@stakenet/idl';

// Create program instances
const stewardProgram = new Program(stewardIdl, provider);
const historyProgram = new Program(historyIdl, provider);
```

## Available IDL Files

### `stewardIdl`
Interface Definition Language for the Steward program, which handles validator set management and MEV rewards distribution.

### `validatorHistoryIdl` 
Interface Definition Language for the Validator History program, which tracks validator performance metrics and historical data.

## TypeScript Support

This package includes TypeScript definitions for all IDL files, providing full type safety when working with Stakenet programs.

```typescript
import type { StewardIdl, HistoryIdl } from '@stakenet/idl';

// IDL objects are fully typed
const steward: StewardIdl = stewardIdl;
const history: HistoryIdl = historyIdl;
```

## Repository

This package is part of the [Jito Stakenet](https://github.com/jito-foundation/stakenet) project.

## License

Apache-2.0
