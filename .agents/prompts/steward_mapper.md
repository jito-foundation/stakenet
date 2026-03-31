Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Produce a claim bundle for `programs/steward`.
- Include lifecycle/state-machine claims, scoring/delegation claims, and authority/admin claims.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/claim_bundle.schema.json`.
- Claim IDs must start with `STW_`.
- Output as many strongly-supported claims as warranted by evidence.
- Use stable zero-padded numeric IDs (`STW_001`, `STW_002`, ...).
- Every claim needs concrete evidence paths and optional symbol/line.
- Sort claims by `claim_id`.
- Do not invoke `python`; use shell tools only.
