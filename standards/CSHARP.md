# C#

> Additive to STANDARDS.md. Read that first. Everything here is C#/.NET-specific.
>
> Target: .NET 10 LTS, C# 14. Mouseion media management backend.
>
> **Key decisions:** Dapper (no EF Core), DryIoc DI, System.Text.Json source gen, primary constructors, CancellationToken everywhere, Polly resilience, PascalCase constants.

---

## Toolchain

- **Framework:** .NET 10 LTS
- **Language:** C# 14
- **ORM:** None. **Dapper only.** No Entity Framework Core.
- **Build/validate:**
  ```bash
  dotnet build Mouseion.sln --configuration Release
  dotnet test --configuration Release --verbosity minimal
  dotnet format --verify-no-changes
 ```

---

## Naming

See STANDARDS.md § Naming for universal conventions. C# overrides noted below.

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `PascalCase.cs` | `AudiobookService.cs` |
| Interfaces | `IPascalCase` | `IMediaRepository` |
| Methods | `PascalCase` | `GetChaptersAsync`, `LoadConfig` |
| Properties | `PascalCase` | `TotalDuration`, `IsActive` |
| Private fields | `_camelCase` | `_repository`, `_cache` |
| Local variables | `camelCase` | `albumCount`, `isValid` |
| Constants | `PascalCase` (overrides UPPER_SNAKE) | `DefaultCacheTtl`, `MaxRetries` |
| Async methods | suffix `Async` | `FetchMetadataAsync`, `LoadAlbumAsync` |

---

## Type system

### Records for value types

```csharp
public record AlbumSummary(string Title, int TrackCount, TimeSpan Duration);
```

Use `record` for immutable data transfer. `record struct` for stack-allocated small types.

### Primary constructors

Use for DI injection and classes. Parameters are captured as needed, not as fields.

```csharp
public class AlbumService(IMediaRepository repository, ILogger<AlbumService> logger)
{
    public async Task<Album?> FindAsync(int id, CancellationToken ct)
    {
        logger.LogDebug("Loading album {Id}", id);
        return await repository.FindByIdAsync(id, ct);
    }
}
```

For properties that must be `readonly`, assign to an explicit `readonly` field. Primary constructor parameters are mutable captures.

### Collection expressions

```csharp
int[] ids = [1, 2, 3];
List<string> names = ["alice", "bob"];
int[] combined = [..first, ..second, 42];
```

Compiler generates optimal code (stack-allocated spans where possible). Use over `new[] { }` and `new List<T> { }`.

### `required` and `init` properties

```csharp
public class SessionConfig
{
    public required string Name { get; init; }
    public required int MaxTurns { get; init; }
    public TimeSpan Timeout { get; init; } = TimeSpan.FromSeconds(30);
}
```

`required` enforces initialization at compile time. Caveat: reflection-based deserialization does not enforce `required`: use source-generated `System.Text.Json` serialization for safety.

### `field` keyword (C# 14)

Custom property logic without declaring a backing field:

```csharp
public string Name
{
    get => field;
    set => field = value?.Trim() ?? throw new ArgumentNullException(nameof(value));
}
```

### Nullable reference types

Enabled project-wide. `string?` means nullable, `string` means non-null. No `!` null-forgiving operator without explanation.

### Pattern matching

```csharp
return result switch
{
    Success<Album> s => Ok(s.Value),
    Failure<Album> { Error: NotFoundError } => NotFound(),
    Failure<Album> f => Problem(f.Error.Message),
};
```

Use exhaustive `switch` expressions. The compiler warns on unhandled cases.

### Raw string literals

Use for embedded SQL, JSON, regex: any string with quotes or backslashes:

```csharp
const string sql = """
    select id, name, created_at
    from albums
    where artist_id = @ArtistId
    """;
```

Prefer over `@""` verbatim strings and escape sequences.

### `file`-Scoped types

```csharp
file class AlbumValidator { /* ... */ }
```

Visibility restricted to the declaring file. Primary use: source generators, file-local helpers.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

### Custom exception hierarchies

```csharp
public class AppException : Exception
{
    public AppException(string message, Exception? inner = null) : base(message, inner) { }
}

public class ConfigException : AppException { /* ... */ }
public class SessionException : AppException { /* ... */ }
```

### Rules

- `IResult` / `Results.Problem()` for API error responses
- Include correlation ID in error responses
- Custom exception types per domain area

---

## Async & concurrency

