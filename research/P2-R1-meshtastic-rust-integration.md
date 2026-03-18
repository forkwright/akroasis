# P2-R1: Meshtastic Rust Integration

**Phase:** Wave 4 (kerykeion)
**Blocks:** P2-01, P2-02
**Date:** 2026-03-18
**Depends on:** P2-R3 (crate selection matrix, authoritative versions and crypto details)

---

## Summary

kerykeion talks to Meshtastic hardware over serial (primary), TCP, or BLE. This document covers the Rust implementation of that interface: crate selection, frame codec design, protobuf code generation, AES-CTR encryption, node database schema, connection state machine, and test harness architecture.

Key decisions:

| Concern | Decision |
|---|---|
| Serial I/O | `tokio-serial 5.4.5` over raw `serialport-rs` |
| BLE I/O | `btleplug 0.12.0` (see P2-R3 §2 for full BLE detail) |
| Frame codec | `tokio_util::Codec` with magic-byte sync and rejection at 512 bytes |
| Protobuf | `prost 0.14.3` + `prost-build 0.14.3` from pinned proto tag |
| AES-CTR | RustCrypto `aes 0.9.0-rc.4` + `ctr 0.10.0-rc.4`, **`Ctr128LE`** |
| PKI | `x25519-dalek 3.0.0-pre.6` for Curve25519 ECDH |
| NodeDB | in-memory `HashMap<NodeId, NodeRecord>` + fjall persistence |
| Discovery | USB VID:PID via `rusb 0.9.4`, mDNS via `mdns-sd 0.18.2`, BLE fallback |
| Testing | Unix PTY pair (via `nix 0.31.2`) as mock serial device; `proptest 1.10.0` for fuzzing |

---

## 1. Serial Protocol Implementation

### 1.1 Crate Selection: tokio-serial

`tokio-serial 5.4.5` (MIT) wraps `serialport 4.9.0` (MPL-2.0) and exposes an async `SerialStream` implementing `AsyncRead + AsyncWrite`. It is the correct choice for kerykeion because:

- The entire stack runs on Tokio. A blocking `serialport-rs` call requires `spawn_blocking`, adding a thread-pool hop and complicating backpressure.
- `tokio-serial` feeds directly into `tokio_util::codec::Framed`, so the frame codec described in §1.3 composes without glue code.
- Linux support is complete. DTR/RTS pin control is available via the `SerialPort` trait from the underlying `serialport` crate, accessible through `.get_ref()`.

`serialport` uses MPL-2.0 (file-level copyleft). Code that calls it is not subject to MPL; AGPL-3.0 is compatible.

`serialport-rs` alone is appropriate only if the connection is managed by a dedicated thread. That model adds unnecessary complexity when Tokio handles the runtime.

### 1.2 Opening a Port

```rust
use tokio_serial::{SerialPortBuilderExt, SerialStream};

pub(crate) fn open_device(path: &str) -> Result<SerialStream, SerialError> {
    tokio_serial::new(path, 115_200)
        .data_bits(tokio_serial::DataBits::Eight)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .flow_control(tokio_serial::FlowControl::None)
        .timeout(std::time::Duration::from_millis(100))
        .open_native_async()
        .context(OpenSnafu { path })
}
```

DTR assertion (required by some T-Echo firmware builds to signal host presence):

```rust
use serialport::SerialPort as _;

stream.get_mut().write_data_terminal_ready(true).context(DtrSnafu)?;
```

### 1.3 Frame Codec

The wire format: `0x94 0xC3` (magic) + 2-byte big-endian length + protobuf payload. Maximum payload is 512 bytes; the firmware rejects larger frames.

```rust
use bytes::{Buf, BufMut, Bytes, BytesMut};
use snafu::Snafu;
use tokio_util::codec::{Decoder, Encoder};

pub(crate) struct MeshFrameCodec;

const MAGIC: [u8; 2] = [0x94, 0xC3];
const HEADER_LEN: usize = 4;
const MAX_PAYLOAD: usize = 512;

#[derive(Debug, Snafu)]
pub(crate) enum CodecError {
    #[snafu(display("payload length {len} exceeds maximum {MAX_PAYLOAD}"))]
    OversizedFrame { len: usize },
    #[snafu(display("I/O error"))]
    Io { source: std::io::Error },
}

impl From<std::io::Error> for CodecError {
    fn from(e: std::io::Error) -> Self {
        Self::Io { source: e }
    }
}

impl Decoder for MeshFrameCodec {
    type Item = Bytes;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Scan forward to magic bytes, discarding corrupt or stale data.
        let start = src
            .iter()
            .zip(src.iter().skip(1))
            .position(|(&a, &b)| a == MAGIC[0] && b == MAGIC[1]);

        let start = match start {
            Some(pos) => pos,
            None => {
                // No magic found; discard all but last byte (could be first byte of magic).
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

        let payload_len = u16::from_be_bytes([src[2], src[3]]) as usize;

        if payload_len > MAX_PAYLOAD {
            // Bad header. Skip past magic to resync.
            src.advance(2);
            return Err(CodecError::OversizedFrame { len: payload_len });
        }

        let total = HEADER_LEN + payload_len;
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(HEADER_LEN);
        Ok(Some(src.split_to(payload_len).freeze()))
    }
}

impl Encoder<Bytes> for MeshFrameCodec {
    type Error = CodecError;

    fn encode(&mut self, payload: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if payload.len() > MAX_PAYLOAD {
            return Err(CodecError::OversizedFrame { len: payload.len() });
        }
        dst.put_slice(&MAGIC);
        dst.put_u16(payload.len() as u16);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}
```

