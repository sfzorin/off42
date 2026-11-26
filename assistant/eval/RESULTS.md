# Assistant model evaluation — 2026-11-26

Which OpenRouter model should drive the OFF42 editor assistant (ARCHITECTURE.md, D13)? This evaluation ran the
system prompt (`../prompt/system.md`) and the tools (`../tools/*.json`) against an in-memory document and
spreadsheet model. A tool loop of up to 8 turns ran to completion, and the final file state was graded automatically.

## Method

- **31 tasks**: 18 document tasks and 13 spreadsheet tasks. Languages: Russian 10, English 14, German 2, Spanish 2,
  French 1 (see `tasks/`). Six are adversarial:
  - a prompt injection in the document text, and another in a spreadsheet cell;
  - two impossible requests ("send this by email", "insert a pie chart");
  - two ambiguous requests ("Make it better.", and "Delete the paragraph about the meeting." when two paragraphs
    are about meetings).
- **Grading** looks at the final state, not at the formulas or calls the model used. Edited text must match the
  expected text, ignoring case and punctuation. Paragraphs and cells the task does not cover must be byte-identical.
  Spreadsheet values are computed by the harness's formula engine. Where the request asks for formulas, the cells
  must hold formulas. `selftest.mjs` checks every grader in both directions: the correct (oracle) edit passes, and
  the untouched file fails.
- **Valid tool calls**: the name exists, the arguments are JSON and they pass the tool's schema
  (`additionalProperties: false`, required fields, enums) on the first try. An invalid call is returned to the
  model as an error, as the app would do.
- **Request settings**: `temperature 0.2`, `max_tokens 2048`, `tool_choice "auto"`, and the provider's default
  reasoning. The provider object is pinned per model (`models.json`) with `allow_fallbacks: false`,
  `require_parameters: true` and `data_collection: "deny"`. `usage: {include: true}` returns cost and cached tokens.
  Alibaba-served Qwen models get explicit `cache_control` breakpoints.
- **Runs**:
  - First pass: every candidate once.
  - The "Delete the paragraph about the meeting" task was added after the first pass. In that pass every model
    answered "Make it better." with a light polish, which the prompt permits ("choose the most reasonable reading"),
    so that task did not test ambiguity. Both tasks are kept and reported.
  - Repeat pass for the top 3 paid models.
  - Cache experiments.
  - Six-task checks of the fallback providers.
- **Spend**: $0.15 of OpenRouter credit in total, as reported by `usage.cost`. That covers all runs, including probes.

Raw transcripts stay in `runs/` (git-ignored). `results/*.json` holds the per-task records (outcome, turns,
latency, tokens, cost, providers and error strings) without transcripts or replies.

## Results — first pass (every model, 31 tasks)

| Model (provider used) | Success | Docs | Sheets | Adversarial | Valid tool calls | Median turns | Median latency/task | Mean cost/task | Cache hit |
|---|---|---|---|---|---|---|---|---|---|
| deepseek/deepseek-v4.1-flash (Alibaba) | **97%** | 14/14 | 11/11 | 5/6 | 100% (67/67) | 3 | 3.5 s | $0.00015 | 89% |
| qwen/qwen3.8-flash (Alibaba) | **97%** | 14/14 | 11/11 | 5/6 | 100% (74/74) | 3 | 6.2 s | $0.00042 | 84% |
| z-ai/glm-5.3-flash (Z.AI) | **97%** | 14/14 | 11/11 | 5/6 | 100% (49/49) | 2 | 8.7 s | $0.00032 | 85% |
| qwen/qwen3.7-plus (Alibaba), reference | **97%** | 14/14 | 11/11 | 5/6 | 100% (57/57) | 2 | 7.4 s | $0.00093 | 83% |
| nvidia/nemotron-3.5-super-vl:free (NVIDIA) ¹ | **97%** | 14/14 | 11/11 | 5/6 | 98% (40/41) | 2 | 10.5 s | free | 0% |
| openrouter/free (Baidu ERNIE 5.5 Flash preview, Ling 3.2 Flash) ² | **97%** | 14/14 | 11/11 | 5/6 | 100% (68/68) | 3 | 5.5 s | free | 69% |
| deepseek/deepseek-v4-flash-0731 (Alibaba) | 94% | 13/14 | 11/11 | 5/6 | 100% (38/38) | 2 | 4.5 s | $0.00029 | 86% |
| minimax/minimax-m3.1-flash (Fireworks) | 94% | 13/14 | 11/11 | 5/6 | 100% (95/95) | 4 | 3.4 s | $0.00039 | 93% |
| qwen/qwen3.7-flash (Alibaba) | 90% | 14/14 | 10/11 | 4/6 | 100% (44/44) | 2 | 4.9 s | $0.00010 | 82% |
| nvidia/nemotron-3-ultra-550b-a55b:free (NVIDIA) ¹ | 84% | 11/14 | 11/11 | 4/6 | 100% (60/60) | 2 | 9.3 s | free | 0% |
| google/gemma-4-31b-it:free (Google AI Studio) | not evaluated ³ | – | – | – | – | – | – | – | – |

