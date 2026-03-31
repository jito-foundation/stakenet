Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Build a deterministic structural map of this repository.
- Focus only on objective facts from code/layout.
- Include workspace members, key paths, program instruction modules, and CLI entrypoints.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/repo_index.schema.json`.
- Sort all arrays lexicographically.
- If unknown, use empty array or `[UNKNOWN]` note.
- Do not invoke `python`; use shell tools only.
