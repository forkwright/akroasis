# P2-R2: Mesh Topology and Routing

**Phase:** 2  -  Kerykeion
**Block:** P2-03, P2-04
**Depends on:** P2-R1 (Meshtastic routing mechanics), P2-R27 (DTN patterns)

---

## 1. Graph Library Selection

**Recommendation: `petgraph`**

| Criterion | petgraph | graphlib | Custom |
|-----------|----------|----------|--------|
| Directed weighted graphs | Yes (`DiGraph`) | Yes | Build it |
| Node/edge attributes | Via index map | Limited | Build it |
| Dijkstra / BFS / DFS | Built-in | Partial | Build it |
| Serde support | `petgraph::Graph` + manual | Partial | Build it |
| Maintenance | Active, widely used | Slow | N/A |
| `no_std` path | No (not needed) | No | Possible |

`graphlib` is a thin wrapper with a sparser API. A custom adjacency list is warranted only if the graph structure is specialized (it is not here  -  this is a sparse, small, weighted digraph). `petgraph` covers the use case and does not require writing traversal algorithms.

**Concrete types:**

```rust
use petgraph::graph::{DiGraph, NodeIndex};

// WHY: DiGraph because A→B SNR and B→A SNR are independent observations.
// NodeIndex is the stable handle stored in the node table.
type MeshGraph = DiGraph<NodeRecord, LinkRecord>;
```

The graph holds at most ~50 nodes in practice (a dense Meshtastic deployment). `petgraph` is not a bottleneck at this scale.

---

## 2. Node and Edge Data Model

### 2.1 Node Record

```rust
use compact_str::CompactString;
use jiff::Timestamp;

/// Per-node state maintained by the topology engine.
///
/// WHY: Flat struct, not enum. Every live node in the graph has all fields;
/// absence is expressed with Option. This keeps match arms minimal.
pub(crate) struct NodeRecord {
    /// Meshtastic node number (lower 32 bits of the hardware MAC hash).
    pub node_num: u32,
    /// Short display name (4 chars by convention, e.g. "ALPH").
    pub short_name: CompactString,
    /// Long display name.
    pub long_name: CompactString,
    /// Most recent GPS fix reported by this node, if any.
    pub position: Option<NodePosition>,
    /// Most recent device telemetry.
    pub metrics: Option<DeviceMetrics>,
    /// Wall-clock time the server last received any packet from this node.
    pub last_heard: Timestamp,
    /// How the server reaches this node (direct USB/TCP or via mesh hops).
    pub connection: NodeConnection,
    /// True if this node has internet connectivity (MQTT / WiFi bridge role).
    pub is_gateway: bool,
    /// Minimum hop distance from any server-connected node, as last observed.
    pub hops_from_server: u8,
    /// Meshtastic device role reported in NodeInfo.
    pub role: DeviceRole,
    /// Whether this node is directly connected to kerykeion via serial/TCP.
    pub server_connected: bool,
}

pub(crate) struct NodePosition {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<i32>,
    /// When this position was reported (may differ from last_heard).
    pub observed_at: Timestamp,
}

pub(crate) struct DeviceMetrics {
    /// Battery level 0–100. None if node is externally powered.
    pub battery_level: Option<u8>,
    /// Air utilization fraction 0.0–1.0 (firmware channel utilization metric).
    pub channel_utilization: f32,
    pub observed_at: Timestamp,
}

pub(crate) enum NodeConnection {
    /// Directly connected over USB serial.
    Serial { port: CompactString },
    /// Directly connected over TCP.
    Tcp { addr: std::net::SocketAddr },
    /// Reachable only via mesh hops through another node.
    Mesh,
    /// Not reachable (partitioned or powered off).
    Unreachable,
}

/// Meshtastic device roles relevant to topology decisions.
#[non_exhaustive]
pub(crate) enum DeviceRole {
    Client,
    ClientMute,
    Router,
    RouterClient,
    Repeater,
    Tracker,
    Sensor,
    Tak,
    TakTracker,
    Unknown(u32),
}
```

