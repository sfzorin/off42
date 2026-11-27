# Scope guard for the OFF42 assistant

The assistant only works on the open file: it edits it through the tools, inserts formulas and answers questions
about its contents. Everything else (general knowledge, advice, small talk, writing that does not go into the file,
programming, unrelated maths, questions about itself, role changes, "ignore previous instructions") gets one fixed
sentence and no tool calls. The system prompt (`prompt/system.md`, section "Scope") is the primary control; the
evaluation in `eval/tasks/scope.mjs` measures it (`eval/RESULTS.md`, "Scope tasks").

| Language | Fixed reply |
|---|---|
| English | I can't answer that: I only help with this document or spreadsheet. |
| Russian | Не могу ответить: я помогаю только с этим документом или таблицей. |
| German | Das kann ich nicht beantworten: Ich helfe nur mit diesem Dokument oder dieser Tabelle. |
| Spanish | No puedo responder a eso: solo ayudo con este documento u hoja de cálculo. |
| Other | The same sentence, translated by the model. |

The prompt gives these as examples next to sample questions ("Wie spät ist es in Tokio?" → German sentence).
Listing them only by language name was not enough: without visible reasoning, DeepSeek answered short German
questions with the English sentence in up to 8 of 10 probes.

A prompt is not a guarantee. The Worker (`off42.cc/api/assistant`, D13) adds a deterministic safety
net. It must stay simple: no second model call, no classifier, no logging of document text.

## Models and data policy

Only paid models are used, every request with `provider.data_collection: "deny"`:
`deepseek/deepseek-v4.1-flash` (providers `alibaba`, `together`, `google-vertex`, `allow_fallbacks: false`), and on
429/5xx or timeout `z-ai/glm-5.3-flash` (`z-ai`, `fireworks`). No free model and no `openrouter/free` route is used,
because free endpoints may keep or train on prompts that contain the user's document. The Worker rejects any model
or provider setting not on this allowlist, whatever the browser sends.

## Spend limits

The money limits of D13 apply to every request, refused or not: $0.10 of model cost per anonymous user id and day,
$0.50 per IP hash and day, $5 site-wide per day. An out-of-scope message costs one short model call (about
$0.00001–0.00003 measured, no tool turns), so a user who only sends off-topic chatter reaches the daily limit long
after anyone would stop. Requests the Worker rejects itself (checks 1–4) make no model call and cost nothing.

## Worker checks

Requests (browser → Worker), checked before anything is forwarded:

1. **Fixed prefix.** The Worker inserts the system prompt and the tool list itself from its own copy of
   `prompt/system.md` and `tools/*.json` (selected by `kind: "document" | "spreadsheet"`). A `system` message or
   `tools` sent by the browser is rejected with 400. This keeps the prompt byte-stable for caching
   (`CACHING.md`) and stops a modified page from replacing the instructions.
2. **User message length.** A user message longer than **2,000 characters** is rejected with 400
   (`message_too_long`); the panel shows "Shorten your request". Requests are short commands or questions; long
   pasted text belongs in the document. Tool results and the file snapshot are not counted here; they have their
   own caps (snapshot 30,000 characters, one tool result 60,000, whole request 400,000).
3. **Turn count.** At most 8 model turns per user message (the eval's limit) and at most 30 user messages per
   session; then the panel asks the user to start a new session.
4. **Allowed roles.** Only `user`, `assistant` and `tool` messages after the Worker's system message; `tool`
   messages must answer a `tool_call_id` from the preceding assistant message.

Responses (OpenRouter → Worker → browser), checked before the reply reaches the browser:

5. **Unknown tools.** A tool call whose name is not in the tool list for that `kind` is dropped and replaced by the
   fixed reply. (The browser also validates arguments against the schema, as the eval harness does.)
6. **Length cap without tools.** If a response has no tool calls in the whole user turn (the model only talked),
   its text is cut to **600 characters** at the last sentence end before the limit. In-scope answers ("what is the
   total", "summarise this document") measured well under 400 characters; an essay, poem or code listing is not.
7. **Echo filter.** The reply is compared with the system prompt: if it contains any **40-character window** of
   `system.md` (after collapsing whitespace and lower-casing), or any tool name from the tool list (for example
   `set_cells`, `get_outline`), the whole text is replaced by the fixed reply in the user's language. The windows
   are precomputed once at start-up as a set of normalized strings, so the check is linear in the reply length. Tool names
   inside actual `tool_calls` are of course allowed; the check covers only the visible text.
8. **Provider names.** Visible text that matches a short deny-list (model and provider names: DeepSeek, GLM, Zhipu,
   Z.AI, Alibaba, Qwen, OpenRouter, OpenAI, GPT, Claude, Anthropic, Llama, Gemini, Mistral) is replaced by the fixed
   reply. Documents that legitimately contain such words are rare, and the cost of a false positive is one generic
   sentence the user can rephrase around.
9. **Fixed-reply language.** When the Worker substitutes the fixed reply (checks 5–8), it picks the language from
   the user's message: Cyrillic → Russian, `[äöüß]` or a German stop-word list → German, `[ñ¿¡]` or a Spanish
   stop-word list → Spanish, otherwise English. It does not call a model to translate.
10. **Wrong-language refusal.** If the model's whole reply is the English fixed sentence but check 9 detects
    another language in the user's message, the Worker swaps in that language's sentence. DeepSeek V4.1 Flash
    occasionally answers a short German question with the English sentence (about 1 in 10 in probes with the
    final prompt; see `eval/RESULTS.md`); this makes the visible result deterministic for the languages listed.

What the Worker does **not** do: it does not inspect document content, does not try to classify the user's
question as on- or off-topic (the model does that), and does not rewrite replies except by the substitutions above.
Each substitution increments a counter (no content logged), so a rise after a model or prompt change is visible.

## Worker regression cases

- A 2,001-character user message → 400; 2,000 → forwarded.
- A mocked model reply that quotes a 40-character slice of `system.md` → fixed reply; one that quotes 39
  characters → unchanged.
- A mocked reply naming `set_cells` or "DeepSeek" in text → fixed reply in the message's language.
- A mocked 2,000-character reply with no tool calls → cut to ≤ 600 characters at a sentence end; the same reply
  after a tool call in that turn → unchanged.
- A request carrying its own `system` message, `tools`, a model or a provider order → 400.