Wire into a `Framed`:

```rust
use tokio_util::codec::Framed;

let framed = Framed::new(serial_stream, MeshFrameCodec);
let (mut sink, mut stream) = framed.split();
```

**Partial read handling:** `BytesMut` accumulates bytes across poll cycles. `decode` returns `Ok(None)` whenever the buffer does not hold a complete frame; Tokio calls it again when more bytes arrive. No manual reassembly needed.

**Corrupt magic:** The scan loop advances through garbage until the magic pair is found or the buffer is exhausted. Count discarded bytes with a metrics counter for hardware diagnostics.

### 1.4 Reconnection

T-Echo and T-Deck disconnect when USB suspends or the cable is pulled. Exponential backoff with a cap:

```rust
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;

const BASE_BACKOFF: Duration = Duration::from_millis(200);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

pub(crate) async fn connection_loop(
    path: String,
    cancel: CancellationToken,
    tx: tokio::sync::mpsc::Sender<RadioEvent>,
) {
    let mut backoff = BASE_BACKOFF;

    loop {
        match open_device(&path) {
            Ok(stream) => {
                backoff = BASE_BACKOFF; // reset on success
                let result = run_session(stream, cancel.clone(), tx.clone()).await;
                if cancel.is_cancelled() {
                    return;
                }
                tracing::warn!(reason = ?result, "session ended, reconnecting");
            }
            Err(e) => {
                tracing::debug!(error = %e, delay = ?backoff, "port open failed");
            }
        }

        tokio::select! {
            _ = sleep(backoff) => {}
            _ = cancel.cancelled() => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}
```

