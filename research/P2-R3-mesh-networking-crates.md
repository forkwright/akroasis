# P2-R3: Mesh networking crates for kerykeion

**Date:** 2026-03-18
**Scope:** Rust crate selection for kerykeion  -  the clean-room Meshtastic stack within Akroasis. Covers serial, BLE, protobuf, crypto, graph, discovery, and test infrastructure. Excludes the official `meshtastic` crate (0.1.8, GPL-3.0) per project constraints.

kerykeion (from Greek keryx, the herald's staff carried by Hermes) handles the physical-to-application boundary: receiving LoRa packets from Meshtastic radio hardware, decrypting and decoding them, and presenting structured mesh topology to the rest of Akroasis.

---

## Crate selection matrix

| Crate | Version | License | Pure Rust | MSRV | Maintained | Verdict | Notes |
|---|---|---|---|---|---|---|---|
| tokio-serial | 5.4.5 | MIT | No (libc/libudev) | unstated | Yes (2024-12) | **USE** | Wraps serialport 4.x |
| serialport | 4.9.0 | MPL-2.0 | No (libc/libudev) | 1.59 | Yes (2026-03) | USE (transitive) | MPL-2.0: file-level copyleft, AGPL compatible |
| tokio-util | 0.7.18 | MIT | Yes | 1.71 | Yes (2026-01) | **USE** | Framed codec for packet framing |
| btleplug | 0.12.0 | MIT/Apache-2.0/BSD-3-Clause | No (dbus FFI) | unstated | Yes (2026-03) | **USE** | Best GATT client API for Linux |
| bluer | 0.17.4 | BSD-2-Clause | No (dbus FFI) | 1.75 | Active (2025-06) | SKIP | Linux-only, worse API ergonomics |
| prost | 0.14.3 | Apache-2.0 | Yes | 1.82 | Yes (2026-01) | **USE** | Best protobuf codegen for Rust |
| prost-build | 0.14.3 | Apache-2.0 | Yes | 1.82 | Yes (2026-01) | **USE** | build.rs codegen companion |
| prost-types | 0.14.3 | Apache-2.0 | Yes | 1.82 | Yes (2026-01) | USE if needed | Well-known types; Meshtastic uses Timestamp in some protos |
| protobuf (stepancheg) | 4.34.0 | BSD-3-Clause | Yes | 1.79 | Yes (2026-02) | SKIP | Larger API surface, worse serde story |
| aes | 0.9.0-rc.4 | MIT OR Apache-2.0 | Yes | 1.85 | Yes (2026-02) | **USE** | RustCrypto, AES-128/256 block cipher |
| ctr | 0.10.0-rc.4 | MIT OR Apache-2.0 | Yes | 1.85 | Yes (2026-02) | **USE** | Ctr128LE matches Meshtastic nonce layout |
| ring | 0.17.x | BoringSSL-derived | Partial C | varies | Active | SKIP | No raw AES-CTR; AEAD only |
| x25519-dalek | 2.0.1 | BSD-3-Clause | Yes | 1.60 | Yes (2024-02) | **USE** | PKI direct message key exchange; 3.x series in pre-release |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | Yes | 1.64 | Yes (2025-09) | **USE** | StableGraph + Dijkstra covers mesh topology |
| libp2p | 0.56.0 | MIT | Yes | 1.83 | Yes (2025-06) | **DEFER** | Cannot run over LoRa; TCP/WiFi sync only |
| mdns-sd | 0.18.2 | Apache-2.0 OR MIT | Yes | 1.71 | Yes (2026-03) | **USE** | Pure Rust, no async runtime dependency |
| zeroconf | 0.17.0 | Non-standard | No (Avahi FFI) | unstated | Active (2025-11) | SKIP | Wraps Avahi/Bonjour C libraries |
| rusb | 0.9.4 | MIT | No (libusb FFI) | unstated | Active (2024-04) | **USE** | USB enumeration for serial port discovery |
| nix | 0.31.2 | MIT | No (libc wrappers) | 1.69 | Yes (2026-02) | **USE** | PTY pairs for serial port test mocks |
| proptest | 1.10.0 | MIT OR Apache-2.0 | Yes | 1.84 | Yes (2026-02) | **USE** | Property-based testing |

**Notes on rc versions:** `aes 0.9.0-rc.4` and `ctr 0.10.0-rc.4` are pre-release. The RustCrypto ecosystem coordinates releases; the rc versions target MSRV 1.85 and have been stable in practice for months. Pin to exact versions and update when stable releases land. `x25519-dalek 2.0.1` is the current stable release; the 3.x series has pre-releases in progress but no stable tag as of 2026-03-18.

---

## Section 1: Serial communication

### Recommendation

Use `tokio-serial 5.4.5` with `tokio-util 0.7.18` codec framing.

`tokio-serial` wraps `serialport 4.9.0` (the established synchronous serial library) with a Tokio async layer using `mio-serial` under the hood. Linux support for CP2102, CH340, and CH9102 USB serial chipsets is OS-driver level  -  all three present as `/dev/ttyUSBn` on Linux and work without special crate handling. Baud rate configuration, hardware flow control (RTS/CTS, DTR/DSR), and 8N1 framing are all supported through `serialport::SerialPortBuilder`.

`serialport` uses MPL-2.0. This is file-level copyleft: modifications to `serialport`'s own source files must be shared, but code that calls it is not affected. AGPL-3.0 is compatible with this.

### Meshtastic packet framing

Meshtastic's serial protocol uses a 4-byte header: `0x94`, `0xC3`, MSB of payload length, LSB of payload length. This wraps a protobuf-encoded `ToRadio` or `FromRadio` message. The decoder reads the two magic bytes, then a 16-bit big-endian length, then that many bytes of protobuf payload.

Implement a custom `tokio_util::codec::Decoder` to handle this framing:

```rust
use bytes::{Buf, BytesMut};
use snafu::{ResultExt, Snafu};
use tokio_util::codec::{Decoder, Encoder};

const MAGIC_1: u8 = 0x94;
const MAGIC_2: u8 = 0xC3;
const HEADER_LEN: usize = 4;
const MAX_PAYLOAD: usize = 512; // Meshtastic firmware limit

#[derive(Debug, Snafu)]
pub(crate) enum FrameError {
    #[snafu(display("payload length {len} exceeds maximum {MAX_PAYLOAD}"))]
    PayloadTooLarge { len: usize },
    #[snafu(display("I/O error reading frame"))]
    Io { source: std::io::Error },
}

impl From<std::io::Error> for FrameError {
    fn from(e: std::io::Error) -> Self {
        Self::Io { source: e }
    }
}

pub(crate) struct MeshtasticCodec;

impl Decoder for MeshtasticCodec {
    type Item = BytesMut;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Need at least the 4-byte header.
        if src.len() < HEADER_LEN {
            src.reserve(HEADER_LEN);
            return Ok(None);
        }

        // Scan for the 0x94 0xC3 magic bytes. Discard any leading garbage.
        let start = src
            .iter()
            .zip(src.iter().skip(1))
            .position(|(&a, &b)| a == MAGIC_1 && b == MAGIC_2);

        let start = match start {
            Some(pos) => pos,
            None => {
                // No magic found; discard all but last byte (partial match).
                let keep = src.len().saturating_sub(1);
                src.advance(keep);
                return Ok(None);
            }
        };

        if start > 0 {
            src.advance(start);
        }

        if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let payload_len = (src[2] as usize) << 8 | src[3] as usize;

        if payload_len > MAX_PAYLOAD {
            // Hard error: the frame is malformed or we lost sync.
            src.advance(2); // skip past the bad magic to resync next call
            return Err(FrameError::PayloadTooLarge { len: payload_len });
        }

        let total = HEADER_LEN + payload_len;
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(HEADER_LEN);
        let payload = src.split_to(payload_len);
        Ok(Some(payload))
    }
}

impl Encoder<bytes::Bytes> for MeshtasticCodec {
    type Error = std::io::Error;

    fn encode(&mut self, payload: bytes::Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let len = payload.len();
        dst.reserve(HEADER_LEN + len);
        dst.extend_from_slice(&[MAGIC_1, MAGIC_2, (len >> 8) as u8, len as u8]);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}
```

Open the port and wrap it with the codec:

```rust
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Framed;

pub(crate) async fn open_radio(
    path: &str,
) -> Result<Framed<tokio_serial::SerialStream, MeshtasticCodec>, FrameError> {
    let port = tokio_serial::new(path, 115_200)
        .timeout(std::time::Duration::from_millis(100))
        .open_native_async()
        .context(IoSnafu)?;

    Ok(Framed::new(port, MeshtasticCodec))
}
```

---

## Section 2: BLE communication

### Recommendation

Use `btleplug 0.12.0`.

Both `btleplug` and `bluer` use D-Bus to talk to BlueZ on Linux  -  neither is truly free of system daemon coupling. The choice comes down to API ergonomics and cross-platform future.

`btleplug` provides a `Central` trait for scanning and a `Peripheral` trait for GATT operations. Characteristic discovery, read/write, and notification subscribe all have clean async interfaces. The triple license (MIT/Apache-2.0/BSD-3-Clause) causes no AGPL complications. The 2026-03-09 update date confirms active maintenance.

`bluer` is the "official" BlueZ Rust binding, maintained by the BlueZ project itself. Its GATT client works but the API is more verbose (explicit `Session` and `Adapter` handles, manual service/characteristic path traversal). It is Linux-only with no path to cross-platform. MSRV 1.75 is fine for our targets, but the API overhead is not justified when `btleplug` already has better ergonomics. Skip it.

### Meshtastic BLE specifics

Meshtastic exposes three GATT characteristics on service UUID `6ba4xxxx-1200-11e4-9191-0800200c9a66`:

- `FromRadio` (UUID `6ba40003-...`): read and notify, contains `FromRadio` protobuf
- `ToRadio` (UUID `6ba40001-...`): write-without-response, contains `ToRadio` protobuf
- `FromNum` (UUID `6ba40002-...`): notify, u32 sequence counter indicating a new `FromRadio` is available

The MTU on BLE is typically 512 bytes after negotiation, sufficient for Meshtastic's maximum payload.

### BLE GATT connection example

```rust
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use snafu::{ResultExt, Snafu};
use std::time::Duration;
use tokio::time;
use uuid::Uuid;

const MESHTASTIC_SERVICE: Uuid =
    Uuid::from_u128(0x6ba4_1200_1200_11e4_9191_0800_200c_9a66);
const FROM_RADIO_CHAR: Uuid =
    Uuid::from_u128(0x6ba4_0003_1200_11e4_9191_0800_200c_9a66);
const TO_RADIO_CHAR: Uuid =
    Uuid::from_u128(0x6ba4_0001_1200_11e4_9191_0800_200c_9a66);
const FROM_NUM_CHAR: Uuid =
    Uuid::from_u128(0x6ba4_0002_1200_11e4_9191_0800_200c_9a66);

#[derive(Debug, Snafu)]
pub(crate) enum BleError {
    #[snafu(display("BLE manager error"))]
    Manager { source: btleplug::Error },
    #[snafu(display("no Meshtastic device found after scan"))]
    NotFound,
    #[snafu(display("GATT operation failed"))]
    Gatt { source: btleplug::Error },
}

pub(crate) struct MeshtasticBle {
    peripheral: Peripheral,
    from_radio: btleplug::api::Characteristic,
    to_radio: btleplug::api::Characteristic,
}

impl MeshtasticBle {
    pub(crate) async fn connect(name_prefix: &str) -> Result<Self, BleError> {
        let manager = Manager::new().await.context(ManagerSnafu)?;
        let adapters = manager.adapters().await.context(ManagerSnafu)?;
        let central = adapters.into_iter().next().ok_or(BleError::NotFound)?;

        central
            .start_scan(ScanFilter::default())
            .await
            .context(ManagerSnafu)?;
        time::sleep(Duration::from_secs(3)).await;
        central.stop_scan().await.context(ManagerSnafu)?;

        let target = central
            .peripherals()
            .await
            .context(ManagerSnafu)?
            .into_iter()
            .find(|p| {
                // Scan for devices whose local name starts with the prefix.
                // btleplug surfaces this via properties().await in a real scan loop;
                // simplified here for clarity.
                let _ = name_prefix;
                true // replace with actual name check from p.properties()
            })
            .ok_or(BleError::NotFound)?;

        target.connect().await.context(GattSnafu)?;
        target.discover_services().await.context(GattSnafu)?;

        let chars = target.characteristics();

        let from_radio = chars
            .iter()
            .find(|c| c.uuid == FROM_RADIO_CHAR)
            .cloned()
            .ok_or(BleError::NotFound)?;

        let to_radio = chars
            .iter()
            .find(|c| c.uuid == TO_RADIO_CHAR)
            .cloned()
            .ok_or(BleError::NotFound)?;

        let from_num = chars
            .iter()
            .find(|c| c.uuid == FROM_NUM_CHAR)
            .cloned()
            .ok_or(BleError::NotFound)?;

        // Subscribe to FromNum notifications so we know when new packets arrive.
        target.subscribe(&from_num).await.context(GattSnafu)?;

        Ok(Self {
            peripheral: target,
            from_radio,
            to_radio,
        })
    }

    pub(crate) async fn read_packet(&self) -> Result<Vec<u8>, BleError> {
        self.peripheral
            .read(&self.from_radio)
            .await
            .context(GattSnafu)
    }

    pub(crate) async fn write_packet(&self, payload: &[u8]) -> Result<(), BleError> {
        self.peripheral
            .write(&self.to_radio, payload, WriteType::WithoutResponse)
            .await
            .context(GattSnafu)
    }
}
```

---

## Section 3: Protobuf generation

### Recommendation

Use `prost 0.14.3` + `prost-build 0.14.3`. Add `prost-types 0.14.3` if any Meshtastic proto files use `google.protobuf.Timestamp` (the telemetry protos do).

`prost` generates clean Rust structs with `#[derive(Clone, PartialEq, prost::Message)]`. Enum handling maps proto enums to `i32` fields by default, with a companion `TryFrom<i32>` impl on the generated enum type. `oneof` fields become `Option<enum>`. The generated types work well in practice.

`stepancheg/rust-protobuf` (4.34.0) is also actively maintained but generates more verbose code, has a larger runtime dependency, and the serde story requires more ceremony. Skip it.

### MPL-2.0 note on serialport

`prost` is Apache-2.0. No license concern.

### build.rs configuration

Add `serde` derives to all generated types via `type_attribute` in `prost-build`. This lets koinon or upper crates serialize topology snapshots without manual From/Into impls:

```rust
// build.rs
use std::path::PathBuf;

fn main() {
    let proto_dir = PathBuf::from("proto");
    let protos: Vec<_> = std::fs::read_dir(&proto_dir)
        .expect("proto dir missing")
        .filter_map(|e| {
            let path = e.ok()?.path();
            (path.extension()? == "proto").then_some(path)
        })
        .collect();

    let proto_paths: Vec<&str> = protos.iter().map(|p| p.to_str().unwrap()).collect();

    prost_build::Config::new()
        // Add serde derives to every generated message type.
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        // Skip serializing None fields and default values.
        .type_attribute(".", "#[serde(default)]")
        // Represent bytes fields as base64 strings in JSON.
        .bytes_type(prost_build::BytesType::Vec)
        // Map the Meshtastic channel key field to a proper name.
        .field_attribute("Channel.psk", "#[serde(with = \"serde_bytes\")]")
        .compile_protos(&proto_paths, &[proto_dir])
        .expect("prost-build failed");

    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
}
```

Include the generated code in the crate:

```rust
// src/proto.rs
pub(crate) mod meshtastic {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}
```

Note: prost generates a flat module per proto package. If the Meshtastic `.proto` files use the `meshtastic` package declaration, the generated file is `meshtastic.rs`. Inspect `OUT_DIR` on first build to confirm filenames.

---

## Section 4: Cryptography

### Recommendation

Use `aes 0.9.0-rc.4` + `ctr 0.10.0-rc.4` from the RustCrypto ecosystem. Use `x25519-dalek 2.0.1` (stable) for PKI direct message key exchange.

### Meshtastic nonce format (authoritative)

The Meshtastic firmware (`CryptoEngine.cpp`) constructs the 16-byte AES-CTR nonce as:

```
bytes  0..7  : packetId as u64, little-endian
bytes  8..11 : fromNode as u32, little-endian
bytes 12..15 : extraNonce as u32, little-endian (0 for normal packets)
```

This is a 128-bit nonce used as the initial counter value. The intended layout uses a u64 packet ID (not u32).

**Firmware bug (confirmed in `CryptoEngine.cpp`):** The `extraNonce` branch writes to offset `sizeof(uint32_t)` (offset 4) instead of `sizeof(uint64_t) + sizeof(uint32_t)` (offset 12). When `extraNonce != 0`, it overwrites bytes 4–7 (the high word of `packetId`) rather than bytes 12–15. This is a latent firmware defect. For normal mesh packets `extraNonce` is always 0, so this bug is harmless in practice  -  the nonce layout above is correct for all packets kerykeion will receive.

### CTR variant selection

The `ctr` crate provides `Ctr32BE`, `Ctr32LE`, `Ctr64BE`, `Ctr64LE`, `Ctr128BE`, and `Ctr128LE`. Meshtastic initializes the entire 16-byte array as the counter start value and increments it as a 128-bit little-endian integer. Use `Ctr128LE<Aes128>`.

### Why not ring

`ring` exposes AES-GCM and ChaCha20-Poly1305 (AEAD constructions) but does not expose raw AES-CTR. Confirmed: not suitable.

### AES-CTR decryption example

```rust
use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher};
use ctr::Ctr128LE;
use snafu::{ensure, Snafu};

#[derive(Debug, Snafu)]
pub(crate) enum CryptoError {
    #[snafu(display("ciphertext is empty"))]
    EmptyCiphertext,
    #[snafu(display("channel key must be 16 bytes for AES-128"))]
    InvalidKeyLength,
}

/// Decrypt a Meshtastic AES-128-CTR payload in place.
///
/// `key` is the 16-byte channel PSK (padded to 16 bytes if shorter, per
/// Meshtastic firmware convention).
/// `packet_id` is the 64-bit packet identifier from the MeshPacket header.
/// `from_node` is the 32-bit node number of the sender.
pub(crate) fn decrypt_payload(
    ciphertext: &mut [u8],
    key: &[u8; 16],
    packet_id: u64,
    from_node: u32,
) -> Result<(), CryptoError> {
    ensure!(!ciphertext.is_empty(), EmptyCiphertextSnafu);

    // Build the 16-byte nonce: packetId (u64 LE) || fromNode (u32 LE) || 0x00 (u32)
    let mut nonce = [0u8; 16];
    nonce[0..8].copy_from_slice(&packet_id.to_le_bytes());
    nonce[8..12].copy_from_slice(&from_node.to_le_bytes());
    // bytes 12..15 remain zero (extraNonce default)

    let mut cipher = Ctr128LE::<Aes128>::new(key.into(), &nonce.into());
    cipher.apply_keystream(ciphertext);

    Ok(())
}

/// Pad a short PSK to 16 bytes, matching Meshtastic firmware behavior.
/// Meshtastic XORs the default key with the configured PSK bytes, but for
/// custom channels it pads with zeros if the PSK is shorter than the key size.
pub(crate) fn pad_psk_to_128(psk: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    let copy_len = psk.len().min(16);
    key[..copy_len].copy_from_slice(&psk[..copy_len]);
    key
}
```

### License check

All RustCrypto crates (`aes`, `ctr`) are MIT OR Apache-2.0. `x25519-dalek` is BSD-3-Clause. All are AGPL-3.0 compatible.

---

## Section 5: Graph and topology

### Recommendation

Use `petgraph 0.8.3` with `StableGraph`.

For mesh topology, the graph changes at runtime: nodes go offline (node removal) and links degrade or recover (edge weight updates). `StableGraph` preserves `NodeIndex` values across removals, which matters when kerykeion maps node numbers to indices and other parts of the system hold those indices.

Regular `Graph` reuses indices after removal. If a node goes offline and a new node comes online, its index could collide with old references held elsewhere. Use `StableGraph` to avoid this.

`graphlib` does not support stable node handles or Dijkstra. Skip it.

### Force-directed layout

No mature pure-Rust force-directed layout crate exists. The Fruchterman-Reingold algorithm is ~50 lines of Rust. Implement it directly in kerykeion's UI layer. At under 100 nodes, performance is not a concern.

### Petgraph mesh topology example

```rust
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::algo::dijkstra;
use petgraph::Directed;
use std::collections::HashMap;

/// Edge weight: SNR value in dB, higher is better.
/// Dijkstra minimizes cost, so store cost = -snr or use a separate
/// representation. Here we use cost = (max_snr - snr) to keep weights positive.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkWeight {
    /// Signal-to-noise ratio in dB (raw, for display).
    pub snr_db: f32,
    /// Routing cost: lower is better. Derived as max_snr - snr_db.
    pub cost: f32,
}

impl LinkWeight {
    pub(crate) fn from_snr(snr_db: f32) -> Self {
        // Meshtastic reports SNR in range roughly -20..+10 dB.
        // Shift so cost is always positive: cost = 30 - snr_db.
        Self {
            snr_db,
            cost: 30.0 - snr_db,
        }
    }
}

pub(crate) struct MeshTopology {
    graph: StableGraph<u32, LinkWeight, Directed>,
    node_map: HashMap<u32, NodeIndex>,
}

impl MeshTopology {
    pub(crate) fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            node_map: HashMap::new(),
        }
    }

    pub(crate) fn add_node(&mut self, node_num: u32) -> NodeIndex {
        *self.node_map.entry(node_num).or_insert_with(|| {
            self.graph.add_node(node_num)
        })
    }

    pub(crate) fn upsert_link(&mut self, from: u32, to: u32, snr_db: f32) {
        let from_idx = self.add_node(from);
        let to_idx = self.add_node(to);
        let weight = LinkWeight::from_snr(snr_db);

        // Remove existing edge before inserting updated one.
        if let Some(edge) = self.graph.find_edge(from_idx, to_idx) {
            self.graph.remove_edge(edge);
        }
        self.graph.add_edge(from_idx, to_idx, weight);
    }

    pub(crate) fn remove_node(&mut self, node_num: u32) {
        if let Some(idx) = self.node_map.remove(&node_num) {
            self.graph.remove_node(idx);
        }
    }

    /// Return minimum-cost path from `from` to `to` as ordered node numbers.
    pub(crate) fn shortest_path(
        &self,
        from: u32,
        to: u32,
    ) -> Option<Vec<u32>> {
        let from_idx = *self.node_map.get(&from)?;
        let to_idx = *self.node_map.get(&to)?;

        let costs = dijkstra(
            &self.graph,
            from_idx,
            Some(to_idx),
            |e| e.weight().cost,
        );

        // Dijkstra gives costs but not the path itself. For path reconstruction
        // use petgraph::algo::astar or maintain predecessors separately.
        // For now: confirm reachability only.
        costs.contains_key(&to_idx).then(|| {
            vec![from, to] // placeholder; full path reconstruction omitted
        })
    }
}
```

For full path reconstruction, use `petgraph::algo::astar` instead of `dijkstra`. `astar` returns the path as a `Vec<NodeIndex>` alongside the cost.

---

## Section 6: libp2p assessment

### Verdict: defer to post-Wave-4

**Do not include libp2p in the initial kerykeion implementation.**

The reason is simple: Meshtastic transport is LoRa. LoRa packets are 237 bytes maximum, at roughly 1 kbps effective throughput. libp2p's protocols (Kademlia DHT, gossipsub, Noise handshake) assume TCP-grade connections. The Kademlia routing table alone would consume multiple LoRa packets just to exchange hello messages.

libp2p cannot and should not run over the LoRa transport.

Where libp2p becomes relevant is server-to-server synchronization: multiple Akroasis installations exchanging topology state over TCP/WiFi. This is a distributed deployment concern, not a Wave-4 concern. Wave 4 delivers a single-node Meshtastic stack. Distributed sync comes later.

When that work begins, `libp2p 0.56.0` (MIT, MSRV 1.83) is the right foundation. It provides mDNS, gossipsub for state broadcast, and both TCP and QUIC transports. The MSRV of 1.83 is below our 1.85 floor, which is fine.

For now: no libp2p dependency in kerykeion. Revisit in the wave that adds multi-node Akroasis deployment.

---

## Section 7: Discovery strategy

### USB serial is the reliable path

USB serial (CP2102N, CH9102, CH340) connects reliably at 115,200 baud and does not require pairing, daemon state, or WiFi. BLE is the fallback for devices that do not expose serial (older Meshtastic devices, stock firmware with BLE-only). mDNS/TCP is available on newer firmware with WiFi enabled.

### Known VID:PID pairs

| Chipset | VID | PID | Common on |
|---|---|---|---|
| Silicon Labs CP2102N | 10C4 | EA60 | RAK4631, most DIY devices |
| WCH CH9102 | 1A86 | 55D4 | T-Beam Supreme, HTCC-AB02 |
| WCH CH340 | 1A86 | 7523 | T-Beam v0.7, older boards |

`rusb 0.9.4` wraps `libusb1` via C FFI. It is not pure Rust. For production use, build with `libusb` statically linked or ensure it is present on target systems. On Getac rugged laptops running the standard Akroasis Linux image, `libusb` is a safe assumption.

### Discovery strategy

```
fn discover_meshtastic_transport() -> Result<Transport, DiscoveryError>:

1. Enumerate USB devices via rusb.
   For each device:
     if (vid, pid) in KNOWN_CHIPSETS:
       path = resolve_serial_path(vid, pid)  // /dev/ttyUSBn via sysfs
       if open_serial(path, 115_200) succeeds:
         return Transport::Serial(path)

2. Query mDNS for "_meshtastic._tcp.local." services.
   Timeout: 2 seconds.
   For each advertised service:
     (host, port) = service.address()
     if tcp_connect(host, port) succeeds:
       return Transport::Tcp(host, port)

3. Scan BLE for 5 seconds.
   Filter peripherals advertising service UUID 6ba4xxxx-1200-11e4-9191-0800200c9a66.
   Connect to first match.
   return Transport::Ble(peripheral)

4. Return DiscoveryError::NoDeviceFound
```

Priority rationale: Serial is deterministic and does not require scanning time. mDNS is faster than BLE scanning (2 seconds vs 5 seconds) and TCP is lower latency than BLE GATT. BLE is last because scanning takes the most time and GATT operations have higher overhead.

Note: Meshtastic firmware 2.x advertises `_meshtastic._tcp` when WiFi is enabled. Confirm the exact service type against target firmware version; some builds use `_http._tcp` with a Meshtastic-specific TXT record.

---

## Section 8: Testing infrastructure

### PTY pairs for serial port tests

`nix 0.31.2` provides `nix::pty::openpty`, which creates a linked PTY pair. Write to one end, read from the other. This lets tests exercise the full `MeshtasticCodec` path without hardware:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nix::pty::{openpty, OpenptyResult};
    use tokio::io::AsyncWriteExt;
    use tokio_serial::SerialPortBuilderExt;
    use tokio_util::codec::FramedRead;
    use futures::StreamExt;

    #[tokio::test]
    async fn codec_decodes_valid_frame() {
        // Open a PTY pair. master is the "device", slave is the port kerykeion opens.
        let OpenptyResult { master, slave } = openpty(None, None).unwrap();

        let slave_path = unsafe {
            std::ffi::CStr::from_ptr(nix::pty::ptsname_r(&slave).unwrap().as_ptr())
                .to_str()
                .unwrap()
                .to_owned()
        };

        // Write a valid Meshtastic frame to the master end.
        let payload = b"\x01\x02\x03"; // fake 3-byte protobuf
        let frame = [&[0x94u8, 0xC3, 0x00, payload.len() as u8][..], payload].concat();

        let mut master_file = tokio::fs::File::from_std(
            unsafe { std::fs::File::from_raw_fd(master.into_raw_fd()) },
        );
        master_file.write_all(&frame).await.unwrap();

        // Open the slave end as a serial port and decode.
        let port = tokio_serial::new(&slave_path, 115_200)
            .open_native_async()
            .unwrap();
        let mut framed = FramedRead::new(port, MeshtasticCodec);

        let decoded = framed.next().await.unwrap().unwrap();
        assert_eq!(decoded.as_ref(), payload);
    }
}
```

### Deterministic async timing

`tokio::time::pause()` stops Tokio's internal clock. Advance it manually with `tokio::time::advance(Duration)`. Use this for timeout tests (e.g., reconnect backoff, scan timeout expiry) without actual sleeps:

```rust
#[tokio::test]
async fn reconnect_backs_off() {
    tokio::time::pause();
    // ... set up a mock that fails N times ...
    tokio::time::advance(Duration::from_secs(30)).await;
    // assert the reconnect attempted the correct number of times
}
```

### proptest for packet fuzzing

`proptest 1.10.0` generates arbitrary inputs and shrinks failures automatically. Use it to ensure the codec never panics on arbitrary byte sequences:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn codec_never_panics_on_arbitrary_bytes(data in any::<Vec<u8>>()) {
        let mut buf = bytes::BytesMut::from(data.as_slice());
        let mut codec = MeshtasticCodec;
        // The codec should return Ok(None), Ok(Some(_)), or Err(_), never panic.
        let _ = codec.decode(&mut buf);
    }

    #[test]
    fn codec_decodes_roundtrip(payload_len in 1usize..256) {
        let payload: Vec<u8> = (0..payload_len).map(|i| i as u8).collect();
        let mut codec = MeshtasticCodec;

        // Encode.
        let mut buf = bytes::BytesMut::new();
        codec.encode(bytes::Bytes::copy_from_slice(&payload), &mut buf).unwrap();

        // Decode.
        let result = codec.decode(&mut buf).unwrap().unwrap();
        prop_assert_eq!(result.as_ref(), payload.as_slice());
    }
}
```