**Stale node policy:** A node is not removed from the graph when it goes silent. Removal destroys path history. Instead, update `connection` to `Unreachable` and leave the node in place. The visualization layer reads `last_heard` to render status. Nodes unseen for longer than the configured eviction window (default: 7 days) are culled on startup, not inline.

### 2.2 Edge Record

```rust
/// Directional link: "node A received a packet from node B with these characteristics."
///
/// WHY: Asymmetric by design. A→B SNR comes from B's radio reporting what it
/// received from A, and vice versa. These are never equal in practice.
pub(crate) struct LinkRecord {
    /// Signal-to-noise ratio in dB at the time of last observation.
    /// Meshtastic LoRa range: approximately −20 dB (noise floor) to +15 dB.
    pub snr_db: f32,
    /// RSSI in dBm, if available (not all firmware versions report it).
    pub rssi_dbm: Option<i16>,
    /// Wall-clock time this link was last observed.
    pub last_observed: Timestamp,
    /// Number of times this link was observed (used for reliability scoring).
    pub observation_count: u32,
    /// Where this observation came from.
    pub source: LinkSource,
}

pub(crate) enum LinkSource {
    /// Reported in a NEIGHBORINFO_APP broadcast from the receiving node.
    NeighborInfo,
    /// Inferred from a received MeshPacket's rx_snr / relay_node fields.
    PacketMetadata,
    /// Confirmed by a TRACEROUTE_APP response.
    Traceroute,
}
```

**Time-decaying edge weight:** The graph weight used for Dijkstra is not raw SNR. It is a composite that penalizes stale observations:

```
weight(edge) = (snr_max - snr_db) + age_penalty(last_observed)

age_penalty(t) = 0         if age < 15 min
               = 3         if age < 60 min
               = 8         if age < 4 hours
               = 20        if age >= 4 hours   (effectively disconnected)
```

Lower weight = better path (Dijkstra minimizes). An SNR of +10 dB observed 5 minutes ago has weight ~5; the same SNR observed 3 hours ago has weight ~13.

**Bidirectional quality:** A link is only considered usable for a directed message if both the forward and reverse edges are live. The composite quality of a path is `min(forward_weight, reverse_weight)` per hop, not `(forward + reverse) / 2`, because the weaker direction is the bottleneck.

---

## 3. Data Sources and Graph Update Strategy

### 3.1 Data Sources

| Source | Portnum | Trigger | Graph Info |
|--------|---------|---------|------------|
| `NODEINFO_APP` | 4 | Broadcast ~15 min, or on demand | Node name, role, hardware model |
| `POSITION_APP` | 3 | Broadcast per position interval | Node coordinates |
| `TELEMETRY_APP` | 67 | Broadcast per telemetry interval | Battery, channel utilization |
| `NEIGHBORINFO_APP` | 8 | Broadcast per neighbor interval (default 6h, min 1h) | Direct adjacency pairs with SNR |
| `TRACEROUTE_APP` | 70 | On-demand only | Full multi-hop path with per-hop SNR |
| Any `MeshPacket` | Any | Every received packet | `rx_snr`, `relay_node`, `hop_start`, `hop_limit` |

**Passive inference from MeshPacket fields:**

Every packet the server receives carries:
- `rx_snr`: SNR at the last relay node (not the originator)
- `relay_node`: node_num of the immediate relay (0 if direct)
- `hop_start`: hops remaining when the packet left the originator
- `hop_limit`: hops remaining when the server received the packet

From these: `hops_traversed = hop_start - hop_limit`. If `relay_node` is non-zero, we know the final hop is `relay_node → server_connected_node` with SNR `rx_snr`. Add that directed edge to the graph immediately  -  no traceroute required.

### 3.2 Update Frequency

| Event | Frequency | Mesh traffic generated |
|-------|-----------|----------------------|
| NEIGHBORINFO broadcast | Configurable, default 6h | 1 packet per node per interval |
| NODEINFO refresh | ~15 min | 1 packet per node per interval |
| Passive packet learning | Every received packet | 0 (piggybacks existing traffic) |
| Traceroute (active) | On-demand, server-initiated | 1 packet per target per traceroute |
| Server-initiated NEIGHBORINFO request | As needed | 1 admin request packet |