### Async all the way

- `CancellationToken` on **all** async method signatures: no exceptions
- Never `.GetAwaiter().GetResult()`: always async all the way down
- `Task.Run()` only for CPU-bound work, never for I/O

```csharp
public async Task<Album> GetAlbumAsync(int id, CancellationToken ct)
{
    return await _repository.FindByIdAsync(id, ct);
}
```

### `ConfigureAwait(false)`

**Not needed in ASP.NET Core application code**: `SynchronizationContext` is null since ASP.NET Core 1.0.

**Still use in shared libraries** that may run in WPF, WinForms, MAUI, or legacy ASP.NET contexts:

```csharp
// Library code consumed by non-ASP.NET hosts
public async Task<byte[]> ReadAsync(CancellationToken ct)
{
    return await _stream.ReadAsync(ct).ConfigureAwait(false);
}
```

### `IAsyncEnumerable<T>` for streaming

Return `IAsyncEnumerable<T>` from endpoints for streaming results. ASP.NET Core serializes elements as they arrive.

```csharp
public async IAsyncEnumerable<TrackSummary> StreamTracksAsync(
    int albumId,
    [EnumeratorCancellation] CancellationToken ct)
{
    await foreach (var track in _repository.GetTracksAsync(albumId, ct))
    {
        yield return new TrackSummary(track.Id, track.Title, track.Duration);
    }
}
```

Items materialized one at a time: no buffering of the full result set.

### `params` collections (C# 13+)

```csharp
// Span-based params avoids heap allocation for small argument lists
public void Log(params ReadOnlySpan<string> messages) { /* ... */ }
```

---

## Data access

### Dapper only

No Entity Framework Core. No ORM magic. Explicit SQL with type-safe mapping.

- Generic repository base with type-safe queries
- Parameterized queries always: never string interpolation in SQL
- `CommandDefinition` with `CancellationToken` for cancellable queries
- Transaction scope for multi-statement operations

```csharp
public async Task<MediaItem?> FindByIdAsync(int id, CancellationToken ct)
{
    const string sql = """
        select * from media_items where id = @Id
        """;
    return await _connection.QueryFirstOrDefaultAsync<MediaItem>(
        new CommandDefinition(sql, new { Id = id }, cancellationToken: ct));
}
```

---

## Serialization

### System.Text.Json with source generation

Mandate source generation for AOT, trimmed, and high-performance scenarios. Eliminates reflection cost.

```csharp
[JsonSerializable(typeof(AlbumSummary))]
[JsonSerializable(typeof(List<TrackSummary>))]
internal partial class AppJsonContext : JsonSerializerContext;
```

Set `JsonSerializerIsReflectionEnabledByDefault` to `false` in `.csproj` to prevent accidental reflection fallback.

---

## Dependency injection

**DryIoc** container.

- Constructor injection only: no service locator pattern
- Primary constructors for DI (see Type System section)
- Interface-based registration for testability
- Scoped lifetime for request-bound services
- Singleton for stateless services and caches

---

## Resilience

**Polly** for external service calls.

- Retry with exponential backoff for transient failures
- Circuit breaker for cascading failure protection
- Timeout policies on all external HTTP calls
- Combine policies in a `PolicyWrap`

---

## Caching

### IMemoryCache for hot-path data

Use `IMemoryCache` for metadata and reference data that is read far more often than written. 15-minute default TTL.

```csharp
public class GenreCache(IMemoryCache cache, IMediaRepository repository, ILogger<GenreCache> logger)
{
    private static readonly TimeSpan DefaultTtl = TimeSpan.FromMinutes(15);

    public async Task<IReadOnlyList<Genre>> GetGenresAsync(CancellationToken ct)
    {
        // WHY: GetOrCreateAsync is atomic — only one factory runs even under concurrent requests.
        // Prevents cache stampede where N requests all miss and all hit the database.
        return await cache.GetOrCreateAsync("genres:all", async entry =>
        {
            entry.AbsoluteExpirationRelativeToNow = DefaultTtl;
            logger.LogDebug("Cache miss for genres, loading from database");
            return await repository.ListGenresAsync(ct);
        }) ?? [];
    }

    public void InvalidateGenres()
    {
        cache.Remove("genres:all");
    }
}
```