### Mock BLE

No purpose-built mock BLE crate exists for Rust. The practical approach: define a `RadioTransport` trait over `MeshtasticBle` and write a `MockRadioTransport` that returns canned packets from a `VecDeque`. This covers application logic tests without requiring BLE hardware or a running BlueZ daemon.

### CI strategy

The full test suite runs on standard CI (GitHub Actions, Ubuntu runner) without hardware:

- Serial tests: PTY pairs via `nix::pty::openpty` (available in Linux CI)
- Crypto tests: deterministic, no hardware
- Codec tests: proptest fuzz suite, no hardware
- Graph tests: deterministic
- BLE tests: mock transport only; real BLE tests are `#[ignore]` and run on hardware manually
- `tokio::time::pause()` throughout async tests: no real sleeps in CI

---

## Section 9: kerykeion Cargo.toml

Complete `[dependencies]` block with version pins and feature flags:

```toml
[package]
name = "kerykeion"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "AGPL-3.0-only"

[dependencies]
# -- Core async runtime --
tokio = { version = "1.44", features = ["rt-multi-thread", "net", "io-util", "sync", "time", "macros"] }
tokio-util = { version = "0.7.18", features = ["codec"] }

# -- Serial communication --
# tokio-serial wraps serialport (MPL-2.0) and mio-serial.
# serialport MPL-2.0 is file-level copyleft; compatible with AGPL-3.0.
tokio-serial = { version = "5.4.5", default-features = false }

# -- BLE communication --
# btleplug uses dbus FFI on Linux; not pure Rust, acceptable for our targets.
btleplug = { version = "0.12.0", features = [] }
uuid = { version = "1", features = ["v4"] }

# -- Protobuf --
prost = { version = "0.14.3" }
prost-types = { version = "0.14.3" }  # Timestamp in telemetry protos

# -- Cryptography (RustCrypto) --
# Pin rc versions until stable releases land; both target MSRV 1.85.
aes = { version = "=0.9.0-rc.4" }
ctr = { version = "=0.10.0-rc.4" }
# PKI direct messaging (Wave 4+).
x25519-dalek = { version = "2.0.1", features = ["static_secrets"] }

# -- Graph / topology --
petgraph = { version = "0.8.3", features = ["serde-1"] }

# -- Service discovery --
mdns-sd = { version = "0.18.2" }
# rusb wraps libusb; requires libusb1-dev on build host.
rusb = { version = "0.9.4" }

# -- Shared types and error handling --
# (from workspace)
koinon = { path = "../koinon" }
kryphos = { path = "../kryphos" }
snafu = { version = "0.8", features = ["backtraces"] }
bytes = "1"
tokio-stream = "0.1"
futures = "0.3"
tracing = "0.1"

# -- Serde for serializing topology snapshots --
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
prost-build = { version = "0.14.3" }

[dev-dependencies]
proptest = { version = "1.10.0" }
nix = { version = "0.31.2", features = ["pty"] }
tokio = { version = "1.44", features = ["test-util"] }  # tokio::time::pause

[features]
default = []
# Enable BLE transport. Disabled in environments without BlueZ.
ble = []
# Enable USB discovery via rusb. Disabled if libusb unavailable.
usb-discovery = []
```

### Version pinning notes

- `aes`, `ctr`: exact pins (`=`) because pre-release. Update to stable when 0.9.x/0.10.x land.
- `x25519-dalek 2.0.1`: stable; use without `=` pin. Upgrade to 3.x when that series stabilizes.
- `prost` at 0.14.3: MSRV 1.82, fine for our 1.85 floor.
- `libp2p` absent: deferred until distributed deployment work begins.
- `bluer` absent: `btleplug` covers the same need with better ergonomics.
- `zeroconf` absent: wraps Avahi C library; `mdns-sd` covers the need in pure Rust.