For udev-triggered reconnect (when koinon's hardware registry detects device arrival), send a wakeup on a separate channel alongside the backoff timer in the `select!`.

### 1.5 Connection Lifecycle

**Config handshake:**

1. Open serial port.
2. Send `ToRadio { want_config_id: <random u32> }`.
3. Consume `FromRadio` messages until `FromRadio::ConfigCompleteId(id)` matches the sent id.
4. Transition to `Ready`.

The firmware sends in order during handshake: `MyNodeInfo`, N × `NodeInfo`, multiple `Config`, multiple `ModuleConfig`, N × `Channel`, then `ConfigCompleteId`. All must be processed before declaring ready.

**Heartbeat:** Send `ToRadio { packet: None }` (empty ToRadio) every 180 seconds to keep the USB connection alive. Absence of any `FromRadio` for > 300 seconds indicates a stale connection.

**Stale detection:**

```rust
use tokio::time::{sleep_until, Instant};

const STALE_THRESHOLD: Duration = Duration::from_secs(300);

// In the receive loop:
let mut deadline = Instant::now() + STALE_THRESHOLD;
loop {
    tokio::select! {
        frame = stream.next() => {
            // process frame; reset deadline
            deadline = Instant::now() + STALE_THRESHOLD;
        }
        _ = sleep_until(deadline) => {
            return Err(SessionError::StaleConnection);
        }
    }
}
```

**Device reboot mid-stream:** The magic-byte sync loop handles garbled output during reboot. After full reboot, the device stops sending. The stale timeout fires, the session closes, and the reconnect loop opens a fresh connection. Re-handshake is identical to initial handshake.

---

## 2. Protobuf Code Generation

### 2.1 Proto File Set

Pin to a specific release tag of the `meshtastic/protobufs` repository. For firmware 2.5.x, use the matching `v2.5.x` tag. Do not track HEAD; firmware and proto schema must stay in sync.

Vendor into `crates/kerykeion/proto/meshtastic/`:

| File | Contents |
|---|---|
| `mesh.proto` | `MeshPacket`, `FromRadio`, `ToRadio`, `MyNodeInfo`, `NodeInfo`, `User`, `Position`, `Routing`, `RouteDiscovery` |
| `config.proto` | `Config` (device, LoRa, Bluetooth, network, display, position, power) |
| `module_config.proto` | `ModuleConfig` (MQTT, serial, store-forward, telemetry, canned message, neighbor info, etc.) |
| `channel.proto` | `Channel`, `ChannelSettings` |
| `admin.proto` | `AdminMessage` |
| `telemetry.proto` | `Telemetry`, `DeviceMetrics`, `EnvironmentMetrics`, `PowerMetrics`, `AirQualityMetrics` |
| `portnums.proto` | `PortNum` enum |
| `storeforward.proto` | `StoreAndForward` |
| `waypoint.proto` | `Waypoint` |
| `connection_status.proto` | `DeviceConnectionStatus`, `NetworkInterfaces` |

Skip `xmodem.proto` (firmware update; not needed in kerykeion v1).

Layout:

```
crates/kerykeion/
  proto/
    meshtastic/           ← vendored at tag v2.5.x, committed to VCS
      mesh.proto
      config.proto
      ...
  src/
    proto.rs              ← include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"))
  build.rs
```

Generated code goes to `OUT_DIR` (Cargo's standard artifact directory). Do not put generated files in `src/`; they are not source and must not be committed.

### 2.2 Build Script

```rust
// crates/kerykeion/build.rs

use std::path::PathBuf;

fn main() {
    let proto_dir = PathBuf::from("proto/meshtastic");

    let protos: Vec<_> = [
        "mesh",
        "config",
        "module_config",
        "channel",
        "admin",
        "telemetry",
        "portnums",
        "storeforward",
        "waypoint",
        "connection_status",
    ]
    .iter()
    .map(|name| proto_dir.join(format!("{name}.proto")))
    .collect();

    let proto_str: Vec<&str> = protos
        .iter()
        .map(|p| p.to_str().expect("proto path is valid UTF-8"))
        .collect();

    let mut config = prost_build::Config::new();
    config
        .extern_path(".google.protobuf", "::prost_types")
        // serde derives on all generated types.
        .type_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .type_attribute(".", "#[serde(default)]");

    // build.rs panics on failure: compilation cannot proceed without generated
    // types. The #[expect(clippy::expect_used)] attribute is acceptable here.
    #[expect(clippy::expect_used, reason = "build failure must abort compilation")]
    config
        .compile_protos(&proto_str, &[proto_dir.to_str().expect("valid UTF-8")])
        .expect("protobuf compilation failed");

    println!("cargo:rerun-if-changed=proto/meshtastic");
}
```

Include the generated module:

```rust
// src/proto.rs
pub(crate) mod meshtastic {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}
```

Inspect `OUT_DIR` on first build to confirm the exact generated filename. If the `.proto` files declare `package meshtastic;`, the output is `meshtastic.rs`.

### 2.3 Type Wrappers

prost generates `i32` for proto enums and raw structs for oneofs. Wrap at the domain boundary.

**PortNum:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PortNum {
    TextMessage,
    RemoteHardware,
    Position,
    NodeInfo,
    Routing,
    Admin,
    IpTunnel,
    Paxcounter,
    Serial,
    Telemetry,
    Zps,
    Simulator,
    TraceRoute,
    Neighborinfo,
    Atak,
    Map,
    StoreForward,
    RangeTest,
    Private(i32),  // 256–511: application-defined
    Unknown(i32),
}

impl From<i32> for PortNum {
    fn from(v: i32) -> Self {
        use crate::proto::meshtastic::PortNum as P;
        match P::try_from(v) {
            Ok(P::TextMessageApp) => Self::TextMessage,
            Ok(P::RemoteHardwareApp) => Self::RemoteHardware,
            Ok(P::PositionApp) => Self::Position,
            Ok(P::NodeinfoApp) => Self::NodeInfo,
            Ok(P::RoutingApp) => Self::Routing,
            Ok(P::AdminApp) => Self::Admin,
            Ok(P::TelemetryApp) => Self::Telemetry,
            Ok(P::NeighborinfoApp) => Self::Neighborinfo,
            Ok(P::StoreAndForwardApp) => Self::StoreForward,
            _ if (256..=511).contains(&v) => Self::Private(v),
            _ => Self::Unknown(v),
        }
    }
}
```

**MeshPacket payload:** The proto `oneof payload_variant` generates `Option<mesh_packet::PayloadVariant>`. Wrap with a cleaner API:

```rust
pub(crate) enum PacketPayload {
    /// Unencrypted or already-decrypted application data.
    Decoded { port: PortNum, data: bytes::Bytes },
    /// Encrypted bytes. Caller must decrypt before use.
    Encrypted(bytes::Bytes),
}
```

### 2.4 Version Compatibility

prost ignores unknown fields on decode, so newer firmware messages do not break older compiled code. The risk runs the other direction: sending an `AdminMessage` compiled against a newer proto to firmware that does not understand the field. Document the minimum firmware version for each `AdminMessage` variant.

---

## 3. AES-CTR Encryption

### 3.1 Crate Selection

Use RustCrypto `aes 0.9.0-rc.4` + `ctr 0.10.0-rc.4`. Do not use `ring`:

- `ring` provides AES-GCM and ChaCha20-Poly1305 (AEAD), not raw AES-CTR.
- Meshtastic uses raw AES-CTR (no authentication tag). Payload integrity is implicitly guaranteed by protobuf structure and the channel PSK being a shared secret.
- RustCrypto `aes` supports AES-128 (16-byte PSK) and AES-256 (32-byte PSK) with the same API.
- All RustCrypto crates are MIT OR Apache-2.0.

These are pre-release (rc) versions. Pin with exact version constraints (`=0.9.0-rc.4`). Update to stable releases when they land; the RustCrypto ecosystem coordinates simultaneous releases.

### 3.2 CTR Variant: Ctr128LE

**Use `Ctr128LE`, not `Ctr128BE`.**

The `ctr` crate provides `Ctr32BE`, `Ctr32LE`, `Ctr64BE`, `Ctr64LE`, `Ctr128BE`, and `Ctr128LE`. Meshtastic's `CryptoEngine.cpp` initializes the entire 16-byte nonce array as the counter start value and increments it as a 128-bit little-endian integer. `Ctr128LE<Aes128>` matches this behavior.

### 3.3 Nonce Construction

From `CryptoEngine.cpp`:

```
bytes  0..7  : packetId as u64, little-endian (MeshPacket.id is u32, zero-extended to u64)
bytes  8..11 : fromNode as u32, little-endian
bytes 12..15 : extraNonce as u32, little-endian (0x00000000 for normal packets)
```

```rust
fn build_nonce(packet_id: u64, from_node: u32) -> [u8; 16] {
    let mut nonce = [0u8; 16];
    nonce[0..8].copy_from_slice(&packet_id.to_le_bytes());
    nonce[8..12].copy_from_slice(&from_node.to_le_bytes());
    // bytes 12..15 remain zero (extraNonce default)
    nonce
}
```

`MeshPacket.id` is `uint32` in the proto but the firmware casts it to `u64` for nonce construction. Pass `packet_id as u64` at the call site.

### 3.4 AES-128 Decryption (16-byte PSK)

```rust
use aes::Aes128;
use ctr::cipher::{KeyIvInit, StreamCipher};
use ctr::Ctr128LE;

/// Decrypt or encrypt in place. AES-CTR is symmetric.
pub(crate) fn crypt_payload(
    payload: &mut [u8],
    psk: &[u8; 16],
    packet_id: u64,
    from_node: u32,
) {
    let nonce = build_nonce(packet_id, from_node);
    let mut cipher = Ctr128LE::<Aes128>::new(psk.into(), &nonce.into());
    cipher.apply_keystream(payload);
}
```

### 3.5 AES-256 Decryption (32-byte PSK)

```rust
use aes::Aes256;

pub(crate) fn crypt_payload_256(
    payload: &mut [u8],
    psk: &[u8; 32],
    packet_id: u64,
    from_node: u32,
) {
    let nonce = build_nonce(packet_id, from_node);
    let mut cipher = Ctr128LE::<Aes256>::new(psk.into(), &nonce.into());
    cipher.apply_keystream(payload);
}
```

Dispatch on PSK length:

```rust
pub(crate) fn decrypt(
    payload: &mut [u8],
    psk: &[u8],
    packet_id: u64,
    from_node: u32,
) -> Result<(), CryptoError> {
    match psk.len() {
        16 => {
            let key: &[u8; 16] = psk.try_into().context(KeyLengthSnafu { len: 16 })?;
            crypt_payload(payload, key, packet_id, from_node);
        }
        32 => {
            let key: &[u8; 32] = psk.try_into().context(KeyLengthSnafu { len: 32 })?;
            crypt_payload_256(payload, key, packet_id, from_node);
        }
        len => return Err(CryptoError::InvalidKeyLength { len }),
    }
    Ok(())
}
```

### 3.6 PSK Resolution

The `ChannelSettings.psk` field holds raw PSK bytes. Three cases:

**Default channel key** (`psk == [0x01]`): expand to the well-known 16-byte default key. Verify the exact bytes against the pinned firmware tag's `Default.h` constant `DEFAULT_PSK` — do not hardcode from memory.

```rust
/// Meshtastic default channel key, expanded from PSK byte 0x01.
/// Source: meshtastic/firmware Default.h, DEFAULT_PSK constant.
/// VERIFY these bytes against the pinned firmware tag before shipping.
pub(crate) const DEFAULT_PSK: [u8; 16] = [
    0xd4, 0xf1, 0xbb, 0x3a, 0x20, 0x29, 0x07, 0x59,
    0xf0, 0xbc, 0xff, 0xab, 0xcf, 0x4e, 0x69, 0x03,
];
```

**No encryption** (`psk` is empty): payload is plaintext.

**Custom PSK** (≤ 16 bytes padded to 16, or 32 bytes for AES-256): use raw bytes. Pad shorter PSKs with zeros per firmware convention:

```rust
pub(crate) fn pad_psk_to_128(psk: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    let n = psk.len().min(16);
    key[..n].copy_from_slice(&psk[..n]);
    key
}
```

PSK storage: kryphos vault stores channel PSKs keyed by channel index. kerykeion accesses them through a `PskResolver` trait to avoid a direct dependency on kryphos internals:

```rust
pub(crate) trait PskResolver: Send + Sync {
    fn psk_for_channel(&self, index: u8) -> Option<Psk>;
}

pub(crate) enum Psk {
    Key128([u8; 16]),
    Key256([u8; 32]),
    None,
}
```

### 3.7 Multi-Channel Decryption

For broadcasts on an unknown channel, try each registered channel's PSK. A successful decrypt produces parseable protobuf; an invalid decode means the wrong key. This is a heuristic — AES-CTR has no authentication tag. A valid `Data::decode` is necessary but not sufficient; in practice, false positives are rare.

```rust
pub(crate) fn decrypt_any_channel(
    payload: &[u8],
    packet_id: u64,
    from_node: u32,
    resolver: &dyn PskResolver,
) -> Option<(u8, Data)> {
    for channel_index in 0..8u8 {
        let Some(psk) = resolver.psk_for_channel(channel_index) else { continue };
        let mut buf = payload.to_vec();
        let _ = decrypt(&mut buf, psk.as_bytes(), packet_id, from_node);
        if let Ok(data) = Data::decode(buf.as_slice()) {
            return Some((channel_index, data));
        }
    }
    None
}
```

### 3.8 PKI Encryption (v2.5+)

Direct messages use PKI encryption when `MeshPacket.pki_encrypted` is set.

Key exchange:
1. Each node broadcasts its Curve25519 public key in `User.public_key`.
2. Sender: `shared_secret = ECDH(sender_private_key, recipient_public_key)`.
3. Key derivation: `session_key = HKDF-SHA256(shared_secret, salt=packet_id || from_node)`.
4. Encryption: AES-256-CTR with `session_key` and the standard nonce.

```rust
use x25519_dalek::{PublicKey, StaticSecret};
use hkdf::Hkdf;
use sha2::Sha256;

pub(crate) fn derive_pki_key(
    my_private: &StaticSecret,
    their_public: &PublicKey,
    packet_id: u64,
    from_node: u32,
) -> [u8; 32] {
    let shared = my_private.diffie_hellman(their_public);

    let mut salt = [0u8; 12];
    salt[..8].copy_from_slice(&packet_id.to_le_bytes());
    salt[8..12].copy_from_slice(&from_node.to_le_bytes());

    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut key = [0u8; 32];
    hkdf.expand(b"meshtastic-pki", &mut key)
        .expect("HKDF output length is valid");
    key
}
```

Store the node's Curve25519 private key in kryphos vault. Load once at startup; keep in memory as a `StaticSecret`. Never log or persist derived shared bytes.

### 3.9 Nonce Security

`MeshPacket.id` is a random u32 zero-extended to u64. For a given (from_node, channel_psk) pair, the effective nonce space is 2^32. Birthday bound for nonce collision: approximately 2^16 = 65,536 packets. At one packet per minute, that is ~45 days before collision probability reaches 50%.

This is a known limitation of Meshtastic's security model. kerykeion cannot improve on it without firmware changes. Document it in the security notes for kerykeion.

---

## 4. NodeDB Data Model

### 4.1 In-Memory Schema

```rust
use compact_str::CompactString;
use jiff::Timestamp;
use std::collections::HashMap;

/// Newtype for the 32-bit node number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct NodeId(pub u32);

pub(crate) struct NodeDb {
    nodes: HashMap<NodeId, NodeRecord>,
    my_node_id: Option<NodeId>,
}

pub(crate) struct NodeRecord {
    pub num: NodeId,
    pub user: Option<NodeUser>,
    pub position: Option<CachedPosition>,
    pub device_metrics: Option<DeviceMetrics>,
    pub environment_metrics: Option<EnvironmentMetrics>,
    pub snr: Option<f32>,
    pub last_heard: Option<Timestamp>,
    pub hops_away: Option<u8>,
    pub is_licensed: bool,
    pub via_mqtt: bool,
    pub public_key: Option<[u8; 32]>,
    pub neighbors: Vec<NeighborLink>,
}

pub(crate) struct NodeUser {
    pub id: CompactString,           // "!aabbccdd"
    pub long_name: CompactString,
    pub short_name: CompactString,   // max 4 chars
    pub hw_model: i32,               // HardwareModel proto enum value
}

pub(crate) struct CachedPosition {
    pub latitude_i: i32,             // degrees × 1e7
    pub longitude_i: i32,
    pub altitude: Option<i32>,       // metres
    pub precision_bits: u8,
    pub recorded_at: Timestamp,
    pub sats_in_view: Option<u32>,
}

pub(crate) struct NeighborLink {
    pub node_id: NodeId,
    pub snr: f32,
    pub last_rx: Timestamp,
}
```

`DeviceMetrics` and `EnvironmentMetrics` mirror the proto definitions; store the most recent values only.

### 4.2 Update Strategy

After the initial config dump, `NodeInfo` arrives as app packets on `NODEINFO_APP`. Merge rules:

1. Always update `user`, `snr`, `last_heard`, `hops_away`, `via_mqtt`, `public_key`.
2. Update `position` only if incoming position is newer (compare `position.time`) or existing is absent.
3. Update `device_metrics` and `environment_metrics` unconditionally.
4. Preserve `neighbors` until a new `NeighborInfo` packet arrives for that node.

```rust
impl NodeDb {
    pub(crate) fn upsert(&mut self, incoming: NodeInfo) {
        let id = NodeId(incoming.num);
        let entry = self.nodes.entry(id).or_insert_with(|| NodeRecord::empty(id));

        if let Some(user) = incoming.user {
            entry.user = Some(NodeUser::from_proto(user));
        }
        if let Some(pos) = incoming.position {
            let newer = entry.position.as_ref()
                .map(|p| pos.time > p.recorded_at.as_second() as u32)
                .unwrap_or(true);
            if newer {
                entry.position = Some(CachedPosition::from_proto(pos));
            }
        }
        if let Some(dm) = incoming.device_metrics {
            entry.device_metrics = Some(DeviceMetrics::from_proto(dm));
        }
        entry.snr = Some(incoming.snr);
        entry.last_heard = jiff::Timestamp::now().ok();
        entry.hops_away = incoming.hops_away.try_into().ok();
        entry.via_mqtt = incoming.via_mqtt;
        if !incoming.public_key.is_empty() {
            entry.public_key = incoming.public_key.as_slice().try_into().ok();
        }
    }
}
```

### 4.3 Persistence

Serialize `NodeRecord` to CBOR via `ciborium` and write to a fjall partition keyed by `NodeId` bytes:

```rust
pub(crate) fn persist_node(
    keyspace: &fjall::Keyspace,
    record: &NodeRecord,
) -> Result<(), DbError> {
    let mut buf = Vec::new();
    ciborium::into_writer(record, &mut buf).context(SerializeSnafu)?;
    let partition = keyspace
        .open_partition("nodes", Default::default())
        .context(PartitionSnafu)?;
    partition
        .insert(record.num.0.to_be_bytes(), buf)
        .context(InsertSnafu)?;
    Ok(())
}

pub(crate) fn load_all(keyspace: &fjall::Keyspace) -> Result<NodeDb, DbError> {
    let partition = keyspace
        .open_partition("nodes", Default::default())
        .context(PartitionSnafu)?;
    let mut db = NodeDb::new();
    for item in partition.iter() {
        let (_, value) = item.context(IterSnafu)?;
        let record: NodeRecord =
            ciborium::from_reader(value.as_ref()).context(DeserializeSnafu)?;
        db.nodes.insert(record.num, record);
    }
    Ok(db)
}
```

---

## 5. Connection State Machine

```
                    ┌─────────────────────────────────────────────────────┐
                    │                                                     │
    port detected   ▼              port open,                            │
┌──────────────► Connecting ────── DTR set ──────────────────┐           │
│               └───────┘                                    ▼           │
│                  │ open error                         Handshaking       │
│                  │                                    └──────┘         │
│                  ▼                                         │            │
│             ─────────                    config_complete_id             │
│             cleanup                          received                   │
│                  │                                    ▼                 │
│                  │                                  Ready               │
│                  │                                  └────┘              │
│                  │                                ╱       ╲             │
│                  │                OS error /     ╱   PKT   ╲            │
│                  │                stale timeout ╱   FLOW    ╲           │
│                  │                      ▼      ╱              ╲         │
│                  │                    Error                    │         │
│                  │                    └────┘                   │         │
│                  │                       │                     │         │
│            backoff elapsed               │ cleanup             │         │
└──────────────────┴───────────────────────┘                    │         │
                                                                 │         │
                                   cable pulled / device reboot ──────────┘

States: Disconnected  Connecting  Handshaking  Ready  Error
```

**State transitions:**

| From | Event | To | Action |
|---|---|---|---|
| `Disconnected` | port appears (poll or udev) | `Connecting` | reset backoff |
| `Connecting` | `open()` succeeds | `Handshaking` | send `want_config_id`, start handshake timeout (30 s) |
| `Connecting` | `open()` fails | `Error` | log, increment backoff |
| `Handshaking` | `config_complete_id` matches | `Ready` | emit `SessionReady`, cancel handshake timeout |
| `Handshaking` | timeout (30 s) | `Error` | close port |
| `Ready` | `FromRadio` received | `Ready` | process, reset stale timer |
| `Ready` | stale timer fires (300 s) | `Error` | log stale |
| `Ready` | OS error on read/write | `Error` | log |
| `Error` | cleanup complete | `Disconnected` | — |
| `Disconnected` | backoff elapsed | `Connecting` | — |

**Handshake sub-states:**

```
AwaitingMyNodeInfo
  → (MyNodeInfo received) → AwaitingConfigItems
  → (all Config/ModuleConfig/Channel buffered) → AwaitingComplete
  → (config_complete_id matches) → Done
```

`AwaitingConfigItems` stays active until `ConfigCompleteId` is received regardless of how many config items arrive. The sequence is not length-prefixed; the complete marker signals the end.

---

## 6. Channel Configuration Management

### 6.1 Multi-Channel Representation

Up to 8 channels (index 0 = PRIMARY, 1–7 = SECONDARY):

```rust
pub(crate) struct ChannelSet {
    channels: [Option<ChannelConfig>; 8],
}

pub(crate) struct ChannelConfig {
    pub index: u8,
    pub role: ChannelRole,
    pub settings: ChannelSettings,
    pub psk: Psk,
}

pub(crate) enum ChannelRole {
    Primary,
    Secondary,
    Disabled,
}
```

Channel 0 (PRIMARY) carries `NODEINFO_APP`, `POSITION_APP`, `TELEMETRY_APP`, and mesh-wide broadcasts. Secondary channels carry application-specific traffic.

### 6.2 Admin Operations

Channel add/modify/remove goes through `AdminMessage`:

```rust
// Set (add or replace) a channel
let msg = AdminMessage {
    payload_variant: Some(admin_message::PayloadVariant::SetChannel(channel_proto)),
    ..Default::default()
};

// Remove: set role to DISABLED
let msg = AdminMessage {
    payload_variant: Some(admin_message::PayloadVariant::SetChannel(Channel {
        index: idx as i32,
        role: channel::Role::Disabled as i32,
        settings: None,
    })),
    ..Default::default()
};
```

Send via `ToRadio::Packet` on port `ADMIN_APP`, addressed to `my_node_id` as both `to` and `from`.

**PKI admin (v2.5+):** The firmware supports a `session_passkey` field in `AdminMessage` for authenticated admin operations. Defer PKI admin to a later block; implement channel PSK-encrypted admin first. Detect firmware version from `MyNodeInfo.firmware_version`.

### 6.3 Channel URL Encoding

`ChannelSettings` can be serialized as base64url-encoded protobuf:

```
https://meshtastic.org/e/#<base64url(ChannelSet protobuf)>
```

```rust
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

pub(crate) fn encode_channel_url(channel_set_proto: &meshtastic::ChannelSet) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(channel_set_proto.encode_to_vec());
    format!("https://meshtastic.org/e/#{encoded}")
}

pub(crate) fn decode_channel_url(url: &str) -> Result<meshtastic::ChannelSet, ChannelError> {
    let fragment = url.split('#').nth(1).ok_or(ChannelError::MissingFragment)?;
    let bytes = URL_SAFE_NO_PAD.decode(fragment).context(Base64Snafu)?;
    meshtastic::ChannelSet::decode(bytes.as_slice()).context(ProtobufSnafu)
}
```

---

## 7. Transport Discovery

Serial is the reliable path. USB serial (CP2102N, CH9102, CH340) connects deterministically and requires no scanning. BLE and TCP are fallbacks.

### 7.1 USB VID:PID Discovery

| Chipset | VID | PID | Common on |
|---|---|---|---|
| Silicon Labs CP2102N | 10C4 | EA60 | RAK4631, most DIY |
| WCH CH9102 | 1A86 | 55D4 | T-Beam Supreme |
| WCH CH340 | 1A86 | 7523 | T-Beam v0.7, older boards |

Use `rusb 0.9.4` to enumerate USB devices and match VID:PID, then resolve the `/dev/ttyUSBn` path from sysfs.

### 7.2 Discovery Priority

```
1. Enumerate USB via rusb → find known chipset → open serial
2. mDNS query "_meshtastic._tcp.local." (2 s timeout) → TCP connect
3. BLE scan (5 s) → GATT connect via btleplug
4. Fail with DiscoveryError::NoDeviceFound
```

Priority rationale: serial is instantaneous; mDNS is faster than BLE scan; BLE scan takes the most time and GATT has higher per-operation overhead.

See P2-R3 §2 for full BLE (btleplug) GATT implementation detail and §7 for complete discovery strategy with code examples.

---

## 8. Test Harness Architecture

### 8.1 Mock Serial Device via Unix PTY

A Unix PTY pair gives two file descriptors: master (held by the test) and slave (presented to code under test as a serial port path). No real hardware needed.

```rust
use nix::pty::{openpty, OpenptyResult};
use std::os::unix::io::{FromRawFd, IntoRawFd};

pub(crate) struct MockDevice {
    master: std::fs::File,
    pub slave_path: std::path::PathBuf,
}

impl MockDevice {
    pub(crate) fn new() -> Result<Self, nix::Error> {
        let OpenptyResult { master, slave } = openpty(None, None)?;
        let slave_path = std::fs::read_link(
            format!("/proc/self/fd/{}", slave.as_raw_fd())
        ).unwrap_or_else(|_| format!("/dev/fd/{}", slave.as_raw_fd()).into());

        Ok(Self {
            master: unsafe { std::fs::File::from_raw_fd(master.into_raw_fd()) },
            slave_path,
        })
    }

    /// Send a FromRadio message to the code under test.
    pub(crate) fn send_from_radio(&mut self, msg: &FromRadio) -> std::io::Result<()> {
        use std::io::Write as _;
        let payload = msg.encode_to_vec();
        let header = [0x94u8, 0xC3, (payload.len() >> 8) as u8, payload.len() as u8];
        self.master.write_all(&header)?;
        self.master.write_all(&payload)
    }

    /// Read a ToRadio message sent by the code under test.
    pub(crate) fn recv_to_radio(&mut self) -> std::io::Result<ToRadio> {
        use std::io::Read as _;
        let mut header = [0u8; 4];
        self.master.read_exact(&mut header)?;
        assert_eq!(&header[..2], &[0x94, 0xC3]);
        let len = u16::from_be_bytes([header[2], header[3]]) as usize;
        let mut buf = vec![0u8; len];
        self.master.read_exact(&mut buf)?;
        ToRadio::decode(buf.as_slice())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
```

### 8.2 Recorded Data Format

Capture real serial traces from T-Echo/T-Deck for regression tests. Binary format:

```
File header:
  magic[4]       = "MESH"
  version[2]     = 0x0001
  reserved[2]    = 0x0000

Record:
  timestamp_us[8]   u64 LE — microseconds since Unix epoch
  direction[1]      0x00 = device→host, 0x01 = host→device
  length[2]         u16 LE — raw frame bytes (header + payload)
  data[length]      raw bytes as they appeared on the wire
```

Replay with configurable timing (real-time, 10× compressed, or instant for CI). Check fixture files into `crates/kerykeion/testdata/`:

```
testdata/
  t_echo_handshake.meshcap
  t_deck_handshake.meshcap
  t_echo_packet_stream.meshcap   ← 5 minutes of normal mesh traffic
```

### 8.3 Proptest for Codec Fuzzing

`proptest 1.10.0` generates arbitrary inputs and shrinks failures. Use it to ensure the codec never panics:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn codec_never_panics_on_arbitrary_bytes(data in any::<Vec<u8>>()) {
        let mut buf = bytes::BytesMut::from(data.as_slice());
        let mut codec = MeshFrameCodec;
        // Must not panic; any of Ok(None), Ok(Some(_)), Err(_) is acceptable.
        let _ = codec.decode(&mut buf);
    }

    #[test]
    fn codec_roundtrip(payload_len in 1usize..=MAX_PAYLOAD) {
        let payload: Vec<u8> = (0..payload_len).map(|i| (i % 256) as u8).collect();
        let mut codec = MeshFrameCodec;

        let mut buf = bytes::BytesMut::new();
        codec.encode(bytes::Bytes::copy_from_slice(&payload), &mut buf).unwrap();

        let result = codec.decode(&mut buf).unwrap().unwrap();
        prop_assert_eq!(result.as_ref(), payload.as_slice());
    }
}
```

### 8.4 Deterministic Async Timing

`tokio::time::pause()` stops the internal clock. Advance with `tokio::time::advance(Duration)` for timeout tests without actual sleeps:

```rust
#[tokio::test]
async fn reconnect_backs_off_correctly() {
    tokio::time::pause();
    // ... configure mock that rejects N times ...
    tokio::time::advance(Duration::from_secs(60)).await;
    // assert correct number of attempts
}
```

### 8.5 Test Partitioning

| Category | Location | CI |
|---|---|---|
| Frame codec encode/decode | `src/codec.rs` unit | Yes |
| Codec proptest fuzz | `src/codec.rs` unit | Yes |
| AES-CTR known-answer tests | `src/crypto.rs` unit | Yes |
| NodeDB upsert/merge logic | `src/nodedb.rs` unit | Yes |
| Handshake state machine (PTY mock) | `tests/handshake.rs` | Yes |
| Config round-trip over PTY | `tests/integration.rs` | Yes |
| Real hardware: T-Echo handshake | `tests/hardware.rs` | `#[ignore]` |
| Real hardware: packet tx/rx | `tests/hardware.rs` | `#[ignore]` |

Hardware tests run manually with `cargo test -p kerykeion -- --ignored` before release.

---

## 9. Crate Dependency List

Licenses checked against AGPL-3.0-only. MIT, Apache-2.0, and BSD-3-Clause are compatible. MPL-2.0 is file-level copyleft; AGPL-compatible for code that calls it. GPL-2.0 and GPL-3.0 are excluded.

### New dependencies for kerykeion

| Crate | Version | License | Purpose |
|---|---|---|---|
| `tokio-serial` | `5.4.5` | MIT | Async serial I/O |
| `btleplug` | `0.12.0` | MIT/Apache-2.0/BSD-3-Clause | BLE GATT client (Linux/BlueZ) |
| `uuid` | `1` | MIT OR Apache-2.0 | BLE service/characteristic UUIDs |
| `prost` | `0.14.3` | Apache-2.0 | Protobuf encode/decode |
| `prost-types` | `0.14.3` | Apache-2.0 | Well-known proto types (Timestamp) |
| `aes` | `=0.9.0-rc.4` | MIT OR Apache-2.0 | AES-128/256 block cipher |
| `ctr` | `=0.10.0-rc.4` | MIT OR Apache-2.0 | CTR stream cipher mode |
| `x25519-dalek` | `2.0.1` | BSD-3-Clause | Curve25519 ECDH for PKI |
| `hkdf` | `0.12` | MIT OR Apache-2.0 | Key derivation for PKI |
| `sha2` | `0.10` | MIT OR Apache-2.0 | SHA-256 for HKDF |
| `petgraph` | `0.8.3` | MIT OR Apache-2.0 | Mesh topology graph |
| `mdns-sd` | `0.18.2` | Apache-2.0 OR MIT | mDNS/TCP device discovery |
| `rusb` | `0.9.4` | MIT | USB enumeration for serial discovery |
| `base64` | `0.22` | MIT OR Apache-2.0 | Channel URL encoding |
| `tokio-util` | `0.7.18` | MIT | `Codec` trait, `Framed` |
| `bytes` | `1` | MIT | `Bytes`, `BytesMut` in codec |

### Build dependencies

| Crate | Version | License | Purpose |
|---|---|---|---|
| `prost-build` | `0.14.3` | Apache-2.0 | Proto code generation in build.rs |

### Dev dependencies

| Crate | Version | License | Purpose |
|---|---|---|---|
| `proptest` | `1.10.0` | MIT OR Apache-2.0 | Property-based codec fuzzing |
| `nix` | `0.31.2` | MIT | PTY pairs for mock serial device |
| `tokio` (test-util feature) | `1.44` | MIT | `tokio::time::pause()` |

### Already in workspace

| Crate | Purpose in kerykeion |
|---|---|
| `tokio` | Async runtime |
| `serialport` | DTR/RTS pin control via `tokio-serial` |
| `fjall` | NodeDB persistence |
| `ciborium` | CBOR serialization for NodeDB |
| `compact_str` | Short string fields in NodeRecord |
| `jiff` | Timestamps |
| `snafu` | Error handling |
| `tracing` | Structured logging |
| `serde` | Derives on generated proto types |

### Crates to avoid

| Crate | Reason |
|---|---|
| `meshtastic` (official Rust crate, 0.1.8) | GPL-3.0, incomplete (~15% coverage), not clean-room |
| `ring` | No raw AES-CTR; AEAD-only |
| `bluer` | Linux-only, worse API ergonomics than btleplug |
| `zeroconf` | Wraps Avahi/Bonjour C library; `mdns-sd` covers the need |
| `openssl` | Heavy C FFI, license complexity |

### deny.toml additions

```toml
[[licenses.deny]]
name = "GPL-2.0-only"

[[licenses.deny]]
name = "GPL-2.0-or-later"

# GPL-3.0 excluded to stay clean-room relative to the official Meshtastic Rust crate.
[[licenses.deny]]
name = "GPL-3.0-only"
```

### Pre-release version pinning

`aes` and `ctr` are pinned with exact version (`=`) because they are pre-release. The RustCrypto ecosystem coordinates simultaneous stable releases; update both at once when stable versions land. MSRV for both is 1.85 (our floor). `x25519-dalek 2.0.1` is the current stable release; pin without `=`. The 3.x series is in pre-release as of 2026-03-18; upgrade when it stabilizes (see P2-R3 §crate-selection-matrix).

---

## Open Questions

1. **PKI `session_passkey` exchange:** The v2.5 PKI admin protocol is partially documented. Trace through `AdminModule.cpp` in the pinned firmware tag before implementing. Defer PKI admin to a later block; implement channel PSK admin first.

2. **`_meshtastic._tcp` service type:** Meshtastic firmware 2.x advertises this service when WiFi is enabled. Some builds may use `_http._tcp` with a Meshtastic-specific TXT record. Confirm the exact service name against the pinned firmware version before relying on mDNS discovery.

3. **PTY on macOS:** `nix::pty::openpty` is POSIX and works on macOS (`/dev/ttys*`). CI runs on Linux (`/dev/pts/*`). Confirm `tokio-serial` correctly opens PTY slave paths on both platforms if local development uses macOS.

4. **`want_config_id` multi-client:** The config_id is a random u32. For single-client use (our case), collision probability is negligible. For future multi-client scenarios, the device matches `config_complete_id` to the most recent `want_config_id` only. Document this limitation.