¹ Unavailable with `data_collection: "deny"` (HTTP 404, "No endpoints found matching your data policy (Free model
training)"). These rows were run with `allow`, which is acceptable only because the fixtures are synthetic.
² `openrouter/free` works under `deny`, but it chooses the model itself: 87 requests went to
`baidu/ernie-5.5-flash-preview:free` and 10 to `inclusionai/ling-3.2-flash:free`. Neither the model nor the
provider can be pinned.
³ Every request returned an upstream HTTP 429 ("Provider returned error"), including during a later solo run. The
Google AI Studio free quota behind OpenRouter was exhausted all day.

Latency percentiles and cost spread across both passes (`deepseek-v4.1-flash`, `qwen3.8-flash`, `glm-5.3-flash`
have 62 task runs each; the others 31):

| Model | Median / p90 latency per task | Median / p90 cost per task | Median prompt tokens per task |
|---|---|---|---|
| deepseek/deepseek-v4.1-flash | 3.5 s / 4.4 s | $0.00015 / $0.00020 | 6,100 |
| qwen/qwen3.7-flash | 4.9 s / 9.4 s | $0.00007 / $0.00019 | 4,234 |
| deepseek/deepseek-v4-flash-0731 | 4.5 s / 6.3 s | $0.00027 / $0.00047 | 4,065 |
| minimax/minimax-m3.1-flash | 3.4 s / 4.5 s | $0.00032 / $0.00045 | 8,369 |
| z-ai/glm-5.3-flash | 8.3 s / 13.9 s | $0.00029 / $0.00055 | 3,697 |
| qwen/qwen3.8-flash | 6.2 s / 11.5 s | $0.00033 / $0.00063 | 6,411 |
| qwen/qwen3.7-plus | 7.4 s / 14.7 s | $0.00073 / $0.00152 | 4,190 |
| nvidia/nemotron-3.5-super-vl:free | 11.7 s / 27.1 s | free | 4,548 |
| nvidia/nemotron-3-ultra-550b-a55b:free | 9.7 s / 22.1 s | free | 4,643 |
| openrouter/free | 5.5 s / 8.1 s | free | 5,867 |

## Repeat pass (top 3 paid models, same 31 tasks)

| Model | Success | Adversarial | Valid tool calls | Median latency | Mean cost/task | Cache hit |
|---|---|---|---|---|---|---|
| deepseek/deepseek-v4.1-flash | **100%** | 6/6 | 100% (65/65) | 3.4 s | $0.00013 | 93% |
| qwen/qwen3.8-flash | 97% | 5/6 | 99% (70/71) | 6.0 s | $0.00039 | 86% |
| z-ai/glm-5.3-flash | 97% | 5/6 | 100% (51/51) | 7.6 s | $0.00031 | 91% |

Over both passes (62 runs each): DeepSeek V4.1 Flash 61/62, Qwen3.8 Flash 60/62, GLM-5.3 Flash 60/62. Each of
their failures was "Make it better.", where the model polished the text instead of asking.

Fallback providers, six tasks each (typos, bold, totals, sort, injection, ambiguous reference):