**Policy:** Do not increase NEIGHBORINFO broadcast frequency from the firmware default (6h). The topology engine builds its picture from passive packet metadata as the primary source, using NeighborInfo to fill gaps and confirm adjacency. Active traceroutes are issued only when:
- A node's path to server has not been confirmed within the last 2h AND the node is still heard.
- A partition heals (first contact after a gap triggers a traceroute to map the new path).
- A gateway failover occurs (traceroute all nodes via new gateway immediately).

### 3.3 Persistence

Graph state is serialized to fjall on every topology change (add node, add edge, update edge SNR, mark unreachable). The serialization format is CBOR (already a workspace dependency via `ciborium`), keyed by a fixed topology key. On startup, the engine loads the persisted graph and treats all nodes as `Unreachable` until a packet is received.

**Delta updates vs full snapshot:** Write full snapshot. The graph is small (≤50 nodes, ≤200 edges). A full CBOR snapshot is under 20 KB. Delta tracking adds code complexity with no meaningful I/O benefit at this scale.

---

## 4. Route Discovery and Path Analysis

### 4.1 Active Traceroute

Send a `TRACEROUTE_APP` packet (portnum 70) to a target node. The firmware relays it hop-by-hop, appending each relay's node_num and the link SNR, then the destination returns the reverse path. The server receives a `RouteDiscovery` protobuf with:

```
route:        [node_num, ...]    // intermediate nodes, originator to dest
snr_towards:  [snr_db, ...]      // SNR at each hop toward destination
route_back:   [node_num, ...]    // reverse path (may differ)
snr_back:     [snr_db, ...]      // SNR at each hop back
```

Processing: For each consecutive pair in `route` + destination, add or update a directed edge. For each pair in `route_back` + origin, add the reverse edges. This single traceroute populates multiple directed edges in one transaction.

Traceroute cadence for our 7-node mesh: issue one traceroute per node every 2 hours, staggered. That is 7 traceroutes over 2 hours = one every ~17 minutes. Each traceroute generates 2 packets (request + response) ×  hop count airtime. See bandwidth budget in section 9.

### 4.2 Passive Route Learning

Every received `MeshPacket` is inspected before dispatch:

```rust
fn ingest_packet(graph: &mut MeshGraph, pkt: &MeshPacket, received_via: u32) {
    let from = pkt.from;
    let hops = pkt.hop_start.saturating_sub(pkt.hop_limit);
    let snr = pkt.rx_snr;

    // Update hop distance for the originating node.
    graph.update_hops_from_server(from, hops);

    if pkt.relay_node != 0 {
        // The immediate relay node forwarded this to our connected node.
        // Add edge: relay_node → received_via with this SNR.
        graph.update_edge(pkt.relay_node, received_via, snr, LinkSource::PacketMetadata);
    } else {
        // Direct transmission: from → received_via.
        graph.update_edge(from, received_via, snr, LinkSource::PacketMetadata);
    }
}
```

### 4.3 Path Quality Score

When multiple routes exist between server-connected node and destination, the path score ranks candidate sequences of directed edges by weight, hop count penalty, and delivery rate.

```
path_score = sum over edges of: weight(edge)
           + hop_count_penalty * hops
           + (1 - delivery_rate) * 20

delivery_rate = acked_messages / sent_messages (rolling 24h window, min 5 samples)
hop_count_penalty = 2 per hop (each relay adds latency and failure probability)
```

Thresholds:

| Score | Classification |
|-------|---------------|
| 0–15 | Good  -  direct or short path with strong SNR |
| 16–35 | Degraded  -  long path or weak links |
| 36–60 | Poor  -  marginally usable; prefer store-and-forward |
| 61+ | Effectively disconnected |

### 4.4 Multi-Path Tracking

The graph retains all observed paths. For routing decisions, the engine runs Dijkstra from each server-connected node to the target, producing a ranked list of paths. The top path is used for immediate delivery attempts. The second-best path is the automatic failover if the primary fails two consecutive ACK cycles.

