# Protocol Buffers

> Standards for proto3 schema design, wire compatibility, gRPC service patterns, JSON mapping, and Rust codegen. Applies to all `.proto` files and generated code.

---

## File organization

- One file per service. Service file named after the service in `lower_snake_case.proto`.
- Supporting messages and enums shared across services go in dedicated files grouped by domain.
- File order: syntax, package, imports (sorted alphabetically), file options (sorted), definitions.
- Directory structure mirrors package path: `package foo.bar.v1;` lives in `foo/bar/v1/`.
- Keep files under 500 lines. Split large domains into multiple files within the same package.

## Package naming

- Dot-delimited `lower_snake_case`: `meshtastic`, `akroasis.kerykeion.v1`.
- No uppercase letters, no emphasizes within segments, no Java-style `com.` prefixes.
- Stable packages end with a version suffix: `v1`, `v2`. Pre-release: `v1alpha1`, `v1beta2`.
- Stable packages never import unstable packages.

## Syntax

- All files declare `syntax = "proto3";` as the first non-comment line.

## Naming conventions

### Messages

- `PascalCase`: `MeshPacket`, `DeviceMetrics`, `ChannelSettings`.
- Request/response types: `{MethodName}Request` / `{MethodName}Response`.
- Treat abbreviations as single words: `GetDnsRequest` not `GetDNSRequest`.
- Never start or end a name with emphasize.

### Fields

- `snake_case`: `battery_level`, `channel_utilization`, `rx_snr`.
- Repeated fields use plural names: `repeated uint32 route = 2;`.
- Boolean fields use `is_` or `has_` prefix when it improves clarity: `is_licensed`, `has_default_channel`.
- Avoid language keywords (`type`, `class`, `default`) as field names -- codegen renames them.

### Enums

- Type name: `PascalCase`: `HardwareModel`, `PortNum`.
- Values: `UPPER_SNAKE_CASE` prefixed with the enum type name: `HARDWARE_MODEL_UNSET`, `PORT_NUM_UNKNOWN_APP`.
- Prefix prevents C++ global scope collisions and clarifies meaning in languages without enum scoping.
- Exception: project-wide enums where the prefix would be redundant and values are unambiguous may omit the prefix if every consumer imports only that enum.

### Services and rPCs

- Service name: `PascalCase`, suffixed with `Service`: `TelemetryService`, `AdminService`.
- RPC methods: `PascalCase` using `VerbNoun` pattern: `GetChannel`, `ListNodes`, `CreateSession`.

### Oneofs

- `snake_case`: `payload_variant`, `config_type`.
- Consistent naming within a project -- pick one convention and use it everywhere.

## Field numbering

- Numbers 1-15 encode as one byte on the wire. Reserve these for frequently-set fields.
- Numbers 16-2047 encode as two bytes. Use for less common fields.
- Never reuse a field number, even after deleting the field. Serialized data in logs, caches, and storage may still reference old numbers.
- Reserve deleted field numbers AND names to prevent accidental reuse:

```protobuf
reserved 6, 9;
reserved "old_field", "deprecated_field";
```

- Number gaps are acceptable when fields have been removed and reserved. Document with a comment above the `reserved` block.
- Field numbers above 19000-19999 are reserved by the protobuf implementation. Never use them.

## Backwards compatibility

Protobuf's value is schema evolution. Breaking the wire format negates the reason to use it.

### Safe changes (additive only)

- Add new fields, enum values, messages, services, RPCs, oneofs.
- Rename fields (wire format uses numbers, not names -- but JSON encoding uses names, so avoid renaming in JSON-facing APIs).
- Add `reserved` entries for removed fields.

### Breaking changes (never within a major version)

- Remove or renumber existing fields.
- Change a field's type (even between wire-compatible types like `int32` → `int64` -- codegen types differ).
- Move a field into or out of a `oneof` (breaks Go stubs, loses data on other platforms).
- Change `repeated` to scalar or vice versa.
- Change the default value of a field.
- Remove an enum value that clients may have persisted.
- Delete a file or move a message to a different file (breaks per-file codegen).

### Deprecation

- Mark deprecated fields with `[deprecated = true]`.
- Add a comment explaining what replaces it.
- Reserve the field number and name when the field is finally removed.