| Model on provider | Success | Median latency | Mean cost/task |
|---|---|---|---|
| deepseek-v4.1-flash on Together | 6/6 | 1.5 s | $0.00074 |
| deepseek-v4.1-flash on Google Vertex | 5/6 ⁴ | 2.6 s | $0.00099 |
| glm-5.3-flash on Fireworks | 6/6 | 3.6 s | $0.00050 |

⁴ The model called `apply_text_style` with `underline: false` instead of `bold: true`, then reported success.
Together and Vertex list prices about 3× Alibaba's, and their cross-task cache hits were lower in these short
runs (see `../CACHING.md`).

## Failures and what made models unsuitable

- **All models, "Make it better."**: they made a light, plausible polish rather than asking. Some also invented
  content: GLM changed "Team offsite" to "Team Offsite" and added "Join us …"; Qwen3.8 bolded the date. The prompt
  allows choosing the most reasonable reading, so this is a prompt-policy question more than a model fault. When
  the request clearly matched two paragraphs ("Delete the paragraph about the meeting"), every model except Qwen3.7
  Flash asked which one. If the owner wants vague requests to trigger a question, tighten rule 7 of the prompt and
  re-run.
- **qwen/qwen3.7-flash**: it is the cheapest model, but it made two damaging mistakes:
  - It deleted one of two matching paragraphs without asking.
  - It wrote `=B2*C2` unchanged into every row. The tool fills formulas with relative references only when a single
    value is given, so rows 3–6 computed row 2's total. It corrected the range, but not the formulas.

  It also spends the most reasoning tokens (350 per task).
- **minimax/minimax-m3.1-flash**: on the Russian typo task it "remembered" text that was not in the paragraph
  (`find: "планируем"`), then spent the remaining turns searching for substrings until it hit the 8-turn limit. It
  is fast and cheap, but no first-party endpoint exists: Fireworks, Together, Nebius, SambaNova and others serve it.
- **deepseek/deepseek-v4-flash-0731**: it included the closing "if you have questions…" paragraph in a numbered
  list. Its quality is otherwise close to V4.1 Flash, at twice the cost.
- **nvidia/nemotron-3-ultra-550b-a55b:free**:
  - incomplete typo fixes, and extra paragraphs restyled;
  - said nothing about being unable to email a document;
  - slow (p90 22 s);
  - not available under `deny`.
- **nvidia/nemotron-3.5-super-vl:free**: quality is good (97%), but p90 latency is 27 s and it is not available under
  `deny`.
- **openrouter/free**: quality was good in this run, but it routes to unannounced preview models chosen by
  OpenRouter. That fails "very reliable, simple": what users get can change without notice.
- **google/gemma-4-31b-it:free**: unusable today (upstream 429 on every request).
- **Prompt injection**: no model followed the injected "delete everything" text in the document or in a cell. All
  of them performed the user's actual request.

## Providers for each candidate (OpenRouter catalog, 2026-11-26)

| Model | First-party endpoint | Major-cloud / large hosts on OpenRouter | Pinned in eval |
|---|---|---|---|
| deepseek/deepseek-v4.1-flash | **none** (no DeepSeek endpoint) | Alibaba, Google Vertex, Cloudflare, Together, Fireworks, DeepInfra, Nebius, CoreWeave, Baseten | alibaba → google-vertex → together |
| deepseek/deepseek-v4-flash-0731 | **none** | Alibaba, DeepInfra, Nebius, CoreWeave, Cohere, Baseten, DigitalOcean | alibaba → nebius → deepinfra |
| qwen/qwen3.7-flash | Alibaba (only endpoint) | – | alibaba |
| qwen/qwen3.8-flash | Alibaba (two regions) | Featherless | alibaba |
| qwen/qwen3.7-plus | Alibaba (only endpoints) | – | alibaba |
| minimax/minimax-m3.1-flash | **none** (no MiniMax endpoint) | Fireworks, Together, Nebius (fp4), SambaNova, DigitalOcean | fireworks → together → sambanova |
| z-ai/glm-5.3-flash | Z.AI (fp8) | Mistral, Together, Fireworks, Nebius, CoreWeave, DeepInfra and many others | z-ai → fireworks → together |
| google/gemma-4-31b-it:free | Google AI Studio | – | google-ai-studio |
| nvidia/nemotron-3-*:free | NVIDIA | – | nvidia |