---

## 5. Store-and-Forward Architecture

### 5.1 Firmware S&F vs Server-Side S&F

| Layer | Scope | When to use |
|-------|-------|-------------|
| Firmware S&F (portnum 65) | In-mesh only, router-class nodes buffer for offline clients within the mesh | Node-to-node delivery within the same RF network; no internet required |
| Server-side S&F (kerykeion) | Between connected-node boundary and server, or across internet bridges | Messages routed server→mesh when destination is offline; cross-gateway delivery; priority queuing; crash recovery |

The two layers are complementary, not competing. Firmware S&F handles the "last mile" within the mesh when the server cannot observe the destination directly. Server-side S&F handles custody when the server is the originator or relay point, and provides the priority/TTL/retry semantics that firmware S&F does not.

### 5.2 Queue Design

One fjall keyspace per destination node_num. Keys are ULID (lexicographically ordered by creation time). Values are CBOR-encoded `QueuedMessage`.

```rust
pub(crate) struct QueuedMessage {
    /// ULID used as fjall key; also serves as dedup handle.
    pub message_id: Ulid,
    pub destination: u32,
    pub payload: Vec<u8>,           // Meshtastic packet bytes, already encrypted
    pub portnum: u32,
    pub priority: MessagePriority,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub attempt_count: u8,
    pub last_attempt: Option<Timestamp>,
    pub want_ack: bool,
}

pub(crate) enum MessagePriority {
    Alert    = 0,   // PACE primary: immediate delivery required
    Reliable = 1,   // ACK requested, retry up to limit
    Default  = 2,   // Best effort, no retry
    Background = 3, // Opportunistic: deliver when path is clear
}
```

**Queue depth limit:** 100 messages per destination. On overflow, drop lowest priority, oldest-first. Alert messages are never dropped; if the queue is full of Alerts, the oldest Alert is dropped.

**TTL policy:**

| Priority | Default TTL |
|----------|------------|
| Alert | 72 hours |
| Reliable | 24 hours |
| Default | 6 hours |
| Background | 2 hours |

### 5.3 Delivery and Retry

**Confirmation:** When `want_ack = true`, the firmware sends a `ROUTING_APP` ACK packet when the destination receives and decodes the message. The server matches this against the message_id and marks the queue entry delivered.

**Retry intervals** (exponential backoff with jitter):

```
attempt 1: immediate (node just came online or first enqueue)
attempt 2: 5 min + [0, 60s] jitter
attempt 3: 20 min + [0, 120s] jitter
attempt 4: 60 min + [0, 300s] jitter
attempt 5: 4 hours + [0, 600s] jitter
attempt 6+: 4 hours (flat, until TTL)
max_attempts: 10 for Alert/Reliable, 3 for Default, 1 for Background
```

After max_attempts, emit a `MessageFailed` signal and remove from queue.

### 5.4 Offline Node Detection

Three states, not two:

| State | Definition | Retry strategy |
|-------|-----------|----------------|
| `Reachable` | Path score ≤ 35, last heard < 30 min | Deliver immediately |
| `Intermittent` | Last heard 30 min – 4 hours, OR path score 36–60 | Queue with normal retry; attempt on next any-packet event from that node |
| `Offline` | Last heard > 4 hours | Queue; deliver only on reconnection event |

Transition `Intermittent → Offline` at 4 hours to avoid hammering a weakly-heard node. Transition `Offline → Reachable` immediately on any packet received from the node; flush queue at that point.

---

## 6. Gateway Detection and Failover

### 6.1 Detection

A node is classified as a gateway if any of the following are true:

1. **Manual designation:** Operator has marked it in kerykeion config. The RAK2245 is always manually designated. Highest trust.
2. **Device role:** `DeviceRole::Router` or `DeviceRole::RouterClient`. These roles imply a capable node, though not all have internet access.
3. **WiFi enabled:** Reported in device config (`wifi_enabled = true`). T-Deck Plus qualifies.
4. **MQTT config present:** Node reports an MQTT server address in its channel config. Direct indicator of internet bridge role.
5. **Server observation:** kerykeion receives MQTT-relayed packets that originated at this node. Definitive.

