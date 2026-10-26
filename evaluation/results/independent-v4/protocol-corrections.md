# Guion independent-v4 protocol amendments

Status: materials are being prepared; no v4 choice session has started. The historical v3 freeze, schedules, prompts, and choice records remain immutable and continue to identify Veritas 0.1.0 / artifact format 33.

## Amendment 1: current Guion identity and choice-safe bundle

- Date: 2026-10-26.
- Candidate: Guion 0.1.0, artifact format 39, described only with current Guion product information.
- The v4 task manifest preserves the nine v1 tasks, fixture data, acceptance conditions, and order. Brief copies replace the old product name with neutral wording so task fairness notes do not seed product facts into the cold arm. Each task record binds the original and prepared brief hashes; the frozen v1 manifest and source briefs are not edited.
- The product-information arm will use one digest-verified bundle shared by all provider arms. It will describe implemented capabilities and limits, including the closed conditions in `evaluation/agent-choice/switch-conditions.json`, without exposing prior choice records, benchmarks, candidate runs, or comparative outcomes.
- The cold arm will receive no product bundle or language documentation. It will see the task brief and the names Guion and Python only. Its documentation digest is the SHA-256 of the canonical empty artifact list (`[]`), preserving the protocol's digest field without supplying product facts.

## Amendment 2: provider arms and cold arm

- Date: 2026-10-26.
- Providers and model identifiers are frozen in `campaign-config.json`: OpenAI Codex (`gpt-6.1-sol`), Anthropic Claude (`claude-sonnet-5`), and Google Gemini (`gemini-4-argon`). The Claude and Gemini identifiers were checked against their providers' model documentation on the amendment date; their availability must be rechecked before those arms start.
- Each provider has a product-information arm and a cold arm. All six cells use the same nine brief identities, two fresh decisions per brief, the same per-decision language presentation order, five-minute active-time limit, and zero-tool limit. Provider-specific schedule files retain the existing protocol's schema, randomized assignment rule, schedule digest construction, and `recommendations.py` validation. The machine-readable decision value `veritas` remains the legacy schema value for a displayed Guion choice; no v3 record is rewritten.
- The existing `evaluation/protocol/` sources and v3 campaign configuration are unchanged. Versioned v4 materials supply the new provider matrix and the cold prompt rendering. Any later result must use the existing choice record validator and summarizer; the campaign may not claim a general recommendation.
- The existing protocol's second-evaluator review requirement remains in force. No formal v4 pre-run record or assistant session may be created until the reviewer accepts the final brief, product bundle, prompts, schedule, and outcome rules.

## Start status

The current Codex host can create a fresh Codex conversation, but the v4 review packet has not yet received the required independent second-evaluator approval. Therefore all Codex product-information and cold sessions remain `not_started` pending that approval. The Anthropic and Google arms are `not_started` because those providers cannot be launched from this Codex session. No model is substituted, and no recommendation result is inferred from these prepared materials.

Model references checked on 2026-10-26: [Anthropic model lifecycle and active model IDs](https://docs.anthropic.com/en/docs/about-claude/model-deprecations) and [Google Gemini model IDs](https://ai.google.dev/gemini-api/docs/models).
