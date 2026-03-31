Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Produce a claim bundle for `programs/validator-history`.
- Include instruction surface, core accounts/state, authorities, and key keeper interactions.
- Integration scope is limited to these keeper entry files:
  - `keepers/stakenet-keeper/src/entries/copy_vote_account_entry.rs`
  - `keepers/stakenet-keeper/src/entries/stake_history_entry.rs`
  - `keepers/stakenet-keeper/src/entries/gossip_entry.rs`
  - `keepers/stakenet-keeper/src/entries/mev_commission_entry.rs`
  - `keepers/stakenet-keeper/src/entries/priority_fee_commission_entry.rs`
  - `keepers/stakenet-keeper/src/entries/is_bam_connected_entry.rs`
  - `keepers/stakenet-keeper/src/entries/priority_fee_and_block_metadata_entry.rs`
- Do not deep-read `keepers/stakenet-keeper/src/operations/block_metadata/*`.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/claim_bundle.schema.json`.
- Claim IDs must start with `VH_`.
- Output as many strongly-supported claims as warranted by evidence.
- Use stable zero-padded numeric IDs (`VH_001`, `VH_002`, ...).
- Every claim needs concrete evidence paths and optional symbol/line.
- Sort claims by `claim_id`.
- Maximum 20 command executions.
- Do not invoke `python`; use shell tools only.
