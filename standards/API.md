# API design

> Standards for HTTP APIs, CLI interfaces, and inter-service communication. Applies to any interface a human or machine consumes.

---

## HTTP aPIs

### Naming

- Endpoints use `snake_case` nouns: `/api/v1/sessions`, `/api/v1/knowledge/facts`
- Actions use HTTP verbs, not URL verbs: `DELETE /sessions/{id}` not `POST /sessions/{id}/delete`
- Plurals for collections: `/sessions` not `/session`
- Singular for singletons: `/config` not `/configs`

### Response format

- All responses are JSON with `Content-Type: application/json`
- Field names use `snake_case`. No `camelCase` in wire format. Serde `rename_all` enforced.
- Consistent casing across ALL endpoints. If one endpoint returns `session_key`, none returns `sessionKey`.

### Error responses

Every error response includes:

```json
{
  "error": {
    "code": "session_not_found",
    "message": "Session 01ABC does not exist",
    "request_id": "01XYZ"
  }
}
```

- `code`: machine-readable, snake_case, stable across versions
- `message`: human-readable, may change between versions
- `request_id`: correlates with server logs

### Status codes

| Code | Meaning | When |
|------|---------|------|
| 200 | Success | GET, PUT |
| 201 | Created | POST that creates |
| 204 | No content | DELETE |
| 400 | Bad request | Invalid input |
| 401 | Unauthorized | Missing or invalid auth |
| 403 | Forbidden | Valid auth, insufficient permission |
| 404 | Not found | Resource doesn't exist |
| 409 | Conflict | Duplicate creation, idempotency conflict |
| 422 | Validation failed | Input parses but fails business rules |
| 429 | Rate limited | Include `Retry-After` header |
| 500 | Internal error | Generic message to client, full detail in logs |

### Pagination

Collections with unbounded size use cursor-based pagination:

```
GET /sessions?limit=50&after=01ABC
```

Response includes `has_more: true/false` and the cursor for the next page.

### Versioning

URL-based: `/api/v1/`, `/api/v2/`. No header-based versioning. Breaking changes get a new version. Additive changes (new fields, new endpoints) don't.

### Auth defaults

When auth is disabled (`mode: none`), disable all auth-dependent features (CSRF, per-user rate limiting). Don't create circular dependencies where an auth-free endpoint requires a token only obtainable via an authenticated endpoint.

---

## CLI interfaces

### Argument structure

```
binary [global-flags] subcommand [subcommand-flags] [positional-args]
```

Global flags (like `-r` for instance root) go before the subcommand. Subcommand flags go after.

### Help text

Every subcommand has `--help` with:
- One-line description
- Flag descriptions with defaults shown
- Environment variable alternatives noted
- Examples for non-obvious usage

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime error |
| 2 | Argument error |

### Error messages

Follow the error message standard in WRITING.md: what failed + how to fix it. Include the failing value and a suggestion.

```
Error: instance not found at /home/user/ergon/instance
  Use -r /path/to/instance or set ALETHEIA_ROOT.
  To create a new instance: aletheia init
```

---

## Inter-service communication

### SSE (Server-Sent events)

- Include event type in `event:` field
- Include keep-alive pings (every 30 seconds)
- Send a terminal event on completion (`message_complete`, `error`)
- Client disconnect cancels server-side work (don't waste compute)

### Idempotency

State-changing operations support `Idempotency-Key` header. Same key returns cached response. In-flight duplicate returns 409 Conflict.

---

## Framework patterns (from axum)

### Infallible handlers

HTTP handlers return `Response`, never `Result<Response, Error>`. All errors convert to responses via `IntoResponse`. The service layer never propagates errors.

### Extractor ordering

Split extractors into two categories:
- **Parts extractors**: run first, don't consume body (Path, Query, Headers, State)
- **Body extractors**: run last, consume body (Json, Form, Bytes)

Enforce ordering at compile time. A handler with body extraction before parts extraction is a type error.

### State projection

State is `Clone`. Handlers extract substates via `FromRef` trait, not monolithic state structs. Each handler takes only the state it needs.

### Middleware as tower layers

Don't implement custom middleware systems. Use tower::Layer and tower::Service. The ecosystem composes. Offer `from_fn()` helper for async middleware.

### Testing via service trait

Router implements `tower::Service<Request>`. Tests call `router.oneshot(request).await` directly. No test client, no HTTP server. Tests are async function calls.
