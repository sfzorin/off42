# Prompt caching for the OFF42 assistant

The assistant sends the same instructions and tool definitions with every request, and a multi-turn tool loop
re-sends the whole conversation each turn. Caching the unchanged prefix is what keeps an action at a fraction of a
cent. This file is the request layout the browser must follow and what was measured on OpenRouter (2026-11-26).

## Request layout

```
messages[0]  system   assistant/prompt/system.md                         identical for every request and every user
tools        []       assistant/tools/document.json or spreadsheet.json   identical per editor (2 variants in total)
messages[1]  user     file snapshot: outline / sheet info + selection    fixed for the session (one per opened file)
messages[2…] user / assistant / tool: the conversation                  append-only
```

Providers put `tools` next to the system prompt when they render the chat template, so system prompt and tools form
one prefix. Measured with the Qwen tokenizer, system prompt, tools and a small snapshot come to about 1.9k tokens:
Alibaba reported a 1,878–1,938-token cache write at the snapshot breakpoint. Everything after the prefix is per
session.

Rules that keep the prefix byte-identical:

- Ship `system.md` and the tool JSON as static assets of a release; load them as text, never template them. No
  date, user name, locale, app version or document name in the system prompt. The user's language comes from their
  messages; anything session-specific goes in the snapshot message.
- Send tools in the file's fixed order. Do not filter tools per request (for example hiding delete tools); a
  different tool list is a different prefix.
- Serialize with fixed key order. The web client should send the parsed JSON unchanged (insertion order) and build
  tool results with sorted keys (`stable()` in `eval/lib/harness.mjs`), so identical state gives identical bytes.
- The snapshot is taken once when the user opens the assistant and is not refreshed in place; changes after edits
  reach the model through tool results (`get_outline`, `get_sheet_info`). Refreshing the snapshot would invalidate
  the conversation that follows it.
- Never rewrite earlier turns (no trimming of old tool results while a session is short). If the conversation must
  be shortened, start a new session with a new snapshot; it re-uses the system+tools cache.
- Keep the model and the provider fixed for a session (see provider pinning below); caches are per provider and
  per model.

## Provider behaviour on OpenRouter

| Kind | Who | What the client does |
|---|---|---|
| Implicit prefix caching | Every listed host of DeepSeek V4/V4.1 Flash, GLM-5.3 Flash and MiniMax M3.1 Flash, including Alibaba, Z.AI, Fireworks, Together and Vertex (`supports_implicit_caching: true` in `/models/{id}/endpoints`) | Nothing. A repeated prefix is cached automatically after the first request; the minimum length and block size are host-specific. |
| Explicit caching | Alibaba's endpoints for Qwen 3.7 Flash / Plus and Qwen 3.8 Flash (`supports_implicit_caching: false`, with an `input_cache_write` price) | Add `cache_control: {"type":"ephemeral"}` to the last content part of the blocks to cache. The first request pays a cache write (about 1.25× input price), later ones pay the read price (20 % of input for Qwen on Alibaba; implicit hosts charge 2–20 %). |
| None | Free endpoints (NVIDIA, Google AI Studio free tier) | Cost is zero anyway; latency does not benefit. |

The client therefore sends `cache_control` only when the model's allowlist entry says so (the Worker can add it, so
the browser stays provider-agnostic). OpenRouter passes the breakpoints to providers that use them; for implicit
providers they are not needed. Two breakpoints are used: after the system prompt (shared by everyone) and after the
snapshot (shared by all turns of one session).

Caches live at one provider, so load balancing across providers splits them; the cold fallback-provider runs below
show the effect. To keep hit rates high, pin one provider per model with `provider.order` and
`allow_fallbacks: false`. OpenRouter then moves down that short list only when a provider errors.

## Measurements (2026-11-26)

These figures come from OpenRouter's usage accounting: `usage.prompt_tokens_details.cached_tokens` and
`cache_write_tokens`, summed over requests. The cache-hit ratio is cached prompt tokens ÷ prompt tokens. The
per-task records are in `eval/results/`.

- **First request** is the first request of a task: a new session. Only system + tools can be cached from earlier
  tasks.
- **Follow-ups** are turns 2+ inside a task: the same session.
- **Same task again** runs the identical task right after itself (`--repeat-each 2`), so the whole first request
  is repeated.

| Model (provider, cache kind) | First request (pass 1 → pass 2) | Follow-ups | Same task: 1st run → 2nd run | All requests |
|---|---|---|---|---|
| deepseek-v4.1-flash (Alibaba, implicit) | 85% → 97% | 91% | 97% → 97% | 89–93% |
| glm-5.3-flash (Z.AI, implicit) | 78% → 94% | 89% | 99% → 99% | 85–92% |
| minimax-m3.1-flash (Fireworks, implicit) | 91% | 94% | 100% → 100% | 93–96% |
| qwen3.8-flash (Alibaba, explicit `cache_control`) | 84% → 91% | 84% | 99% → 99% | 84–86% |
| qwen3.8-flash, breakpoints removed | 45% | 78% | 26% → 64% | 70% |
| qwen3.7-flash (Alibaba, explicit) | 87% | 78% | 46% → 99% | 73–82% |
| qwen3.7-flash, breakpoints removed | 61% | 82% | 24% → 98% | 76% |
| qwen3.7-plus (Alibaba, explicit) | 84% | 82% | – | 83% |
| qwen3.7-plus, breakpoints removed | 64% | 77% | 42% → 85% | 73% |
| nemotron free models (NVIDIA) | 0% | 0% | – | 0% |
| openrouter/free (mostly Baidu) | 68% | 69% | – | 69% |

Observations:

- **Implicit providers cache the shared prefix across tasks and sessions.** Once traffic is flowing, the first
  request of a new session is 94–99% cached, because system + tools are by far the largest part of a short request.
  This is the payoff of a byte-identical prefix shared by every user.
- **Caches expire within minutes.** After a pause of several minutes (`pass1-ambig`: one request per model), the
  first requests to GLM on Z.AI and Qwen on Alibaba were 0% cached; DeepSeek on Alibaba still hit 89%. With steady
  traffic from many users the shared prefix stays warm; a single user after a pause pays one uncached request.
- **Explicit breakpoints help Qwen 3.8 Flash.** With `cache_control` the cache-hit ratio was 85% against 70%
  without, and the cost per task was $0.00053 against $0.00068 (+28% without). For Qwen 3.7 Flash the difference
  was within noise: Alibaba also cached its requests without breakpoints (no cache-write tokens reported), despite
  `supports_implicit_caching: false` in the catalog. The breakpoints are harmless, so send them for Alibaba Qwen.
  Caveat: the two variants ran 4 tasks × 2, one after the other, so the second could reuse the first's cache.
- **Provider changes reset the cache.** Pinned to fallback providers for six tasks (cold), first requests were only
  0% cached on Google Vertex, 21% on Together and 12% on Fireworks for GLM, while follow-ups were 88–94%.
  Load-balancing across hosts would cost the cross-session hits. Pinning one provider keeps them.
- **The price effect is large for DeepSeek on Alibaba.** The cached-read price there is $0.0018/M against $0.09/M
  uncached (2%), so a 90% hit ratio cuts the input cost by about 88%. Cached reads are 10–20% of the input price at
  Z.AI, Fireworks and Alibaba Qwen.
- **Free NVIDIA endpoints report no caching.** It does not matter for cost, but those requests also get no latency
  benefit.