### Oneof migration

`oneof` membership is a wire-level commitment. Moving a field into or out of a `oneof` is a breaking change -- it alters the wire encoding and breaks generated stubs (particularly in Go, where `oneof` fields become interface types).

**Adding a variant to an existing oneof** is safe:

```protobuf
// v1: two variants
oneof payload_variant {
  TextPayload text = 3;
  BinaryPayload binary = 4;
}

// v1 (evolved): third variant added -- safe, old clients ignore it
oneof payload_variant {
  TextPayload text = 3;
  BinaryPayload binary = 4;
  JsonPayload json = 5;
}
```

**Removing a variant**: never delete the field. Mark it `[deprecated = true]`, stop setting it in new code, and `reserved` the number when no clients remain:

```protobuf
oneof payload_variant {
  TextPayload text = 3;
  BinaryPayload binary = 4 [deprecated = true];
  JsonPayload json = 5;
}
// After all clients migrate away from binary:
// reserved 4;
// reserved "binary";
```

**Replacing a oneof entirely**: introduce a new `oneof` with a different name and new field numbers. Deprecate the old `oneof`'s fields. Never reuse the old field numbers. Handlers must check both oneofs during the transition window.

**Rust handling**: `prost` generates an `enum` for each `oneof`. New variants added by schema evolution will deserialize as `None` in old code (the field is simply unset from the old binary's perspective). Design match arms to handle the `None` / unknown case explicitly.

## Enum conventions

- First value is always zero and signals "unset": `FOO_UNSPECIFIED = 0` or `FOO_UNKNOWN = 0`.
- Every enum has a zero value. Proto3 uses zero as the default -- without an explicit unspecified value, the first real value becomes the ambiguous default.
- Assign values sequentially. Gaps only where values were removed and reserved.
- Never use `allow_alias` -- aliased values cause JSON serialization ambiguity.
- Never use negative enum values.
- Prefer enums over booleans when the domain may grow beyond two states.

## Service design

- Each RPC has unique request and response types. Never share `{Method}Request` across RPCs -- it couples their evolution.
- Use unary RPCs by default. Use streaming only when the use case demands it (real-time feeds, large transfers, bidirectional communication).
- Map domain errors to gRPC status codes consistently. Document the mapping.
- Standard CRUD methods: `Get`, `List`, `Create`, `Update`, `Delete`.
- Long-running operations return an `Operation` message with status polling.

## Pagination

Use cursor-based pagination by default. Offset-based pagination is fragile under concurrent writes (inserts shift offsets, causing skipped or duplicated items).

### Message pattern

```protobuf
message ListDevicesRequest {
  // Maximum number of items to return. Server may return fewer.
  // Must be positive. Server enforces an upper bound (e.g., 1000).
  int32 page_size = 1;

  // Opaque token from a previous ListDevicesResponse.
  // Empty string for the first page.
  string page_token = 2;

  // Optional filter or sort criteria.
  string filter = 3;
}

message ListDevicesResponse {
  repeated Device devices = 1;

  // Token for the next page. Empty string means no more pages.
  // Clients must not interpret or construct tokens -- they are opaque.
  string next_page_token = 2;

  // Optional: total count if the client needs it for UI pagination.
  // Omit if computing the total is expensive (requires a full scan).
  int32 total_size = 3;
}
```

### Rules

- `page_token` is an opaque string. The server encodes cursor state (e.g., last-seen ID, sort key) into it. Clients must never parse, construct, or modify tokens.
- `page_size` is a request, not a guarantee. The server may return fewer items (final page, server-side cap). The client checks `next_page_token` emptiness to detect the last page, not the response count.
- Servers must enforce a maximum `page_size`. Unbounded page sizes defeat the purpose of pagination.
- Tokens must be stable across schema changes. Encode by value (last-seen sort key + ID), not by position. A token that embeds an offset breaks when rows are inserted or deleted.
- Tokens should expire after a reasonable TTL (e.g., 24 hours). Document the expiry. Expired tokens return `INVALID_ARGUMENT`, not stale data.
- `List` RPCs that support pagination must also work without it: if `page_token` is empty and `page_size` is zero, return the first page with the server's default size.

### Offset-based pagination

Use only when the client genuinely needs random page access (e.g., "jump to page 7") and the dataset is append-only or rarely mutated. The pattern uses `offset` and `limit` fields instead of `page_token`:

```protobuf
message ListAuditLogsRequest {
  int32 limit = 1;
  int32 offset = 2;
}
```

Offset pagination must never be the default. Document the consistency caveats when it is used.

## gRPC patterns

### Streaming

Choose the streaming mode that matches the data flow. Mismatched modes leak complexity into both client and server.

| Mode | When | Example |
|------|------|---------|
| Unary | Single request, single response | `GetChannel`, `CreateSession` |
| Server streaming | Single request, multiple responses over time | `ListNodes` with large result sets, event subscriptions |
| Client streaming | Multiple requests, single response | Batch upload, aggregated metrics |
| Bidirectional streaming | Both sides send independently | Real-time chat, collaborative editing |

- Server-streaming RPCs must define a clear end condition. The server closes the stream; the client must not assume it stays open forever.
- Client-streaming RPCs must define a maximum message count or total size. Unbounded client streams invite resource exhaustion on the server.
- Bidirectional streams must handle half-close: either side can finish sending while the other continues. Design the protocol so both sides know when the conversation is over.

```protobuf
service TelemetryService {
  // Unary: single device lookup.
  rpc GetDevice(GetDeviceRequest) returns (GetDeviceResponse);

  // Server streaming: subscribe to metrics as they arrive.
  // Stream closes when the device goes offline or the client cancels.
  rpc WatchMetrics(WatchMetricsRequest) returns (stream DeviceMetrics);

  // Client streaming: batch-upload recorded positions.
  // Server responds with a summary after the client closes the stream.
  rpc UploadPositions(stream Position) returns (UploadPositionsResponse);
}
```

### Backpressure and flow control

gRPC uses HTTP/2 flow control at the transport layer, but application-level backpressure is the developer's responsibility.

- **Server streaming**: The server must respect client consumption rate. If the client is slow, buffered messages consume server memory. Bound the send buffer and apply backpressure by blocking the send when the buffer is full rather than dropping messages silently.
- **Client streaming**: The server should enforce a maximum message count or total payload size. Reject streams that exceed the limit with `RESOURCE_EXHAUSTED`.
- **Bidirectional streaming**: Both sides must handle slow consumers. Design protocols with explicit acknowledgment or windowing so neither side races ahead unboundedly.
- Set `max_concurrent_streams` on the server to limit how many RPCs a single HTTP/2 connection can multiplex simultaneously. The default is often unlimited, which invites resource exhaustion under load.

### Keepalive configuration

Keepalives detect dead connections that the OS TCP stack has not yet noticed (NAT timeouts, silent intermediary failures).

- **Server-side**: Enable keepalive pings to detect clients that have silently disconnected. Set `keepalive_interval` (how often to ping idle connections) and `keepalive_timeout` (how long to wait for a pong before closing).
- **Client-side**: Enable keepalive pings to detect unresponsive servers before the OS TCP timeout (which can exceed 10 minutes).
- Set `permit_keepalive_without_calls` to `true` on the server if clients maintain long-lived idle connections (e.g., watch streams).
- Coordinate keepalive intervals: the client's interval must not be shorter than the server's `min_recv_ping_interval_without_data`, or the server will terminate the connection with `ENHANCE_YOUR_CALM`.

```rust
// Server keepalive configuration
Server::builder()
    .http2_keepalive_interval(Some(std::time::Duration::from_secs(60)))
    .http2_keepalive_timeout(Some(std::time::Duration::from_secs(20)))
    .add_service(service)
    .serve(addr)
    .await?;

// Client keepalive configuration
let channel = Channel::from_shared(endpoint)?
    .keep_alive_while_idle(true)
    .http2_keep_alive_interval(std::time::Duration::from_secs(60))
    .keep_alive_timeout(std::time::Duration::from_secs(20))
    .connect()
    .await?;
```

### Deadlines and timeouts

- Every RPC call must set a deadline. Calls without deadlines can hang indefinitely, consuming server resources and blocking client threads.
- Propagate deadlines across service boundaries. If service A calls service B within a request, B's deadline must not exceed A's remaining time.
- Servers must check deadline expiry before starting expensive work. Return `DEADLINE_EXCEEDED` early rather than completing work the client has already abandoned.

### Health checking

- Every gRPC service exposes the standard `grpc.health.v1.Health` service. Load balancers and orchestrators depend on it for routing decisions.
- `Check` returns `SERVING`, `NOT_SERVING`, or `UNKNOWN`. Never return `SERVING` if the service cannot fulfil requests (e.g., database connection lost).
- `Watch` enables streaming health updates for clients that need push-based notifications.

### Reflection

- Enable gRPC server reflection in development and staging environments. It enables `grpcurl` and other diagnostic tools without maintaining separate proto files on the client side.
- Disable reflection in production unless the service is internal-only. Reflection exposes the full schema, which is an information disclosure risk for public APIs.

### Interceptors (middleware)

- Use interceptors for cross-cutting concerns: logging, metrics, authentication, deadline enforcement, request-id propagation.
- Order interceptors deliberately. Authentication must run before authorization. Logging must run before anything that might reject the request, so failures are visible.
- Never put business logic in interceptors. Interceptors handle infrastructure; handlers handle domain.

### Error model

Map domain errors to gRPC status codes consistently. The mapping must be documented and enforced, not ad hoc per handler.

| gRPC code | Domain meaning | When |
|-----------|---------------|------|
| `OK` | Success | Request completed |
| `INVALID_ARGUMENT` | Malformed request | Validation failure, bad field value |
| `NOT_FOUND` | Resource does not exist | Get/Update/Delete on missing entity |
| `ALREADY_EXISTS` | Duplicate creation | Create with conflicting unique key |
| `PERMISSION_DENIED` | Caller lacks access | Authenticated but unauthorized |
| `UNAUTHENTICATED` | Missing or invalid credentials | No token, expired token |
| `RESOURCE_EXHAUSTED` | Quota or rate limit hit | Too many requests, storage full |
| `FAILED_PRECONDITION` | State conflict | Optimistic concurrency violation, invalid state transition |
| `UNAVAILABLE` | Transient failure | Downstream timeout, circuit breaker open. Client should retry |
| `INTERNAL` | Server bug | Unhandled error. Log full details server-side, return generic message |
| `DEADLINE_EXCEEDED` | Timeout | Propagated from deadline infrastructure |
| `UNIMPLEMENTED` | RPC not implemented | Stub for future work or deprecated method |

- Never return `UNKNOWN` for errors you can classify. `UNKNOWN` means the error escaped classification, which is a bug in error handling.
- Include structured error details via `google.rpc.Status` and `google.rpc.ErrorInfo` for machine-readable context. Free-text messages are for humans; structured details are for retry logic and error aggregation.

### Retry and idempotency

- Idempotent RPCs (`Get`, `List`, `Delete` of an already-deleted resource) are safe to retry on `UNAVAILABLE` and `DEADLINE_EXCEEDED`.
- Non-idempotent RPCs (`Create`, `Update`) must use an idempotency key (client-generated UUID in the request) so the server can deduplicate retries. Without this, retries cause double-writes.
- Retry with exponential backoff and jitter. Fixed-interval retries cause thundering herds when a downstream recovers.
- Set a retry budget: no more than 3 attempts or 10% of total requests, whichever is lower. Unbounded retries amplify outages.

## Import conventions

- Use package-relative paths: `import "meshtastic/mesh.proto";`.
- Never use `import public` -- it pollutes the dependency graph across transitive consumers.
- Never use `import weak`.
- Sort imports alphabetically.
- Remove unused imports.
- Avoid circular imports between packages.

## Documentation

- Leading `//` comment on every message, enum, service, and RPC. At least one complete sentence.
- Leading `//` comment on every field that isn't self-explanatory from its name and type.
- Comments describe purpose and constraints, not the type (the schema already says that).
- Document units for numeric fields: `// Altitude in metres above sea level.`
- Document valid ranges or special sentinel values: `// 0 means unset; valid range 1-255.`

## Wire format considerations

### Type selection

| Use case | Type | Encoding |
|----------|------|----------|
| Positive integers, usually small | `uint32` / `uint64` | Varint |
| Integers that are often negative | `sint32` / `sint64` | ZigZag varint |
| Integers that are always large (>2^28) | `fixed32` / `fixed64` | Fixed-width |
| Latitude/longitude in integer degrees | `sfixed32` | Fixed-width (4 bytes always) |
| Flags, small counts | `uint32` | Varint |
| Timestamps | `google.protobuf.Timestamp` | Well-known type |
| Durations | `google.protobuf.Duration` | Well-known type |

- Use well-known types (`Timestamp`, `Duration`, `FieldMask`, `Any`, `Struct`) over custom integer fields for standard concepts.
- `repeated` scalar fields use packed encoding by default in proto3. No action needed.
- `bytes` for opaque binary data. `string` must be valid UTF-8.

### Size discipline

- Keep individual messages under 1 MB. Protobuf is not designed for bulk data transfer.
- Avoid messages with more than 100 fields -- they bloat memory and hit codegen limits in some languages.
- Use streaming RPCs for large result sets instead of single giant response messages.

### Custom type wrappers

Bare scalar fields carry no semantic meaning beyond their type. A `string` field for a device ID and a `string` field for a user name are interchangeable at the proto level. Wrapper messages add type safety at the schema boundary.

**When to wrap:**

- **Identifiers**: entity IDs that must not be confused with each other. A `DeviceId` and a `UserId` are both strings, but swapping them is a bug.
- **Monetary values**: currency and amount must travel together. A bare `double` for money invites rounding errors and currency mismatches.
- **Constrained strings**: email addresses, URLs, semver versions -- values with format invariants the proto schema cannot express.

**When not to wrap**: generic labels, descriptions, free-text fields, and values where the field name alone is unambiguous. Wrapping everything inflates message nesting for no safety gain.

```protobuf
// Semantic wrapper for device identifiers.
message DeviceId {
  string value = 1;
}

// Monetary amount with explicit currency.
message Money {
  // ISO 4217 currency code (e.g., "USD", "EUR").
  string currency_code = 1;

  // Integer units of the currency.
  int64 units = 2;

  // Nano-units (10^-9) of the currency. Must be -999999999 to 999999999.
  // Sign must match units (both positive or both negative).
  int32 nanos = 3;
}

// Use wrappers in service messages.
message GetDeviceRequest {
  DeviceId device_id = 1;
}
```

**Rust-side enforcement**: the generated wrapper is a struct with a single field. Implement `TryFrom` on the Rust side to validate the inner value at the conversion boundary (see Domain conversion below). The proto wrapper provides type distinction; the Rust newtype provides invariant enforcement.

## JSON / protobuf mapping

Protobuf has a canonical JSON encoding defined in the spec. Deviating from it produces payloads that other protobuf-aware tools cannot parse.

### Canonical JSON rules

| Proto type | JSON representation | Notes |
|-----------|-------------------|-------|
| `int32`, `uint32`, `sint32` | Number | |
| `int64`, `uint64`, `sint64`, `fixed64`, `sfixed64` | String | 64-bit integers exceed JavaScript's `Number.MAX_SAFE_INTEGER`. String encoding prevents silent precision loss. |
| `float`, `double` | Number | `NaN`, `Infinity`, `-Infinity` encoded as strings `"NaN"`, `"Infinity"`, `"-Infinity"` |
| `bool` | `true` / `false` | |
| `string` | String | |
| `bytes` | Base64 string | Standard base64 with padding |
| `enum` | String (enum value name) | `"HARDWARE_MODEL_UNSET"`, not `0` |
| `repeated` | Array | Empty repeated field omitted or `[]` |
| `map<K,V>` | Object | Keys are always strings in JSON |
| `google.protobuf.Timestamp` | RFC 3339 string | `"2026-04-04T12:00:00Z"` |
| `google.protobuf.Duration` | String with `s` suffix | `"1.5s"`, `"300s"` |
| `google.protobuf.FieldMask` | Comma-delimited string | `"foo,bar.baz"` (lower_camel in JSON) |
| `google.protobuf.Struct` | JSON object | Pass-through |
| `google.protobuf.Any` | Object with `@type` field | `{"@type": "type.googleapis.com/...", ...}` |

### Field name mapping

- Proto field names are `snake_case`. Canonical JSON uses `lowerCamelCase`: `battery_level` becomes `"batteryLevel"`.
- Parsers must accept both `snake_case` and `lowerCamelCase`. Emitters must produce `lowerCamelCase`.
- Override with `json_name` only when interfacing with external APIs that use a fixed JSON schema. Document the override with a comment explaining the external constraint.

```protobuf
message DeviceMetrics {
  // Maps to "batteryLevel" in JSON (automatic).
  uint32 battery_level = 1;

  // Override: external telemetry API expects "rxSNR" not "rxSnr".
  float rx_snr = 2 [json_name = "rxSNR"];
}
```

### Field presence in JSON

- Proto3 scalar fields have implicit presence: unset fields take their default value (`0`, `""`, `false`) and are omitted from JSON output.
- `optional` fields have explicit presence: the field is either set (including to the default value) or absent. Use `optional` when distinguishing "not set" from "set to zero" matters.
- `message` fields always have explicit presence: `null` in JSON means absent.
- `oneof` fields: exactly one field is set, or none. JSON includes only the set field's key.

```protobuf
message SensorReading {
  // Implicit presence: 0.0 means "default" and is omitted from JSON.
  double temperature_celsius = 1;

  // Explicit presence: 0.0 means "measured zero", absent means "no reading".
  optional double humidity_percent = 2;
}
```

### JSON serialization in Rust

- Use `prost` types with `serde` derives injected via `tonic_build::configure().type_attribute()`. Do not hand-write serde impls for generated types.
- For canonical protobuf JSON (as defined above), use `pbjson` or `prost-reflect` rather than plain serde. Plain serde does not handle 64-bit integers as strings, enums as names, or well-known type formatting.

```rust
// build.rs -- canonical protobuf JSON support via pbjson
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let descriptor_path =
        std::path::PathBuf::from(std::env::var("OUT_DIR")?)
            .join("proto_descriptor.bin");

    tonic_build::configure()
        .file_descriptor_set_path(&descriptor_path)
        .compile_protos(&["proto/service.proto"], &["proto/"])?;

    let descriptor_set = std::fs::read(&descriptor_path)?;
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_set)?
        .build(&[".mypackage"])?;

    Ok(())
}
```

### When to use JSON vs binary encoding

- **Binary** (default): Service-to-service gRPC, persistent storage, message queues. Smaller, faster, schema-enforced.
- **JSON**: External-facing REST APIs, browser clients, debugging, log payloads that humans read. Larger but human-readable.
- Never use protobuf text format for interchange. It is unstable across implementations and breaks on field renames.
- When a service exposes both gRPC and REST, use `grpc-gateway` or a similar transcoder rather than maintaining parallel handler implementations. Dual implementations drift.

## Rust codegen patterns

### Build configuration

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/service.proto"], &["proto/"])?;
    Ok(())
}
```

### Type extensions

- Never modify generated code directly. Extend in separate `_ext.rs` files with `impl` blocks.
- Use `Config::type_attribute` and `Config::field_attribute` to inject derives (`serde::Serialize`, `Hash`, `Eq`) at build time.

### Domain conversion

- Implement `From<ProtoType>` / `TryFrom<ProtoType>` between proto types and domain types.
- Validate at the conversion boundary. Proto types are permissive (all fields optional in proto3). Domain types enforce invariants.
- Keep proto types out of business logic. Convert at the edge (handler entry, client call site).
- Use `TryFrom` (not `From`) when the proto type can represent invalid states. `From` is for infallible conversions (domain → proto).

```rust
use std::num::NonZeroU32;