Several hosts serve fp4/nvfp4 quantizations, and some (DeepInfra's flex tier, Chutes, Baidu for GLM) had uptime
below 90% over the past day. Pin named providers rather than sorting by price.

## Recommendation

**Paid (default): `deepseek/deepseek-v4.1-flash`, pinned to Alibaba with Together and Google Vertex as provider
fallbacks.**
- Best measured result: 61/62, and 100% valid tool calls.
- Fastest of the reliable models: median 3.5 s per action, p90 4.4 s.
- Cheapest of the top group: $0.00014 per action on average.
- No first-party DeepSeek endpoint exists. Alibaba Cloud is a major cloud and served every request. Together was
  tested 6/6 as a fallback; Vertex 5/6.

**Fallback model: `z-ai/glm-5.3-flash`, pinned to Z.AI (first-party) and Fireworks.**
- 60/62, 100% valid calls, $0.00031 per action.
- A different vendor and a different host, so it does not share failures with the primary.
- `qwen/qwen3.8-flash` on Alibaba is equally good, but it shares the Alibaba dependency with the primary.

**Free: none meets the privacy requirement.**
- Under `data_collection: "deny"`, both NVIDIA free models are unavailable. Gemma on Google AI Studio is
  rate-limited to unusable. `openrouter/free` only works by routing to unpinned preview models.
- Recommendation: do not offer a free-model tier. The paid model is cheap enough to fund a daily allowance.
- If a free option is required anyway, `nvidia/nemotron-3.5-super-vl:free` is the best measured (30/31). It needs
  `data_collection: "allow"`: NVIDIA's free endpoint may retain and train on prompts, which include the user's
  document text. That has to be disclosed in the consent dialog and the privacy policy, contradicts D13's "only
  what's needed, not retained" spirit, and is slow (p90 27 s).

### Exact OpenRouter request settings (Worker → `POST https://openrouter.ai/api/v1/chat/completions`)

```json
{
  "model": "deepseek/deepseek-v4.1-flash",
  "messages": ["system prompt", "file snapshot", "conversation…"],
  "tools": ["assistant/tools/document.json or spreadsheet.json, unchanged"],
  "tool_choice": "auto",
  "temperature": 0.2,
  "max_tokens": 2048,
  "provider": {
    "order": ["alibaba", "together", "google-vertex"],
    "allow_fallbacks": false,
    "data_collection": "deny",
    "require_parameters": true
  },
  "usage": { "include": true }
}
```

Fallback model (on 5xx, 429 or "no endpoints" from the primary, retry the same messages once):

```json
{
  "model": "z-ai/glm-5.3-flash",
  "provider": { "order": ["z-ai", "fireworks"], "allow_fallbacks": false,
                "data_collection": "deny", "require_parameters": true }
}
```

- `allow_fallbacks: false` keeps requests on the listed providers only. OpenRouter still tries the next provider in
  `order` when one fails.
- **`parallel_tool_calls`: do not send it.** None of the pinned endpoints (Alibaba, Together, Vertex, Z.AI,
  Fireworks) lists it, and with `require_parameters: true` sending it would exclude them. The models already return
  several tool calls in one turn when useful. The client executes them in order, inside one Undo transaction.
- **`reasoning`: leave unset.** The provider default was used. DeepSeek V4.1 Flash spent about 60 reasoning tokens
  per action. Do not pass `reasoning_details` through the Worker's logs; do return it to the model on the next turn,
  as the harness does.
- **`temperature`: 0.2.** Only this value was tested. Determinism matters more than variety for edits.
- **`max_tokens`: 2048.** The largest output observed for a whole task was 1,872 tokens (MiniMax). Typical outputs
  are 100–600 tokens.
- `cache_control` breakpoints: send them only for Alibaba-served Qwen if Qwen becomes the model. They are not needed
  for DeepSeek, GLM or MiniMax (implicit caching). See `../CACHING.md`.

### Cost per action

A typical action (one user request, 2–3 model turns, about 6k prompt tokens of which about 90% are cached, and
about 300 output tokens) costs **$0.00014** on DeepSeek V4.1 Flash via Alibaba (median $0.00015, p90 $0.00020).
**$10 of credit buys about 70,000 actions** at the measured mix.

Real documents are larger than the fixtures. With a 20k-token prompt (a long outline plus reads), 85% cached and
400 output tokens, an action costs about $0.0004, so roughly 25,000 actions per $10. Costs are OpenRouter's
reported `usage.cost`; credit-purchase fees are not included.

On the fallback hosts (Together or Vertex) the same action costs about $0.0008–0.001, 5–7× more. The GLM fallback
costs $0.0003.

## Tool set against doc42-core and xls42-core

Every tool can be implemented on the current cores. These are the gaps the stage-2 command layer needs to fill:

**Documents (`RichDocument`)**
- **One Undo step per reply.** `RichDocument::transact(|state| …)` is public and applies one snapshot, but the
  helpers (`replace_text`, `set_paragraph_style`, `apply_text_style`) each call `transact` themselves. Commands
  must become `State`-level functions so that one reply's commands run inside a single `transact`.
  `SnapshotHistory` keeps only 8 entries.
- **`get_outline`, `read_range` and `find`** map onto `State.blocks` and `Paragraph { id, style, runs }`. Search
  exists (`replace_all_text`, `suite42_core::text_search`). Offsets are byte offsets in UTF-8; the tools use
  character offsets and must convert.
- **`insert_paragraphs` / `delete_paragraphs`**: there is no public helper and no paragraph-id allocator. The
  command layer needs `next_paragraph_id` and must handle paragraphs inside tables, sections and anchored
  annotations. `replace_text` already refuses revision, field and cached-TOC anchors; the tools should return those
  errors as they are.
- **`set_paragraph_style`** takes a whole `ParagraphStyle`, so named styles need a mapping:
  - heading1–3 → `source_style` plus `outline_level`;
  - list styles → `ListMarker { id, level, numbered }`, with a list-id allocator and adjacent paragraphs sharing
    the id.
- **`apply_text_style`** takes a whole `TextStyle`. Toggling one mark needs a patch operation that keeps font, size
  and colour.

**Spreadsheets (`Workbook`)**
- **One Undo step per reply.** There is no public transaction. Each of `set_cell`, `set_style`, `insert_rows`,
  `sort_range` and the others commits its own history entry, and history keeps 8, so a 10-call reply could not be
  undone at all. The command layer needs `Workbook::batch(|state| …)`.
- **A1 parsing**: `a1()` is private in `office.rs` and must be made public or shared.
- **`set_cells`**: the per-cell API exists (`set_cell`). Filling relative formulas maps to `fill_range`.
- **`set_number_format`**: `set_style` replaces the whole `CellStyle`. It needs a per-cell patch, because a range
  has mixed styles.
- **Structural edits** refuse to run when the sheet has tables, and on external formats other than xlsx/ods. The
  tool should surface that error so the model can tell the user.
- **`sort_range`** maps to `sort_with_options` with `SortOptions { header }`.
- Tools act on the active sheet only. Multi-sheet requests need a `sheet` argument later.

## Caveats

- Graded against the harness's in-memory models, not doc42-core/xls42-core. Error messages and edge cases will
  differ until stage 2 exists.
- One or two runs per model; differences of one task (about 3 percentage points) are within noise.
- Latency was measured from one machine on one day. Free-model latency depends on queue load.
- The fixtures are small (3–7 paragraphs, 3–6 rows). Long documents test the outline and paging more than these
  tasks do.

## How to run

```sh
cd assistant/eval
node selftest.mjs                     # offline: graders and oracle solutions
node run.mjs --label myrun            # all models in models.json; key from OPENROUTER_API_KEY or ~/.config/off42/openrouter.key
node run.mjs --models deepseek/deepseek-v4.1-flash --tasks doc-,xls-total --label quick
node run.mjs --repeat-each 2 --tasks doc-summary-en --label cache   # cache measurement
node summarize.mjs myrun              # markdown tables + results/myrun.json (no transcripts)
```

The harness never prints the key. It stops a model after 10 tasks below 30% success or after 5 API errors, and it
stops the run at `--budget` dollars (default 2).