Rules:
- **Always use `GetOrCreateAsync`**, not separate get-then-set. WHY: separate get/set has a race window where multiple threads miss and all execute the factory.
- **Absolute expiration, not sliding.** WHY: sliding expiration keeps stale data alive indefinitely under steady traffic. Absolute forces periodic refresh.
- **Invalidate on mutation.** Every write operation that changes cached data must call `Remove` on the relevant keys. Stale reads after a write are bugs, not acceptable latency.
- **Cache keys: deterministic, include all varying parameters.** Format: `{entity}:{qualifier}:{id}`. Example: `albums:artist:42`.

### IDistributedCache for shared state

Use `IDistributedCache` when cache must be shared across multiple app instances or survive process restarts. Backed by Redis or similar.

```csharp
public class SessionCache(IDistributedCache cache, ILogger<SessionCache> logger)
{
    private static readonly DistributedCacheEntryOptions Options = new()
    {
        AbsoluteExpirationRelativeToNow = TimeSpan.FromMinutes(30),
    };

    public async Task<UserSession?> GetSessionAsync(string sessionId, CancellationToken ct)
    {
        byte[]? data = await cache.GetAsync($"session:{sessionId}", ct);
        if (data is null) return null;

        // WHY: Source-generated deserialization for AOT safety and performance.
        return JsonSerializer.Deserialize(data, AppJsonContext.Default.UserSession);
    }

    public async Task SetSessionAsync(string sessionId, UserSession session, CancellationToken ct)
    {
        byte[] data = JsonSerializer.SerializeToUtf8Bytes(session, AppJsonContext.Default.UserSession);
        await cache.SetAsync($"session:{sessionId}", data, Options, ct);
    }
}
```

Rules:
- **Serialize with `System.Text.Json` source generation.** WHY: `IDistributedCache` stores `byte[]`, not objects. Reflection-based serialization breaks AOT and is slower.
- **Never store large objects (>64KB).** WHY: distributed cache is a network hop. Large values saturate bandwidth and increase latency for all consumers.
- **Set explicit TTL on every entry.** No indefinite entries in distributed cache. Orphaned keys consume memory on the backing store with no eviction pressure.

### FrozenDictionary / FrozenSet for static data

`FrozenDictionary<K,V>` / `FrozenSet<T>` for lookup data built once at startup. ~50% faster reads than `Dictionary`, thread-safe by nature (immutable).

```csharp
public class CodecRegistry
{
    // WHY: Built once at startup, never mutated. FrozenDictionary optimizes
    // its internal layout for the actual keys, yielding faster lookups.
    private readonly FrozenDictionary<string, CodecInfo> _codecs;

    public CodecRegistry(IEnumerable<CodecInfo> codecs)
    {
        _codecs = codecs.ToFrozenDictionary(c => c.Name, StringComparer.OrdinalIgnoreCase);
    }

    public CodecInfo? Find(string name) =>
        _codecs.GetValueOrDefault(name);
}
```

- Never cache user-specific data in shared memory cache. WHY: memory cache is process-wide. User A sees User B's data on the next request routed to the same instance.

---

## Activity and context propagation

### Distributed tracing with Activity

`System.Diagnostics.Activity` is the .NET standard for distributed tracing. It integrates with OpenTelemetry, ASP.NET Core, and HttpClient without third-party libraries.

```csharp
public static class Telemetry
{
    // WHY: ActivitySource is the .NET equivalent of a tracer. One per logical component.
    // The name must match the OpenTelemetry SDK's AddSource() registration.
    public static readonly ActivitySource Source = new("Mouseion.Media");
}

public class AlbumService(IMediaRepository repository, ILogger<AlbumService> logger)
{
    public async Task<Album?> FindAsync(int id, CancellationToken ct)
    {
        // WHY: StartActivity returns null if no listener is registered,
        // so this is zero-cost when tracing is not configured.
        using var activity = Telemetry.Source.StartActivity("AlbumService.Find");
        activity?.SetTag("album.id", id);

        var album = await repository.FindByIdAsync(id, ct);

        activity?.SetTag("album.found", album is not null);
        return album;
    }
}
```

### Propagation rules

- **One `ActivitySource` per logical component.** Register in a static field. WHY: `ActivitySource` is the unit of subscription in OpenTelemetry. Granular sources let operators enable tracing per-component without noise.
- **`using` on every `StartActivity` call.** WHY: `Activity` implements `IDisposable`. Missing `using` means the span never closes, corrupting trace timelines.
- **Set tags for query parameters, entity IDs, and outcomes.** These are the fields you search in Jaeger/Tempo. A span without tags is a span you cannot filter.
- **Propagate `Activity.Current` across async boundaries automatically.** ASP.NET Core and `HttpClient` do this by default. Do not manually set `Activity.Current` unless you are spawning a detached background task.