/// Domain type with enforced invariants.
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub battery_level: NonZeroU32,
}

/// Validated device identifier. Non-empty, ASCII-only.
pub struct DeviceId(String);

impl TryFrom<String> for DeviceId {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(ValidationError::EmptyField("device_id"));
        }
        if !value.is_ascii() {
            return Err(ValidationError::InvalidFormat("device_id", "ASCII only"));
        }
        Ok(Self(value))
    }
}

/// Proto → domain: fallible, because proto fields are permissive.
impl TryFrom<proto::GetDeviceResponse> for Device {
    type Error = ValidationError;

    fn try_from(proto: proto::GetDeviceResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            id: DeviceId::try_from(proto.device_id)?,
            name: proto.name,
            battery_level: NonZeroU32::new(proto.battery_level)
                .ok_or(ValidationError::EmptyField("battery_level"))?,
        })
    }
}

/// Domain → proto: infallible, domain types are always valid proto.
impl From<Device> for proto::GetDeviceResponse {
    fn from(device: Device) -> Self {
        Self {
            device_id: device.id.0,
            name: device.name,
            battery_level: device.battery_level.get(),
        }
    }
}
```

### Error mapping

- Map gRPC status codes to domain errors via `impl From<ServiceError> for tonic::Status`.
- Include structured details in `tonic::Status::with_details()` for machine-readable error context.

```rust
use tonic::{Code, Status};

