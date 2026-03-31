Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Verify claims from all claim bundles.
- Evaluate evidence adequacy and internal consistency.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/verification_report.schema.json`.
- Be conservative: uncertain claims should be `weak` or `needs_human_review` action.
- Keep result ordering stable by `claim_id`.
- Runtime bound: do targeted checks only.
- Do not do broad repository exploration.
- For each claim, inspect only the cited evidence files/lines (plus at most one nearby context window per evidence item).
- If evidence paths/lines are missing, mark `unsupported` and request mapper fix; do not continue expanding scope.
- Prefer deterministic decisions:
  - `supported` when evidence directly matches claim text.
  - `weak` when evidence is partial/indirect but directionally consistent.
  - `conflicting` when cited evidence contradicts claim.
  - `unsupported` when evidence is absent or unrelated.
- Keep rationale concise and tied to cited evidence only.
- Do not invoke `python`; use shell tools only.