### Correlation IDs for non-traced paths

When full tracing is not active, propagate a correlation ID via `HttpContext.TraceIdentifier`:

```csharp
// WHY: TraceIdentifier is set automatically by ASP.NET Core from
// the W3C traceparent header or a generated value. Use it as the
// correlation ID in logs and error responses — no custom header needed.
logger.LogError("Failed to load album {AlbumId}, trace {TraceId}",
    id, httpContext.TraceIdentifier);
```

---

## Bulk operations

### Dapper bulk patterns

Individual queries in a loop are the primary performance trap with Dapper. Batch operations reduce round-trips.

#### Multi-row insert with Dapper

```csharp
public async Task InsertTracksAsync(IReadOnlyList<Track> tracks, CancellationToken ct)
{
    // WHY: Single statement with multiple value tuples. One round-trip
    // regardless of collection size. Dapper maps the list to parameters.
    const string sql = """
        insert into tracks (album_id, title, duration_ms, track_number)
        values (@AlbumId, @Title, @DurationMs, @TrackNumber)
        """;

    // WHY: Wrap in transaction so a partial failure doesn't leave orphaned rows.
    using var transaction = _connection.BeginTransaction();
    await _connection.ExecuteAsync(
        new CommandDefinition(sql, tracks, transaction: transaction, cancellationToken: ct));
    transaction.Commit();
}
```

#### Bulk read with `WHERE IN`

```csharp
public async Task<IReadOnlyList<Album>> GetByIdsAsync(IReadOnlyList<int> ids, CancellationToken ct)
{
    if (ids.Count == 0) return [];

    // WHY: Dapper expands @Ids into a parameterized IN clause automatically.
    // No string interpolation, no SQL injection risk.
    const string sql = """
        select * from albums where id in @Ids
        """;

    var results = await _connection.QueryAsync<Album>(
        new CommandDefinition(sql, new { Ids = ids }, cancellationToken: ct));
    return results.AsList();
}
```

### Rules

- **Never query in a loop.** WHY: N+1 queries dominate latency. A loop of 100 individual SELECTs is 100 round-trips; a single `WHERE IN` is one.
- **Batch size limit: 1000 parameters per statement.** WHY: SQLite and some PostgreSQL configurations have parameter count limits. For larger sets, chunk into batches of 1000 and wrap in a single transaction.
- **Always wrap multi-statement bulk writes in a transaction.** WHY: without a transaction, a failure mid-batch leaves the database in an inconsistent state with partial writes.
- **Return affected row count from bulk mutations** so callers can verify expectations:

```csharp
public async Task<int> DeactivateAlbumsAsync(IReadOnlyList<int> albumIds, CancellationToken ct)
{
    const string sql = """
        update albums set is_active = false where id in @Ids
        """;
    return await _connection.ExecuteAsync(
        new CommandDefinition(sql, new { Ids = albumIds }, cancellationToken: ct));
}
```

---

## Middleware pipeline

### Ordering

Middleware order determines behavior. The ASP.NET Core pipeline is a Russian doll: each middleware wraps the next. Order is not arbitrary.

```csharp
var app = builder.Build();

// WHY: Exception handler must be outermost so it catches errors from all inner middleware.
app.UseExceptionHandler();

// WHY: Request logging wraps the full pipeline to capture timing and status for every request.
app.UseRequestLogging();

// WHY: CORS must run before auth so preflight OPTIONS requests are handled
// without requiring credentials.
app.UseCors();

app.UseAuthentication();
app.UseAuthorization();

// WHY: Rate limiting after auth so authenticated users get their own limits
// and anonymous abuse is caught by a separate, stricter policy.
app.UseRateLimiter();

app.MapControllers();
app.MapHealthChecks("/health");
```

### Custom middleware pattern

Write middleware as a class, not an inline lambda. WHY: class-based middleware is testable, injectable, and visible in stack traces.