/// Domain error for the telemetry service.
#[derive(Debug, snafu::Snafu)]
pub enum TelemetryError {
    #[snafu(display("device {device_id} not found"))]
    DeviceNotFound { device_id: String },

    #[snafu(display("invalid time range: start must precede end"))]
    InvalidTimeRange,

    #[snafu(display("storage unavailable: {source}"))]
    Storage { source: sqlx::Error },
}

impl From<TelemetryError> for Status {
    fn from(err: TelemetryError) -> Self {
        match &err {
            TelemetryError::DeviceNotFound { .. } => {
                Status::not_found(err.to_string())
            }
            TelemetryError::InvalidTimeRange => {
                Status::new(Code::InvalidArgument, err.to_string())
            }
            TelemetryError::Storage { .. } => {
                // Log full details server-side; return generic message to client.
                tracing::error!(%err, "storage failure");
                Status::unavailable("storage temporarily unavailable")
            }
        }
    }
}
```

### Server implementation

Implement the generated trait, convert proto types to domain types at the handler boundary, and convert back for the response. Keep handlers thin: validate, convert, delegate to domain logic, convert result.

```rust
use tonic::{Request, Response, Status};
use crate::proto::telemetry_service_server::TelemetryService;
use crate::proto::{GetDeviceRequest, GetDeviceResponse};
use crate::domain::DeviceId;

