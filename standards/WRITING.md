# Writing

> Standards for all prose: documentation, READMEs, specs, PR descriptions, commit messages, inline comments, emails, reports. If a human reads it, these rules apply.
>
> **Key rules:** Answer first, active voice, short sentences, no em dashes, no AI tropes (50+ banned), Oxford comma, sentence case headers, FK grade 7-8.

---

## Core Principles

**Say the thing.** Every sentence should advance the reader's understanding. If you remove a sentence and nothing is lost, it shouldn't have been there.

**Answer first.** Lead with the conclusion, decision, or key fact. Supporting context follows. The reader should know the point before they know the reasoning.

**Concrete over abstract.** "The request timed out after 30 seconds" not "there was a temporal issue with the request lifecycle." Specific numbers, specific names, specific outcomes.

**Earn every word.** If a shorter version says the same thing, use it. "Use" not "utilize." "Because" not "due to the fact that." "Now" not "at this point in time."

---

## Voice

### Active Voice

The subject does the thing. Passive voice hides the actor and weakens the statement.

| Active (preferred) | Passive (avoid) |
|---|---|
| The server rejects malformed requests | Malformed requests are rejected by the server |
| We chose snafu for context propagation | snafu was chosen for its context propagation |
| Run `cargo test` before pushing | Tests should be run before pushing |

Passive is acceptable when the actor is genuinely irrelevant or unknown: "The file was last modified in March."

### Tense

- **Present** for how things work: "The config loader reads from three sources."
- **Past** for what happened: "We migrated from thiserror to snafu in January."
- **Imperative** for instructions: "Run the tests. Check the output. Fix failures before pushing."

### Person

- **Second person** for instructions: "You can configure this with..."
- **First person plural** for team decisions: "We use snafu because..."
- **Third person** for system descriptions: "The gateway routes messages to..."
- **Never first person singular** in technical docs: not "I think" or "I believe."

---

## Formatting

### Punctuation

- **No em dashes.** Use commas, parentheses, colons, or separate sentences. Em dashes are typographic crutches that interrupt flow and signal unstructured thinking.
- **Oxford comma.** Always. "Red, white, and blue" not "red, white and blue."
- **One space after periods.** Not two.
- **Straight quotes.** `"` and `'`, not curly/smart quotes.

### Sentence Structure

- **Short sentences.** If a sentence has three clauses, it's probably two sentences. Break it.
- **One idea per sentence.** Complex thoughts get multiple sentences, not semicolons.
- **Parallel structure.** Lists, comparisons, and sequences use the same grammatical form.
- **Vary sentence length.** Uniform 15-20 word sentences read as machine-generated. Mix short fragments with longer explanatory sentences. The rhythm should be uneven.

### Structure Over Prose

Use the right format for the content:

| Content type | Format |
|---|---|
| Comparisons | Table |
| Steps or sequences | Numbered list |
| Properties or features | Bullet list |
| Decisions with context | Header + short paragraph |
| Reference material | Table |
| Narrative or reasoning | Prose paragraphs |

Don't write three paragraphs when a table says it better. Don't make a bullet list when the content needs narrative.

### Headers

- **Sentence case** for document headers: "Error handling patterns" not "Error Handling Patterns"
- Exception: `UPPER_SNAKE` for canonical doc filenames (STANDARDS.md, ARCHITECTURE.md)
- Headers describe content, not categories: "Sessions persist across restarts" not "Session Persistence Feature"
- No trailing punctuation on headers
- **One H1 per page.** The page title.
- **Sequential nesting.** H1 > H2 > H3. Never skip levels. Screen readers present headings as a navigable list — they must form a coherent outline.
- **Descriptive in isolation.** Each heading must make sense when listed without surrounding text.

### Code in Prose

- Inline code for identifiers, paths, commands, values: `session_store`, `/etc/config`, `cargo test`, `true`
- **Bold** for UI elements: **Save**, **Settings**, **Cancel**
- *Italic* for placeholders and parameters: *filename*, *port*
- Code blocks with language tag for multi-line examples
- Introduce code blocks with a sentence ending in a colon
- Don't use code formatting for emphasis. **Bold** is for emphasis.

### Links

- Link text must describe the destination: "see the [API reference](url)" not "[click here](url)"
- Never use "click here," "learn more," or "this link" as standalone link text
- Screen readers can list all links on a page — each must make sense without surrounding context

### Numbers

- Spell out one through nine. Use digits for 10+.
- Always digits for: measurements, versions, times, counts in technical context
- Use units: "30 seconds" not "30s" in prose (abbreviations fine in tables and code)

### Spatial References

Don't use directional language to reference content: "as shown above," "in the right sidebar," "the section below." Name the element directly: "see the Configuration section," "in the **Settings** panel." Content reflows, gets reordered, and renders differently across devices. Spatial references break.

---

## Banned Words and Phrases

These appear constantly in AI-generated text. They signal pattern-matching, not thinking. Never use them.

### AI Tropes