Classification priority: Manual > MQTT-observed > WiFi-enabled + Router role > Router role alone.

### 6.2 Gateway Health Score

```rust
pub(crate) struct GatewayHealth {
    pub node_num: u32,
    /// Higher = better. Computed from battery, last_heard, path_score.
    pub score: f32,
    pub last_seen: Timestamp,
    pub battery_level: Option<u8>,
    pub path_score_to_server: f32,
    pub classification: GatewayClassification,
}

pub(crate) enum GatewayClassification {
    /// Operator-designated, always preferred.
    Designated,
    /// WiFi-capable node with Router role.
    WifiCapable,
    /// Router role without confirmed WiFi.
    RouterRole,
}
```

Health score formula:

```
score = classification_base
      + battery_bonus(battery_level)
      - path_penalty(path_score_to_server)
      - age_penalty(last_seen)

classification_base: Designated = 100, WifiCapable = 60, RouterRole = 30
battery_bonus: 0 if no battery data; else (battery_level / 100) * 20
path_penalty: path_score_to_server (higher path score = worse)
age_penalty: 0 if < 5min, 10 if < 30min, 30 if < 2h, 100 if > 2h
```

### 6.3 Election Algorithm

```
active_gateway = arg_max(GatewayHealth.score, over gateways with age_penalty < 30)
```

If no gateway has been seen within 2 hours, enter no-gateway mode: queue all internet-bound messages, attempt delivery when a gateway reconnects.

### 6.4 Failover Timing

Every 30 seconds the engine runs a cheap in-memory gateway-health scan (no mesh traffic). Failover triggers when the active gateway's age_penalty reaches 30 (unseen for 30 minutes). At that point:

1. Elect new gateway from the health score table.
2. Emit `GatewayOffline` signal for the departed gateway.
3. Emit `GatewayOnline` signal for the new primary.
4. Issue traceroutes to all previously-known nodes via the new gateway path.
5. Flush any internet-bound queue entries through the new gateway.

30 minutes is chosen deliberately: it is long enough to avoid flapping during brief connectivity gaps (a vehicle driving through a dead zone takes 5–15 minutes typically) but short enough that PACE communications are not blocked for mission-critical windows.

### 6.5 Bridge Architecture

The gateway is an RF-to-internet bridge. kerykeion does not speak directly to the internet from the gateway node's perspective  -  it routes a Meshtastic packet to the gateway, which then forwards it outbound via MQTT or a direct API call. The return path is symmetric.

**Outbound (server → internet):**

```
kerykeion → serial/TCP → server-connected node → mesh hops → gateway node → MQTT/HTTP → internet
```

1. kerykeion sends an admin or data packet addressed to a gateway-accessible port on the gateway node.
2. The mesh routes the packet to the gateway via the best available path.
3. The gateway firmware's MQTT or network module forwards it.

**Inbound (internet → mesh node):**

```
internet → MQTT broker → gateway node (subscribed) → mesh hops → destination node
```

kerykeion cannot initiate inbound delivery directly; it depends on the gateway's MQTT subscription being active. For PACE-critical inbound messages, the server maintains a persistent MQTT connection to the same broker the gateway uses, enabling the server to publish directly and bypass the serial path latency.

**Addressing:** Every gateway-destined packet is addressed to the gateway's `node_num`, not broadcast. This prevents the mesh from relaying gateway-bound traffic beyond the gateway node itself.

**Internet-bound queue:** When no gateway is available (`no-gateway mode`), all internet-bound messages accumulate in a separate fjall keyspace keyed by `"internet_queue"`. On gateway reconnection, this queue is flushed in priority order before the per-destination S&F queues.

---

## 7. Partition Detection

### 7.1 Detection Heuristics

A node is considered partitioned when it is absent from all observability sources beyond a threshold. The thresholds account for the fact that NEIGHBORINFO is only broadcast every 6 hours by default.