pub struct TelemetryHandler {
    store: Arc<dyn DeviceStore>,
}

#[tonic::async_trait]
impl TelemetryService for TelemetryHandler {
    async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> Result<Response<GetDeviceResponse>, Status> {
        let req = request.into_inner();

        // Validate and convert at the boundary.
        let device_id = DeviceId::try_from(req.device_id)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Delegate to domain logic.
        let device = self.store
            .get(device_id)
            .await
            .map_err(TelemetryError::from)?;

        // Convert domain type to proto for the response.
        Ok(Response::new(GetDeviceResponse::from(device)))
    }
}
```

### Client usage

Wrap generated clients in a domain-specific client that handles connection management, deadline setting, and response conversion. Raw generated clients leak proto types and connection details into business logic.

```rust
use crate::proto::telemetry_service_client::TelemetryServiceClient;
use tonic::transport::Channel;

pub struct TelemetryClient {
    inner: TelemetryServiceClient<Channel>,
    default_timeout: std::time::Duration,
}

impl TelemetryClient {
    pub async fn connect(endpoint: &str) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(endpoint.to_owned())?
            .connect_timeout(std::time::Duration::from_secs(5))
            .connect()
            .await?;

        Ok(Self {
            inner: TelemetryServiceClient::new(channel),
            default_timeout: std::time::Duration::from_secs(10),
        })
    }

    pub async fn get_device(&self, id: DeviceId) -> Result<Device, TelemetryError> {
        let mut client = self.inner.clone();
        let mut request = tonic::Request::new(GetDeviceRequest {
            device_id: id.to_string(),
        });

        // Every call gets a deadline. No exceptions.
        request.set_timeout(self.default_timeout);

        let response = client
            .get_device(request)
            .await
            .map_err(TelemetryError::from)?;

        Device::try_from(response.into_inner())
    }
}
```

### Interceptors

Use tonic interceptors for cross-cutting concerns. Each interceptor does one thing.

```rust
use tonic::{Request, Status};