| Banned | Why | Alternative |
|---|---|---|
| delve | AI verbal tic, adds nothing | examine, explore, look at (or just start) |
| leverage | corporate jargon | use |
| utilize | bureaucratic inflation of "use" | use |
| facilitate | vague, passive | enable, support, run, or describe what happens |
| streamline | marketing language | simplify, reduce, speed up |
| robust | meaningless without specifics | describe what makes it strong |
| comprehensive | usually false, always vague | complete, thorough, or list what's covered |
| enhance | vague uplift word | improve, add, extend, or describe the change |
| foster | AI-overused verb | encourage, support, build |
| showcase | AI-overused, use a direct verb | demonstrate, show, present |
| underscore | AI-overused emphasis verb | emphasize, highlight (or restructure) |
| illuminate / elucidate | pretentious synonyms for "explain" | explain, clarify, show |
| navigate (metaphorical) | "navigate the challenges" is empty | handle, address, work through |
| embark | "embark on a journey" is a cliche | start, begin |
| harness | marketing verb | use, apply |
| pivotal | AI-overused adjective | important, critical (or explain why) |
| intricate | AI-overused where "complex" works | complex, detailed |
| meticulous | AI-overused praise word | careful, thorough, precise |
| multifaceted | vague complexity signal | complex, or describe the facets |
| nuanced | often means "I can't be specific" | describe the actual nuance |
| paramount | inflated "important" | important, critical |
| profound | rarely earned | significant, or describe the depth |
| groundbreaking | marketing | new, first, original (with evidence) |
| holistic | vague buzzword | complete, whole-system, end-to-end |
| invaluable | cliche praise | valuable, essential (or explain why) |
| tapestry / landscape / realm | AI metaphor placeholders | describe the actual domain |
| testament (e.g., "a testament to") | filler praise | evidence of, demonstrates |
| journey (metaphorical) | cliche | process, progression, or describe it |

### Filler Phrases

| Banned | Alternative |
|---|---|
| it's worth noting / it's important to note | state the thing |
| it should be noted | state the thing |
| in order to | to |
| a wide range of | many, several, or the number |
| at the end of the day | ultimately, or delete |
| as such | so, therefore, or restructure |
| in terms of | about, for, regarding, or restructure |
| dive deep / deep dive | examine, analyze, investigate |
| in today's [adjective] world | delete entirely |
| in the ever-evolving | delete entirely |
| when it comes to | for, regarding, or restructure |
| serves as / functions as / stands as | is |
| plays a [crucial/key/vital] role | matters, contributes, or describe how |
| aims to bridge | connects, links (or describe the connection) |
| paving the way | enabling, or describe what it enables |
| sheds light on | explains, reveals, shows |
| at its core | fundamentally, or just state the core thing |
| key takeaway(s) | the point is, or just state it |
| offers valuable insights/perspectives | teaches, shows, reveals (be specific) |
| at this point in time | now |
| going forward | from now on, or delete |
| please note that | state the thing |
| as mentioned above/below | reference by name, or omit |
| needless to say | then don't say it |
| as a matter of fact | delete, state the fact |
| arguably | state the argument or don't |
| it seems / it appears | investigate and state the finding |
| basically / essentially | say the thing directly |
| in this context | the reader knows the context |
| not only... but also | use when genuinely contrasting; never as a default sentence opener |

### Performative Language

- No superlatives without evidence: "the fastest," "the most elegant," "incredibly powerful"
- No commentary on quality: "this is a great approach," "elegant solution," "clever implementation"
- No social performance: "I'd be happy to," "great question," "absolutely"
- No motivational language: "exciting opportunity," "game-changing improvement"

### Transition Word Density

Avoid opening consecutive paragraphs with transition words: Moreover, Furthermore, Additionally, However, Nevertheless, Consequently, In addition, Similarly. One transition per three paragraphs is a reasonable ceiling. Juxtapose ideas — the reader can infer the relationship.

---

## Inclusive Language

Concrete replacements. Most of these are more precise than the terms they replace.

| Deprecated | Replacement |
|---|---|
| master/slave | primary/replica, leader/follower, controller/worker |
| whitelist/blacklist | allowlist/denylist, include list/exclude list |
| master (branch) | main |
| master (authoritative) | primary, original, source, reference |
| sanity check | validation check, smoke test, soundness check |
| dummy (value) | placeholder, stub, mock, sentinel |
| grandfathered | legacy, exempt, preexisting |
| man-hours | person-hours, engineer-hours |
| man-in-the-middle | on-path attack, machine-in-the-middle |

Don't use ableist metaphors: "crazy," "insane," "blind to," "crippled," "lame," "dumb." Describe the actual condition: "unexpected," "unmonitored," "degraded," "basic."

Don't assume gender in examples. Use "they" for generic singular, or rewrite to second person ("you") or plural.

---

## Accessibility

### Images

- **Informative images**: Alt text describes content and purpose, not appearance. Under 100 characters. Start with the most important information.
- **Functional images** (inside links/buttons): Alt text describes the action. "Submit form" not "green arrow icon."
- **Decorative images**: Empty `alt=""` so screen readers skip them.
- Never start with "Image of..." or "Photo of..." — screen readers already announce it as an image.
- Don't present new information only in images.