**Absence evidence accumulates from multiple sources:**

| Source | Significance |
|--------|-------------|
| No packets received (any portnum) | Primary signal |
| Absent from all NEIGHBORINFO broadcasts received from any node | Confirms no RF contact within the mesh |
| Path score crosses 61 (all edges age out) | Computed consequence |

**Timeout ladder:**

```
0 – 15 min:   Normal gap. No state change.
15 – 30 min:  Mark Intermittent. Begin retry backoff.
30 min – 4h:  Mark Offline. Emit PartitionSuspected signal.
> 4h:         Mark Partitioned. Emit PartitionDetected signal.
```

The distinction between `Offline` (powered off, deliberate) and `Partitioned` (RF isolated but possibly powered on) is inferred from context:

- Vehicle nodes (T-Echo): offline on schedule is expected. If last_heard aligns with a known departure time, classify as `Offline`. Otherwise `Partitioned`.
- Fixed nodes (WisBlock, RAK gateway): any absence > 4h should be treated as `Partitioned` first (solar failure, obstacle, hardware issue), not assumed powered off.

The server cannot definitively distinguish the two. Emit the signal; let the operator decide.

### 7.2 Partition Healing

A partition heals the moment any packet arrives from the node. On receipt:

1. Transition node back to `Reachable` or `Intermittent` based on path score.
2. Emit `PartitionHealed` signal.
3. Issue immediate traceroute to map the reconnected path.
4. Flush the node's store-and-forward queue (first delivery attempt immediately).

### 7.3 Sub-Mesh Detection

Passive detection runs whenever the partition topology changes: if NEIGHBORINFO broadcasts from node A list nodes B and C as neighbors, but A, B, and C are all Partitioned from the server's view, they form a sub-mesh (two or more nodes that can hear each other but cannot reach the server). The topology engine identifies these by finding connected components in the subgraph of Partitioned nodes.

This information informs PACE planning: a sub-mesh may have internal mesh communication even during a partition from the gateway. When a vehicle node from the sub-mesh eventually reaches server connectivity, kerykeion can deliver accumulated messages from the sub-mesh via DTN custody transfer.

---

## 8. Visualization Data Model

### 8.1 What opsis Needs

opsis (ratatui TUI) renders the mesh topology. It needs a serializable snapshot that decouples the topology engine from the rendering layer  -  ratatui runs in the terminal event loop, not the async mesh loop.

```rust
/// Snapshot of the mesh topology, suitable for rendering without locking the graph.
///
/// WHY: Clone-on-read from the live graph, not a live reference. The TUI render
/// loop can hold this struct across frames without blocking mesh processing.
pub struct TopologySnapshot {
    pub nodes: Vec<VisNode>,
    pub edges: Vec<VisEdge>,
    pub active_gateway: Option<u32>,
    pub snapshot_at: Timestamp,
}

pub struct VisNode {
    pub node_num: u32,
    pub short_name: CompactString,
    pub status: NodeStatus,
    /// Force-directed 2D layout position, normalized to [0.0, 1.0].
    /// For GPS-equipped nodes: projected from lat/lon. For others: force layout.
    pub x: f32,
    pub y: f32,
    pub battery_level: Option<u8>,
    pub hops_from_server: u8,
    pub is_gateway: bool,
    pub server_connected: bool,
}

pub struct VisEdge {
    pub from: u32,
    pub to: u32,
    pub snr_db: f32,
    /// Age of this observation (for dimming stale edges in the TUI).
    pub age_secs: u64,
    /// Highlight this edge as part of the active route to some destination.
    pub on_active_path: bool,
}

#[non_exhaustive]
pub enum NodeStatus {
    /// Receiving packets normally.
    Reachable,
    /// Last heard 30 min – 4h.
    Intermittent,
    /// Last heard > 4h, cause unknown.
    Partitioned,
    /// Known to be powered off (operator-classified or scheduled).
    Offline,
}
```

### 8.2 Force-Directed Layout

For nodes without GPS (or for scale-normalized terminal display), a force-directed layout assigns 2D positions. The implementation is a simplified Fruchterman-Reingold iteration run offline (not per-frame):