/// Injects a request-id into every outbound request for tracing correlation.
pub fn request_id_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let request_id = ulid::Ulid::new().to_string();
    req.metadata_mut().insert(
        "x-request-id",
        request_id.parse().unwrap(),
    );
    Ok(req)
}

// Attach to client:
// TelemetryServiceClient::with_interceptor(channel, request_id_interceptor);
```

For server-side interceptors that need async or access to service state, use `tonic::service::interceptor` with a tower layer:

```rust
use tower::ServiceBuilder;

let layer = ServiceBuilder::new()
    .layer(tonic::service::interceptor(auth_interceptor))
    .into_inner();

Server::builder()
    .layer(layer)
    .add_service(telemetry_service)
    .serve(addr)
    .await?;
```

### Testing gRPC services

Test gRPC services by instantiating the handler struct directly and calling trait methods. This avoids network overhead, port allocation, and flaky connection timing. Reserve integration tests with a real server for verifying wire-level concerns (serialization, TLS, interceptor ordering).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[tokio::test]
    async fn get_device_returns_not_found_for_missing_device() {
        let store = Arc::new(InMemoryDeviceStore::empty());
        let handler = TelemetryHandler { store };

        let request = Request::new(GetDeviceRequest {
            device_id: "nonexistent".into(),
        });

        let status = handler
            .get_device(request)
            .await
            .unwrap_err();

        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn get_device_returns_device_when_present() {
        let store = Arc::new(InMemoryDeviceStore::with_device(test_device()));
        let handler = TelemetryHandler { store };

        let request = Request::new(GetDeviceRequest {
            device_id: test_device().id.to_string(),
        });

        let response = handler
            .get_device(request)
            .await
            .expect("should succeed");

        assert_eq!(
            response.into_inner().device_id,
            test_device().id.to_string(),
        );
    }
}
```