### Reading Level

Target Flesch-Kincaid Grade 7-8 for prose, excluding technical terms and code. Complex concepts can be explained in simple sentences. Long words and nested clauses don't signal expertise — they signal unclear thinking.

### Color and Emphasis

Don't use color alone to convey meaning. Don't reference styling: "the red button," "the text in bold." Name the element.

---

## Document Types

### READMEs

- What it is (one sentence)
- What it does (brief)
- How to use it (quick start)
- How to configure it (reference)
- How to contribute (if applicable)

No history, no motivation essays, no badges that aren't useful.

### Specs and Design Docs

- Problem statement (what and why)
- Proposed solution (how)
- Alternatives considered (what else and why not)
- Acceptance criteria (how we know it's done)
- Open questions (what we haven't decided)

### PR Descriptions

- What changed (not how, the diff shows how)
- Why it changed
- How to test / verify
- Breaking changes (if any)

### Commit Messages

Per git workflow standards: conventional commit format, present tense, imperative mood. The first line is the entire message for most commits. Body only when the "why" isn't obvious from the diff.

### Error Messages

Every error message answers two questions: **What went wrong?** and **How does the user fix it?**

Structure (from the Rust compiler model):

| Component | Purpose | Example |
|---|---|---|
| **Primary message** | What failed | "Failed to connect to database at localhost:5432" |
| **Cause** | Why, if known | "Connection refused" |
| **Help** | Actionable fix (things the user can change) | "Check that PostgreSQL is running and DATABASE_URL is correct in config.yaml" |
| **Note** | Context that aids understanding (not actionable) | "The server was reachable 30 seconds ago" |

Rules:
- No stack traces in user-facing messages. Log them at debug level.
- No jargon the user can't act on. "ECONNREFUSED" helps a developer; "connection refused" helps everyone.
- Don't blame the user. "Invalid input" becomes "Expected a number between 1 and 100."
- Don't use "please" — it pads without adding information.
- Messages start lowercase when embedded in structured diagnostics. Sentence case in standalone contexts.

### Changelogs

Follow [Keep a Changelog](https://keepachangelog.com/) format with six categories:

| Category | When |
|---|---|
| **Added** | New features |
| **Changed** | Modifications to existing features |
| **Deprecated** | Features marked for future removal |
| **Removed** | Features removed |
| **Fixed** | Bug fixes |
| **Security** | Vulnerability patches |

Rules:
- Latest version first. ISO 8601 dates (YYYY-MM-DD).
- Maintain an `[Unreleased]` section at the top.
- Each entry: one line, answers "what changed and why."
- Don't dump the commit log. A changelog is curated — it tells users what matters to them, not what was easy to auto-generate.

### Code Examples

- **Contextually complete.** Show the real scenario end-to-end: imports, setup, the operation, error handling, expected output. A syntactically valid snippet that doesn't work when pasted is worse than no example.
- Introduce with a sentence ending in a colon.
- Wrap at 80 characters.

---

## Anti-Patterns

### Writing Mistakes

1. **Throat-clearing opener** — "In this document, we will explore the various aspects of..." Just start.
2. **Hedge stacking** — "It might arguably be somewhat important to perhaps consider..." State the claim.
3. **Paragraph where a table works** — comparing three options in prose when a 3-row table says it in a glance.
4. **Wall of text** — missing headers, no structure, no visual breaks. Reader's eyes glaze at paragraph four.
5. **Explaining what the reader can see** — "The table below shows..." followed by a table. The table shows itself.
6. **Writing for word count** — repeating the same point in different words across consecutive sentences.
7. **Burying the lead** — three paragraphs of context before the actual point. Answer first.
8. **Passive voice chains** — "It was determined that the configuration should be updated by the team." Who determined? Who updates?
9. **Scare quotes for emphasis** — use **bold**. Quotes signal irony or distancing.
10. **Em dash abuse** — long sentences held together with dashes instead of broken into clear, short statements.

### Structural AI Tells

Beyond banned words, these structural patterns mark text as machine-generated:

11. **Uniform sentence length** — every sentence 15-20 words. Human writing varies from 3-word fragments to 40+ word explanations. Mix deliberately.
12. **Uniform paragraph length** — every paragraph the same size. Vary. A one-sentence paragraph hits harder after three long ones.
13. **Topic-sentence-every-paragraph** — AI opens every paragraph with a topic sentence. Human writing buries the point, starts with examples, uses fragments.
14. **Tricolon addiction** — "adjective, adjective, and adjective" in every other sentence. Once per page is a rhetorical device. Three times per page is a pattern.
15. **Elegant variation** — cycling through synonyms for the same thing ("the system," "the platform," "the solution," "the tool") instead of just repeating the name.
16. **Copula avoidance** — "serves as," "functions as," "stands as" when "is" works. Say "is."
17. **Bookend summaries** — concluding by restating everything that was just said. If the reader just read it, don't repeat it.