```
repulsive force: all pairs, F_r = k² / distance
attractive force: connected pairs only, F_a = distance² / k
k = C * sqrt(area / node_count)    // optimal distance
```

SNR maps to spring rest length: lower SNR → longer rest length → visual distance. After convergence (≤50 iterations for ≤50 nodes), positions are normalized to [0.0, 1.0] and stored in the snapshot. The layout is recomputed when the graph structure changes (nodes added/removed), not on every SNR update.

For nodes with GPS, project (lat, lon) to the same [0.0, 1.0] space using the bounding box of all known positions. GPS-positioned nodes act as anchors in the force simulation; other nodes settle relative to them.

### 8.3 Real-Time Update Delivery

The topology engine publishes `TopologySnapshot` on a `tokio::sync::watch` channel. The TUI poll loop reads the latest value from the receiver on each render frame. This is zero-copy (the TUI clones only if it needs to retain the snapshot) and never blocks the topology engine.

```rust
// WHY: watch channel, not broadcast. The TUI only needs the latest snapshot,
// not every intermediate state. watch discards intermediates automatically.
let (tx, rx) = tokio::sync::watch::channel(TopologySnapshot::empty());
```

---

## 9. GeoSignal Variant Definitions

The existing `MeshDetail` enum in `koinon::signal` has three variants (`NodeSeen`, `Message`, `Position`). These cover collection-layer observations but not topology-layer events. The topology engine in kerykeion produces a richer event set.

**Proposed additions to `MeshDetail`:**

```rust
/// A previously unknown node was added to the topology graph.
NodeDiscovered {
    node_num: u32,
    short_name: CompactString,
    hops_from_server: u8,
},

/// A node was not heard for longer than the partition threshold.
NodePartitioned {
    node_num: u32,
    last_heard: Timestamp,
    /// Elapsed seconds since last packet.
    silence_secs: u64,
},

/// A previously partitioned node sent its first packet after reconnecting.
NodeReconnected {
    node_num: u32,
    /// Elapsed seconds the node was silent.
    gap_secs: u64,
},

/// A link's SNR changed by more than the significance threshold (3 dB).
LinkQualityChanged {
    from_node: u32,
    to_node: u32,
    old_snr_db: f32,
    new_snr_db: f32,
},

/// A message was confirmed delivered to its destination via ACK.
MessageDelivered {
    message_id: Ulid,
    destination: u32,
    /// Total elapsed time from enqueue to confirmed delivery.
    latency_secs: u64,
    attempt_count: u8,
},

/// A message exhausted retries or TTL without confirmed delivery.
MessageFailed {
    message_id: Ulid,
    destination: u32,
    reason: MessageFailureReason,
},

/// A gateway node came online or was elected as primary.
GatewayOnline {
    node_num: u32,
    classification: GatewayClassification,
},

/// The primary gateway is no longer reachable.
GatewayOffline {
    node_num: u32,
    /// True if a failover gateway was available.
    failover_available: bool,
},

/// Two or more partitioned nodes were identified as a reachable sub-mesh.
SubMeshDetected {
    /// Node numbers forming the isolated sub-mesh.
    members: Vec<u32>,
},

/// A store-and-forward queue entry was flushed to the mesh on node reconnection.
StoreForwardFlushed {
    destination: u32,
    message_count: u32,
},
```

`MessageFailureReason`:
```rust
pub enum MessageFailureReason {
    MaxRetriesExceeded,
    TtlExpired,
    NoPathAvailable,
    QueueEvicted,   // displaced by higher-priority message
}
```

These signals flow into the `GeoSignal` broadcast channel and are consumed by semaino (aggregation) and opsis (display). No signal is emitted without a corresponding state change in the graph  -  events are not re-emitted on polling, only on transition.

---

## 10. Bandwidth Budget

### 10.1 Assumptions

