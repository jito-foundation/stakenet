# Agent Guides

Reference documents written to be read by an AI agent rather than a person, so a validator operator
can answer questions about StakeNet using their own tooling and their own judgement.

| File | Purpose |
| :- | :- |
| [`jitosol-stake-timing-reference.md`](./jitosol-stake-timing-reference.md) | How JitoSOL delegation timing works, which accounts hold the data, how to compute each quantity, and what an agent must not claim |
| [`jitosol-stake-timing-prompt.md`](./jitosol-stake-timing-prompt.md) | A copy-paste prompt for "when will my validator get stake?", with a worked example of a good answer |

## Why a prompt instead of a command

"When will I get stake?" splits cleanly in two, and the split is the whole reason these are
documents rather than a subcommand.

The **present-tense** half is deterministic and reads straight off the chain: whether a validator is
eligible, which filter it fails and the epoch that failure expires, the per-validator target, rank,
how much SOL is queued ahead, how much rebalancing budget is left in the cycle, and the epoch it
resets.

The **future-tense** half is not derivable. A stake pool is dynamic — deposit and withdrawal flow,
other validators' commission and performance decisions, and the re-scoring that happens at every
cycle boundary all move delegations, and none of them are predictable. Reality 10 or 20 epochs out
can differ substantially from any projection.

A first-party command printing an epoch count would be read as a commitment no matter how it was
labelled. Publishing the model instead means the operator runs it against live data, sees the inputs
and the arithmetic, and owns the estimate. The reference is explicit that an agent following it must
refuse to give a date and must explain why.

## Usage

1. Give your agent both files, by path or by URL.
2. Fill in the prompt's three placeholders: your vote account, an RPC endpoint, and the path or URL
   where the agent can read the reference. A private RPC is strongly preferred; these reads touch a
   lot of accounts and the public endpoint rate-limits.
3. Run it. Re-run it in a later epoch — the firm facts will have moved slightly, and that movement is
   the point rather than a defect.

The preferred data path is the released `steward-cli` in this repository (`view-config`,
`view-state`, `view-directed-stake-meta`). A raw-RPC fallback with account layouts is included for
agents without a Rust toolchain, along with a mandatory self-check the agent must pass before
reporting any decoded figure — see "MANDATORY self-check before reporting anything" in the
reference.

## Two traps these documents exist to prevent

**Dividing gross TVL by the set size.** Targets divide the pool *net of directed stake*. Using gross
TVL overstates every target by `directed_total / N` and invents a pool-wide shortfall equal to the
entire directed balance. The reconciliation invariant in the reference's self-check section
catches it.

**Calling a directed-stake holder underfunded.** Progress toward target is measured on undirected
balance only, so a validator holding directed stake can show a full-target shortfall while holding
several times the target in total. Undirected and directed balances must be reported separately.

## Maintenance

Parameters are DAO-set and change; the on-chain Steward config is always authoritative, and the
reference tells the agent to read it rather than trust any value quoted in the document. The account
layouts in the fallback appendix are **not** a stable interface — a program upgrade can move them, so
if the Steward program is redeployed, re-verify that appendix or remove it in favour of the CLI path.