## Anti-patterns

| Anti-pattern | Problem | Alternative |
|-------------|---------|-------------|
| Reusing field numbers after deletion | Silent data corruption from old serialized data | `reserved` the number and name |
| Changing field types | Breaks deserialization across versions | Add a new field, deprecate the old one |
| Missing enum zero value | First real value becomes ambiguous default | Always define `_UNSPECIFIED = 0` |
| `import public` | Transitive dependency pollution | Direct imports only |
| `allow_alias` on enums | JSON serialization ambiguity | One name per number |
| Boolean for extensible state | Locked to two values forever | Enum with `UNSPECIFIED` zero |
| Shared request/response types across RPCs | Couples independent evolution | Unique types per RPC |
| Hundreds of fields per message | Memory bloat, codegen limits | Split into nested messages |
| Modifying generated code | Overwritten on next build | Extension files with `impl` blocks |
| Text format for interchange | Breaks on any field rename | Binary or JSON encoding |
| `oneof` field migration | Moving fields in/out breaks Go stubs and loses data | Treat `oneof` membership as permanent |
| Stringly-typed fields for structured data | No validation, no evolution | Nested message or enum |
| RPCs without deadlines | Hung calls consume resources indefinitely | Always set a deadline on every call |
| Returning `UNKNOWN` for classified errors | Hides error semantics from clients and monitoring | Map every domain error to a specific gRPC code |
| Business logic in interceptors | Interceptors apply to all RPCs; domain logic is per-RPC | Interceptors for infra, handlers for domain |
| Plain serde for protobuf JSON | Mishandles int64, enums, well-known types | `pbjson` or `prost-reflect` for canonical encoding |
| Unbounded retries without backoff | Thundering herds amplify outages | Exponential backoff with jitter and retry budget |
| Dual REST/gRPC handler implementations | Implementations drift over time | `grpc-gateway` or transcoder from a single definition |
| Offset-based pagination on mutable data | Inserts/deletes shift offsets, skipping or duplicating items | Cursor-based pagination with opaque `page_token` |
| Client-parseable page tokens | Clients depend on token internals, blocking server changes | Opaque tokens; encode cursor state server-side |
| Bare scalars for entity identifiers | `string` device ID and `string` user name are interchangeable | Wrapper messages (`DeviceId`, `UserId`) |
| No keepalive on long-lived connections | Dead connections consume resources until OS TCP timeout | Configure keepalive interval and timeout on both sides |
| Unbounded streaming without backpressure | Slow consumers cause server memory exhaustion | Bound send buffers, enforce message/size limits |
