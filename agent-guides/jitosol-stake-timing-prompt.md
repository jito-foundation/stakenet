# Prompt: "When will my validator get JitoSOL stake?"

Copy the block below into Claude Code, Codex, Cursor, or any agent that can run shell commands and
read files. Replace the three placeholders first.

**You need:**

- `jitosol-stake-timing-reference.md` — either saved locally or reachable by URL. Whichever it is,
  put that path or URL in the `<PATH_OR_URL_TO_REFERENCE>` placeholder below; if your agent cannot
  fetch URLs, download the file first and give it a local path.
- Your validator's **vote account** address
- A Solana RPC endpoint (a private one is strongly preferred — the public endpoint rate-limits and
  these commands read a lot of accounts)

---

## The prompt

```
You are helping me understand where my Solana validator stands in the JitoSOL delegation
queue, and what is actually knowable about when stake might arrive.

My vote account: <YOUR_VOTE_ACCOUNT>
RPC endpoint:    <YOUR_RPC_URL>
Reference doc:   <PATH_OR_URL_TO_REFERENCE>

Read the reference doc above before doing anything else, and follow it. It describes how
JitoSOL delegation works, which accounts hold the data, how to compute each quantity, and —
most importantly — what you must not claim. If you cannot read it, stop and tell me rather
than working from memory: the details that matter here are easy to get subtly wrong.

Do this:

1. Fetch live data. Prefer the released steward-cli commands listed in the reference. If it
   is not installed, either build it from github.com/jito-foundation/stakenet or fall back to
   raw RPC using the layouts in the reference appendix. Do not use any figure I have given
   you or that appears as an example in the reference — read everything live, and tell me the
   epoch and epoch progress your data is from.

2. Work out my position:
   - Am I eligible? If not, which specific filter am I failing, by how much, and in which
     epoch does the offending data point leave the window? That expiry epoch is a real,
     derivable answer — give it to me.
   - Am I in the current delegation set? What is my rank, and out of how many?
   - What is the per-validator target, and what is my undirected balance against it?
   - How much SOL is queued ahead of me for funding?
   - How much rebalancing budget is left this cycle, and at which epoch does it reset?

3. Name the single binding constraint, using the decision table in the reference. This is the
   most useful thing you can tell me: the specific reason stake is or is not moving right now.

4. Tell me what I can change, in order of leverage, with the actual numbers for my validator
   rather than generic advice.

Hard rules — these matter more than being helpful:

- Do NOT give me a date, or an epoch by which I will reach target, or a countdown. It is not
  derivable. A stake pool is dynamic: deposit and withdrawal flow, other validators'
  commission and performance changes, and the re-scoring at each cycle boundary all move
  delegations and none of them are predictable.
- You MAY tell me what would arrive next epoch given the reserve exactly as it stands, framed
  as a conditional.
- Refuse explicitly to extrapolate past the next scoring epoch, and say why.
- Subtract directed stake from pool TVL before dividing by the set size. If you divide gross
  TVL you will overstate every target and invent a shortfall that does not exist.
- Report my undirected and directed balances separately. Never call a validator underfunded
  on the undirected figure alone if it holds directed stake.
- If a number looks implausible, say so rather than reporting it. Sanity-check against
  https://www.jito.network/stakenet/steward/
- Show the commands you ran and the raw values you used, so I can check your arithmetic.

Finish with the caveats from the reference's "Caveats to include in any answer" section, in
your own words. If I ask you to turn any of this into a date afterwards, decline and explain
what would have to be knowable for a date to mean anything.
```

---

## What a good answer looks like

The agent should produce something along these lines — shape, not values:

> **As of epoch 1012, 51% elapsed.** Commands run: `view-config`, `view-state --vote-account …`,
> `view-directed-stake-meta`.
>
> **Eligible:** yes — all filters pass, including BAM connectivity (8 of the trailing 11 epochs).
>
> **In the delegation set:** yes, rank 272 of 312. Target 30,649 SOL. Your undirected balance is
> 278 SOL, so you are 30,371 SOL short. You hold no directed stake.
>
> **Queued ahead of you:** 24,568 SOL of shortfall belonging to higher-ranked validators.
>
> **Budget:** the scoring-unstake budget for this cycle is fully consumed (717,368 of 717,179 SOL).
> It resets at epoch 1020.
>
> **Binding constraint: `next_rebalance`.** The reserve currently holds 62,732 SOL, more than the
> 24,568 SOL queued ahead of you, so you are next in line. *If* the reserve is still around that
> level at the next rebalance and nothing above you claims it first, you would receive your full
> shortfall. That is conditional on the reserve, which moves continuously.
>
> **What you control:** you already clear every filter, so your lever is rank. Your inflation
> commission is 5%, at the threshold — validators at 0–2% rank above you and are funded first.
>
> **I will not tell you when you reach target beyond next epoch.** The set is re-scored at epoch
> 1020: N changes, every target moves with TVL, and new validators enter the queue. Any number I
> gave you past that point would be invented.

Note what is missing: no date, no "you'll be at target in N epochs", and the one forward-looking
claim is explicitly conditional on a value that changes.

---

## Why it is shaped this way

The honest parts of "when do I get stake" are all *present-tense*: eligibility, target, rank, queue
position, remaining budget. Those come straight off the chain and an agent can state them flatly.

The part everyone wants — a date — depends on the rate of deposits and withdrawals, on every other
operator's decisions, and on a re-scoring event that reshuffles the set every cycle. None of that is
predictable, which is why this is a prompt you run against live data rather than a number anyone
publishes. Run it again next epoch and the firm facts will have moved a little; that movement is the
point, not a defect.