- LoRa SF10, BW125 kHz, CR 4/5  -  typical Meshtastic default for longer range
- Effective data rate: ~980 bps (SF10 BW125)
- Duty cycle limit: 1% per LoRa regulatory requirement (EU); US has no duty cycle limit but we budget conservatively
- 7 nodes total; 4 have regular activity (2 T-Deck Plus personal carry, RAK gateway, WisBlock)
- NeighborInfo interval: 6h default (configurable)

### 10.2 Per-Packet Airtime

| Packet type | Payload bytes | Total LoRa frame | Airtime at SF10 BW125 |
|-------------|--------------|------------------|-----------------------|
| NODEINFO_APP | ~60 | ~80 | ~0.65 s |
| POSITION_APP | ~30 | ~50 | ~0.41 s |
| TELEMETRY_APP | ~30 | ~50 | ~0.41 s |
| NEIGHBORINFO_APP | ~50 + 8×neighbor | ~90 | ~0.73 s |
| TRACEROUTE request | ~30 | ~50 | ~0.41 s |
| TRACEROUTE response (3 hops) | ~60 | ~80 | ~0.65 s |
| TEXT_MESSAGE | ~200 (max) | ~230 | ~1.87 s |

### 10.3 Topology Traffic Per Hour

| Source | Packets/hour (7 nodes) | Airtime/hour |
|--------|------------------------|-------------|
| NODEINFO broadcast (~15 min) | 7 × 4 = 28 | 28 × 0.65 = 18.2 s |
| POSITION broadcast (~5 min, mobile nodes only) | 5 × 12 = 60 | 60 × 0.41 = 24.6 s |
| TELEMETRY broadcast (~15 min) | 7 × 4 = 28 | 28 × 0.41 = 11.5 s |
| NEIGHBORINFO broadcast (6h interval) | 7 / 6 = 1.2 | 1.2 × 0.73 = 0.9 s |
| Traceroute (1 per node per 2h, staggered) | 7 / 2 × 2 = 7 | 7 × 1.06 = 7.4 s |
| **Total topology overhead** | ~125 packets | **~62.6 s / 3600 s = 1.7%** |

1.7% channel utilization for pure topology maintenance is within the acceptable range. Meshtastic firmware reports channel_utilization; anything above 25% is considered congested and the server should reduce active traceroute frequency.

### 10.4 Store-and-Forward Impact

A queued text message (200 bytes) consumes 1.87 seconds airtime per transmission attempt. With exponential backoff, a 5-attempt sequence over 4 hours adds 5 × 1.87 = 9.4 seconds of topology-attributed traffic, which is negligible against the 62.6 s/h background.

### 10.5 Topology Throttle Policy

If the firmware reports `channel_utilization > 20%`:
- Suspend active traceroutes entirely.
- Extend NEIGHBORINFO request interval from 6h to 12h.
- Defer Background-priority S&F messages.

If `channel_utilization > 30%`:
- Suspend Default-priority S&F messages.
- Emit `ChannelCongestion` signal (new `MeshDetail` variant, not defined above  -  add when implementing congestion handling).

This keeps kerykeion from amplifying congestion loops.

---

## 11. Implementation Notes for kerykeion

### Crate dependencies to add

```toml
[dependencies]
petgraph = { version = "0.8.3", features = ["serde-1"] }
```

`petgraph` 0.6 is stable and widely used. The `serde-1` feature enables graph serialization for fjall persistence.

### Key structures and their homes

| Structure | Module |
|-----------|--------|
| `MeshGraph`, `NodeRecord`, `LinkRecord` | `topology` |
| `QueuedMessage`, `MessagePriority` | `store_forward` |
| `GatewayHealth`, `GatewayClassification` | `gateway` |
| `TopologySnapshot`, `VisNode`, `VisEdge` | `snapshot` |
| `PartitionTracker` | `partition` |
| Signal emission helpers | `signals` |

### Invariants

- The graph is the single source of truth for node state. No node state lives outside it.
- Every graph mutation emits exactly one GeoSignal. No mutations are silent.
- The S&F queue is append-only from the insert path; the delivery path deletes. No in-place mutation.
- Traceroutes are rate-limited per target: at most one in-flight traceroute per destination at any time.
