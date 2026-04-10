# Prompting

> Standards for constructing API prompts: system messages, user messages, tool-use instructions, and evaluation prompts sent to LLM providers. Based on Anthropic's documented best practices for Claude 4.x models.

**Scope:** API-level prompt construction -- the text sent as system prompts, user messages, and tool calls to LLM APIs. Does not cover repository context files (CLAUDE.md, AGENTS.md, llms.txt); see [AGENT-DOCS.md](AGENT-DOCS.md) for those.

---

## Structure

### System prompt ordering

Static content first, variable content last. This maximizes prompt cache hits (cache prefix matching requires identical leading tokens).

```
1. Behavioral directives (1-2 sentences — what to do, not who to be)
2. Rules and constraints (XML-wrapped)
3. Standards and rules
4. Validation gate
5. Project context (variable per project)
6. Contextual injections (lessons, observations, warm context)
```

### User prompt ordering

Task description goes at the bottom of the prompt body, not the top. Anthropic testing shows up to 30% quality improvement when the query appears after all context.

```
1. Setup instructions
2. Context (prior work, related observations, handoff docs)
3. Acceptance criteria
4. Blast radius
5. Task / directive (LAST)
```

---

## XML tags

Use XML tags to separate instructions from data. Claude processes XML boundaries as structural markers -- content inside tags is less likely to be misinterpreted as instructions, and instructions inside tags are less likely to be treated as data.

### When to use XML tags

- Wrapping behavioral directive blocks in system prompts
- Separating few-shot examples from instructions
- Structuring output format specifications
- Isolating code/diff content from evaluation instructions

### Tag naming

Tag names should describe their content. No canonical "best" names -- use what makes sense.

```xml
<behavioral_directives>
Evaluate code changes against the acceptance criteria.
Ground every judgment in specific code from the diff.
Evaluate each criterion independently.
</behavioral_directives>

<examples>
<example>
Input: ...
Output: ...
</example>
</examples>

<output_format>
Respond with this JSON structure: ...
</output_format>
```

### When NOT to use XML tags

- Simple single-purpose prompts (overhead > value)
- Inside few-shot examples (examples should mirror actual expected I/O)
- For formatting that markdown handles well (tables, lists, headers)

---

## Voice

### Positive instructions over negative

Tell Claude what to DO, not what NOT to do. Claude 4.x models overtrigger on negative framing -- "Do NOT" causes excessive avoidance.

Wrong:
```
Do NOT modify files outside the blast radius.
NEVER use unwrap() in library code.
```

Right:
```
Stay within the blast radius because files outside scope create merge conflicts that block other agents.
Use the ? operator with snafu context in library code because unwrap panics crash the server without diagnostic information.
```

### WHY on every rule

Every rule must explain why it exists. Claude generalizes from the explanation and applies the principle to novel situations. Rules without WHY are followed rigidly but not understood.

Wrong:
```
Use #[expect] over #[allow].
```

Right:
```
Use #[expect(lint, reason = "...")] over #[allow] because #[expect] fires a compiler warning when the suppressed lint is resolved, preventing stale suppressions from accumulating.
```

### Dial back aggressive language

Claude 4.x overtriggers on MUST, CRITICAL, ALWAYS, NEVER. Use direct statements without escalation.

Wrong:
```
CRITICAL: You MUST use this tool when searching for files.
ALWAYS check the blast radius before modifying code.
```

Right:
```
Use this tool when searching for files.
Check the blast radius before modifying code.
```

Reserve emphasis for genuinely critical safety constraints (credential handling, destructive operations).

---

## Few-shot examples

### Placement

Examples go in an `<examples>` block after instructions and before the task. 3-5 examples is optimal. Each example wraps in `<example>` tags.

### Quality over quantity

Examples must be relevant (mirror actual use cases), diverse (cover edge cases), and include both input and expected output. Diverse canonical examples beat exhaustive edge-case lists.

### Reasoning in examples

Include `<thinking>` tags inside examples to demonstrate reasoning patterns. Claude generalizes the reasoning style to its own extended thinking.

```xml
<examples>
<example>
Input: PR diff adds a new HTTP endpoint without input validation
<thinking>
The endpoint accepts user input via query parameters but does not validate length or content.
SECURITY.md requires boundary validation on all HTTP endpoints.
The diff shows no size limit check.
</thinking>
Output:
- CRITERION: Input validation at boundary
- VERDICT: FAIL
- EVIDENCE: handler at line 45 reads query param without length check
</example>
</examples>
```