```csharp
public class CorrelationIdMiddleware(RequestDelegate next)
{
    // WHY: Ensures every request has a correlation ID for log correlation,
    // whether the client sent one or not. Downstream services receive it
    // via HttpClient's default headers.
    public async Task InvokeAsync(HttpContext context)
    {
        const string headerName = "X-Correlation-Id";

        if (!context.Request.Headers.TryGetValue(headerName, out var correlationId)
            || StringValues.IsNullOrEmpty(correlationId))
        {
            correlationId = context.TraceIdentifier;
        }

        context.Response.Headers[headerName] = correlationId.ToString();

        using (logger.BeginScope(new Dictionary<string, object>
        {
            ["CorrelationId"] = correlationId.ToString()!,
        }))
        {
            await next(context);
        }
    }
}
```

### Request logging middleware

Log every request with structured fields. This is the observability contract for the API boundary.

```csharp
public class RequestLoggingMiddleware(RequestDelegate next, ILogger<RequestLoggingMiddleware> logger)
{
    public async Task InvokeAsync(HttpContext context)
    {
        var stopwatch = Stopwatch.StartNew();

        try
        {
            await next(context);
        }
        finally
        {
            stopwatch.Stop();
            logger.LogInformation(
                "HTTP {Method} {Path} responded {StatusCode} in {ElapsedMs}ms",
                context.Request.Method,
                context.Request.Path,
                context.Response.StatusCode,
                stopwatch.ElapsedMilliseconds);
        }
    }
}
```

### Rules

- **Exception handling middleware is always first (outermost).** WHY: any middleware before it can throw unhandled exceptions that bypass your error response format.
- **Never use inline `app.Use(async (ctx, next) => ...)` for anything beyond trivial one-liners.** WHY: lambdas are untestable, invisible in stack traces, and accumulate into an unreadable `Program.cs`.
- **Health check endpoints bypass auth and rate limiting.** Map them after `UseAuthorization` but configure the endpoint to allow anonymous. WHY: monitoring probes must not fail because of expired tokens or rate limits.
- **Each middleware does one thing.** Correlation ID injection is not also request logging. WHY: composability. Operators must be able to add, remove, or reorder individual concerns without side effects.

---

## Validation

**FluentValidation** for request DTOs.

- One validator class per request type
- Validate at the API boundary, not in business logic
- Structured error responses with field-level details

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

C#-specific framework choices:

- **Framework:** xUnit (or NUnit)
- **Mocking:** NSubstitute (or Moq) at interface boundaries
- **Integration tests:** `WebApplicationFactory<T>` for API tests

---

## Architecture

```
src/Project.Core/    : entities, services, business logic
src/Project.Api/     : controllers/endpoints, middleware, API surface
src/Project.Common/  : shared utilities, HTTP client, DI
src/Project.Host/    : entry point, configuration
tests/               : unit and integration tests
```

- Core has no dependency on Api or Host
- Api depends on Core, never the reverse
- Common is a leaf: imported by all, imports nothing project-specific
- Minimal APIs for focused endpoints (health checks, webhooks). Controllers for complex modules.

---

## Anti-Patterns

1. **Entity Framework Core**: we use Dapper. No ORM magic.
2. **`.GetAwaiter().GetResult()`**: deadlock risk. Async all the way.
3. **Missing `CancellationToken`**: every async method signature
4. **Service locator**: constructor injection only
5. **`dynamic` or untyped `var`**: explicit types when non-obvious
6. **Hardcoded connection strings**: configuration injection
7. **`string.Format` over interpolation**: use `$"..."` syntax
8. **Bare `catch (Exception)`**: see STANDARDS.md § No Silent Catch
9. **`null!` without explanation**: fix the nullability, don't suppress it
10. **`ConfigureAwait(false)` in ASP.NET Core app code**: unnecessary noise, no `SynchronizationContext` exists
11. **Reflection-based JSON serialization in AOT/trimmed builds**: use `System.Text.Json` source generation
12. **`new[] { }` / `new List<T> { }`**: use collection expressions: `[1, 2, 3]`
13. **Querying in a loop**: batch with `WHERE IN` or bulk insert. N+1 kills latency.
14. **Separate get-then-set on `IMemoryCache`**: use `GetOrCreateAsync` to avoid cache stampede.
15. **Sliding cache expiration under steady traffic**: stale data never expires. Use absolute expiration.
16. **Missing `using` on `StartActivity`**: unclosed spans corrupt trace timelines.
17. **Inline `app.Use()` lambdas for non-trivial middleware**: untestable, invisible in stack traces. Use a middleware class.
18. **Middleware before `UseExceptionHandler`**: unhandled exceptions bypass your error response format.
