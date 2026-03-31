Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Render deterministic documentation payload from verified claims only.
- Produce multiple docs under `docs/autogen/` including:
  - overview.md
  - reference-validator-history.md
  - reference-steward.md
  - reference-keeper.md
  - reference-cli.md
  - coverage-tests.md

Rules:
- Output JSON only.
- Validate against `.agents/schemas/rendered_docs.schema.json`.
- Do not include run-specific metadata or timestamps.
- Preserve stable section order and deterministic phrasing.
- Use only verified-claims input artifacts; do not rescan source files unless required to resolve a direct contradiction.
- Keep language factual and compact; avoid speculative statements.
- Do not invoke `python`; use shell tools only.
- Deterministic template requirement:
  - Use this exact file order: overview, reference-validator-history, reference-steward, reference-keeper, reference-cli, coverage-tests.
  - Use fixed headings only (`#`, then `##`) and keep heading names stable across runs.
  - Prefer verbatim claim text from input; do not paraphrase unless needed for grammar.
  - Keep bullet ordering strictly by `claim_id` within each section.
  - Do not invent new claims or combine multiple claim IDs into one bullet.