---

## Structured output

### JSON for machine parsing

When output must be parsed programmatically, specify a JSON schema in the prompt. Include the exact structure with field types.

```xml
<output_format>
Respond with this JSON:
{
  "evaluations": [
    {
      "criterion_id": 1,
      "verdict": "PASS" | "FAIL",
      "confidence": "high" | "medium" | "low",
      "evidence": "specific code reference",
      "reasoning": "brief chain of thought"
    }
  ]
}
</output_format>
```

### Escape hatches

Include escape hatches for ambiguous cases. "Return Unknown when insufficient information" prevents forced judgments.

### ID-based matching

When evaluating a list (criteria, requirements, items), assign numeric IDs and reference by ID in the output. Avoids fuzzy string matching failures.

---

## Chain of thought

### Adaptive over prescriptive

General instructions ("think thoroughly about edge cases") produce better reasoning than prescriptive step-by-step plans. Claude finds valid approaches that prompt designers don't anticipate.

### Self-verification

Add a self-check instruction at the end: "verify your answer against the acceptance criteria before responding." This catches errors reliably for coding and evaluation tasks.

### Think tool

For multi-step tool use sequences, the think tool (mid-response reasoning) outperforms front-loaded chain-of-thought. Use when the agent needs to reason between tool calls.

---

## Prompt caching

### Cache prefix ordering

Claude's prompt cache matches on leading token prefixes. The order matters:

```
1. Tools (most static)
2. System prompt (static per role)
3. Messages (variable per session)
```

Changes at any level invalidate that level and all subsequent levels.

### Breakpoint placement

Place cache breakpoints on the last identical block across requests. Common mistake: placing breakpoints on changing content (timestamps, session IDs) -- the hash never matches.

### Minimum tokens

Minimum cacheable prefix: 1,024 tokens for Sonnet 4.5/4, 2,048 for Sonnet 4.6, 4,096 for Opus/Haiku 4.5. Cache reads cost 0.1x base input (90% savings).

---

## Evaluation prompts

### Isolated judgment

Grade each dimension with a separate evaluation, not one judge for all dimensions. Multi-dimensional evaluation in a single prompt produces less consistent results.

### Grade outcomes, not paths

Agents regularly find valid approaches that evaluators didn't anticipate. Grade whether acceptance criteria are met, not whether the approach matches expectations.

### Calibration

Test evaluation prompts against known-good and known-bad examples before deployment. Ensure pass rate on known-good is >95% and fail rate on known-bad is >90%.

---

## Role framing

### Directive hierarchy

Three independent studies (2024-2026) show measurable differences between role-framing strategies. Ranked by coding and factual-recall accuracy:

| Tier | Strategy | Example | Finding |
|------|----------|---------|---------|
| 1 (best) | Directive-only | "Analyze this code for security issues" | Highest accuracy on coding and factual tasks |
| 2 | Behavioral + capability | "You are an expert security analyst. Analyze this code for security issues" | Middle tier -- helps alignment tasks, slight cost on factual recall |
| 3 (weakest) | Identity claim | "You are a senior engineer at Google. Analyze this code for security issues" | Degrades coding accuracy and factual recall; only helps alignment-heavy tasks |

Use Tier 1 by default. Use Tier 2 when the task requires domain-specific judgment framing. Avoid Tier 3 -- the identity claim adds no information the model can act on and measurably hurts performance.

---

## Anti-patterns

- Prose-dump system prompts without structural markers (use XML tags)
- Negative-voice rule lists ("Do NOT" x20) without WHY explanations
- Aggressive emphasis (MUST, CRITICAL, ALWAYS) on non-safety rules
- Task at the top of the prompt body (put it last)
- Fuzzy string matching on structured output (use ID-based matching)
- Prescriptive step-by-step plans where general guidance suffices
- Placing variable content before static content (kills cache hits)
- Evaluating multiple dimensions in a single prompt
- Examples that don't match actual use cases
- Rules without WHY (followed rigidly, not understood)
- Identity claims in role definitions (see [Role framing](#role-framing) -- use directive-only instead)
