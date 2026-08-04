//! Config handshake state machine for Meshtastic radio initialisation.
//!
//! After establishing a transport connection the host must exchange a config
//! dump with the radio before it can send or receive mesh packets:
//!
//! ```text
//! Host → Radio : ToRadio { want_config_id: <random> }
//! Radio → Host : FromRadio { my_info: MyNodeInfo { … } }
//! Radio → Host : FromRadio { node_info: NodeInfo { … } }  (×N)
//! Radio → Host : FromRadio { channel: Channel { … } }    (×N)
//! Radio → Host : FromRadio { config_complete_id: <id> }  ← end-of-dump
//! ```
//!
//! The handshake times out after 10 seconds if the radio does not send
//! `config_complete_id` matching the value sent in `want_config_id`.

use rand_core::{OsRng, RngCore as _};
use tokio::time::timeout;
use tracing::instrument;

use crate::Error;
use crate::config::HandshakeConfig;
use crate::connection::MeshConnection;
use crate::error::HandshakeFailedSnafu;
use crate::node_db::{MeshNode, NodeDb, NodePosition, UserInfo};
use crate::proto::{Channel, ToRadio, from_radio, to_radio};
use crate::types::NodeNum;

// Historical default (10 s) now lives in [`HandshakeConfig::default`].

/// Result of a successful config handshake with the radio.
// WHY: pure data — a handshake result bag with no derived invariant.
#[derive(Debug)]
pub struct HandshakeResult {
    /// Node number of the local radio.
    pub my_node_num: NodeNum,
    /// Channel configurations received during the dump.
    pub channels: Vec<Channel>,
    /// Snapshot of all nodes seen during the handshake.
    pub known_nodes: Vec<MeshNode>,
}

/// Run the config handshake with the default timeout.
///
/// Equivalent to `handshake_with_config(conn, node_db, &HandshakeConfig::default())`.
///
/// # Errors
///
/// Returns [`Error::HandshakeFailed`] if the handshake times out or if the
/// radio does not complete the config dump with a matching ID.
#[instrument(level = "debug", skip(conn, node_db))]
pub async fn handshake(
    conn: &mut impl MeshConnection,
    node_db: &mut NodeDb,
) -> Result<HandshakeResult, Error> {
    handshake_with_config(conn, node_db, &HandshakeConfig::default()).await
}

/// Run the config handshake and populate `node_db` with discovered nodes.
///
/// Sends `want_config_id` to the radio, then reads `FromRadio` messages until
/// `config_complete_id` is received. Returns a [`HandshakeResult`] on success.
///
/// The handshake will be aborted after [`HandshakeConfig::timeout_secs`].
///
/// # Errors
///
/// Returns [`Error::HandshakeFailed`] if the handshake times out or if the
/// radio does not complete the config dump with a matching ID.
#[instrument(
    level = "debug",
    skip(conn, node_db, config),
    fields(timeout_secs = config.timeout_secs)
)]
pub async fn handshake_with_config(
    conn: &mut impl MeshConnection,
    node_db: &mut NodeDb,
    config: &HandshakeConfig,
) -> Result<HandshakeResult, Error> {
    let handshake_timeout = config.timeout();
    let want_config_id: u32 = OsRng.next_u32();

    conn.send(ToRadio {
        payload_variant: Some(to_radio::PayloadVariant::WantConfigId(want_config_id)),
    })
    .await?;

    tracing::debug!(want_config_id, "sent want_config_id; awaiting config dump");

    let mut my_node_num: Option<NodeNum> = None;
    let mut channels: Vec<Channel> = Vec::new();
    let mut known_nodes: Vec<MeshNode> = Vec::new();

    let handshake_fut = async {
        loop {
            let from_radio = conn.recv().await?;
            match from_radio.payload_variant {
                Some(from_radio::PayloadVariant::MyInfo(info)) => {
                    let num = NodeNum(info.my_node_num);
                    node_db.set_my_node(num);
                    my_node_num = Some(num);
                    tracing::debug!(my_node_num = info.my_node_num, "received MyNodeInfo");
                }

                Some(from_radio::PayloadVariant::NodeInfo(ni)) => {
                    let node = node_info_to_mesh_node(&ni);
                    tracing::trace!(node_num = node.num.0, "received NodeInfo");
                    known_nodes.push(node.clone());
                    node_db.insert(node);
                }

                Some(from_radio::PayloadVariant::Channel(ch)) => {
                    tracing::trace!(index = ch.index, "received Channel");
                    channels.push(ch);
                }

                Some(from_radio::PayloadVariant::ConfigCompleteId(id)) => {
                    if id == want_config_id {
                        tracing::debug!(config_complete_id = id, "handshake complete");
                        break;
                    }
                    tracing::warn!(
                        received = id,
                        expected = want_config_id,
                        "config_complete_id mismatch; ignoring"
                    );
                }

                Some(from_radio::PayloadVariant::Packet(pkt)) => {
                    // WHY: mesh packets can arrive during the config dump; log and discard.
                    tracing::trace!(
                        packet_id = pkt.id,
                        "discarding mesh packet during handshake"
                    );
                }

                Some(from_radio::PayloadVariant::Rebooted(_)) => {
                    return HandshakeFailedSnafu {
                        detail: "radio rebooted during handshake",
                    }
                    .fail();
                }

                None => {}
            }
        }

        Ok::<(), Error>(())
    };

    timeout(handshake_timeout, handshake_fut)
        .await
        .map_err(|_| Error::HandshakeFailed {
            detail: format!(
                "config dump timed out after {}s (want_config_id={want_config_id})",
                handshake_timeout.as_secs()
            ),
            location: snafu::location!(),
        })??;

    let my_node_num = my_node_num.ok_or_else(|| Error::HandshakeFailed {
        detail: "radio did not send MyNodeInfo".to_owned(),
        location: snafu::location!(),
    })?;

    Ok(HandshakeResult {
        my_node_num,
        channels,
        known_nodes,
    })
}

/// Convert a proto `NodeInfo` INTO a [`MeshNode`] for the in-memory database.
pub(crate) fn node_info_to_mesh_node(ni: &crate::proto::NodeInfo) -> MeshNode {
    let user = ni.user.as_ref().map(|u| UserInfo {
        id: u.id.clone().into(),
        long_name: u.long_name.clone(),
        short_name: u.short_name.clone(),
        // WHY: proto3 stores HardwareModel as i32; VALUES are always ≥ 0.
        hw_model: u32::try_from(u.hw_model).unwrap_or(0),
        is_licensed: u.is_licensed,
    });

    let position = ni.position.as_ref().map(|p| NodePosition {
        // WHY: Meshtastic encodes lat/lon as integer degrees × 1e7.
        latitude: f64::from(p.latitude_i) * 1e-7,
        longitude: f64::from(p.longitude_i) * 1e-7,
        altitude: if p.altitude != 0 {
            Some(p.altitude)
        } else {
            None
        },
        // WHY: invalid/zero timestamps in Position protobufs map to None rather than
        // propagating an error; the position is still usable for lat/lon display.
        timestamp: jiff::Timestamp::from_second(i64::from(p.time)).ok(), // kanon:ignore RUST/silent-error-ok -- timestamp is optional metadata, invalid→None is correct
    });

    MeshNode {
        num: NodeNum(ni.num),
        user,
        position,
        metrics: None,
        last_heard: if ni.last_heard != 0 {
            jiff::Timestamp::from_second(i64::from(ni.last_heard)).ok()
        } else {
            None
        },
        snr: if ni.snr == 0.0 { None } else { Some(ni.snr) },
        // WHY: hops_away's valid domain includes 0 (a direct neighbor), unlike
        // snr/last_heard where 0 IS the proto3 unset sentinel — so it is
        // always present, matching processor.rs's hop_count == Some(0)
        // "direct" convention (#201) instead of treating 0 as unknown.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "hops_away is bounded by MAX_HOP_LIMIT (7) in Meshtastic firmware"
        )]
        hop_count: Some(ni.hops_away as u8), // SAFETY: Meshtastic hops_away field is 0..255 per protocol; fits u8
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tracing::Instrument as _;

    use super::*;
    use crate::proto::{Channel, FromRadio, MyNodeInfo, NodeInfo, ToRadio, from_radio, to_radio};

    // ── Shared mock types ─────────────────────────────────────────────────────

    /// Mock that returns `config_complete_id` matching whatever `want_config_id` was sent.
    struct DynamicMock {
        my_info_sent: bool,
        node_info_sent: bool,
        config_id: Option<u32>,
    }

    impl MeshConnection for DynamicMock {
        async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
            if let Some(to_radio::PayloadVariant::WantConfigId(id)) = &packet.payload_variant {
                self.config_id = Some(*id);
            }
            Ok(())
        }

        async fn recv(&mut self) -> Result<FromRadio, Error> {
            if !self.my_info_sent {
                self.my_info_sent = true;
                return Ok(FromRadio {
                    id: 1,
                    payload_variant: Some(from_radio::PayloadVariant::MyInfo(MyNodeInfo {
                        my_node_num: 0xCAFE_BABE,
                    })),
                });
            }
            if !self.node_info_sent {
                self.node_info_sent = true;
                return Ok(FromRadio {
                    id: 2,
                    payload_variant: Some(from_radio::PayloadVariant::NodeInfo(NodeInfo {
                        num: 0x1111_1111,
                        snr: 3.5,
                        ..Default::default()
                    })),
                });
            }
            let id = self.config_id.unwrap_or(0);
            Ok(FromRadio {
                id: 3,
                payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(id)),
            })
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// Mock that stalls forever in `recv()`  -  used to test timeout behaviour.
    struct StallMock;

    impl MeshConnection for StallMock {
        async fn send(&mut self, _: ToRadio) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&mut self) -> Result<FromRadio, Error> {
            std::future::pending().await
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// Mock that sends `MyNodeInfo` → one `Channel` → `config_complete_id`.
    struct ChanMock {
        step: u32,
        config_id: Option<u32>,
    }

    impl MeshConnection for ChanMock {
        async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
            if let Some(to_radio::PayloadVariant::WantConfigId(id)) = packet.payload_variant {
                self.config_id = Some(id);
            }
            Ok(())
        }

        async fn recv(&mut self) -> Result<FromRadio, Error> {
            self.step += 1;
            match self.step {
                1 => Ok(FromRadio {
                    id: 1,
                    payload_variant: Some(from_radio::PayloadVariant::MyInfo(MyNodeInfo {
                        my_node_num: 0x1111,
                    })),
                }),
                2 => Ok(FromRadio {
                    id: 2,
                    payload_variant: Some(from_radio::PayloadVariant::Channel(Channel {
                        index: 0,
                        ..Default::default()
                    })),
                }),
                _ => Ok(FromRadio {
                    id: 3,
                    payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(
                        self.config_id.unwrap_or(0),
                    )),
                }),
            }
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// Mock that answers with a WRONG `config_complete_id` before sending the
    /// `NodeInfo` and then the matching id.
    struct MismatchMock {
        step: u32,
        config_id: Option<u32>,
    }

    impl MeshConnection for MismatchMock {
        async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
            if let Some(to_radio::PayloadVariant::WantConfigId(id)) = packet.payload_variant {
                self.config_id = Some(id);
            }
            Ok(())
        }

        async fn recv(&mut self) -> Result<FromRadio, Error> {
            self.step += 1;
            match self.step {
                1 => Ok(FromRadio {
                    id: 1,
                    payload_variant: Some(from_radio::PayloadVariant::MyInfo(MyNodeInfo {
                        my_node_num: 0x2222,
                    })),
                }),
                // WHY: a stale id from an earlier config dump — must be ignored.
                2 => Ok(FromRadio {
                    id: 2,
                    payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(
                        self.config_id.unwrap_or(0).wrapping_add(1),
                    )),
                }),
                3 => Ok(FromRadio {
                    id: 3,
                    payload_variant: Some(from_radio::PayloadVariant::NodeInfo(NodeInfo {
                        num: 0x3333,
                        ..Default::default()
                    })),
                }),
                _ => Ok(FromRadio {
                    id: 4,
                    payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(
                        self.config_id.unwrap_or(0),
                    )),
                }),
            }
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn handshake_happy_path() {
        let mut db = NodeDb::new();
        let mut conn = DynamicMock {
            my_info_sent: false,
            node_info_sent: false,
            config_id: None,
        };

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handshake(&mut conn, &mut db).await.unwrap();

        assert_eq!(result.my_node_num.0, 0xCAFE_BABE);
        assert_eq!(result.known_nodes.len(), 1);
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let node = result.known_nodes.first().unwrap();
        assert_eq!(node.num.0, 0x1111_1111);
    }

    #[tokio::test(start_paused = true)]
    async fn handshake_timeout_on_incomplete_dump() {
        // Spawn INTO a task so we can advance time while the handshake awaits recv().
        let handle = tokio::spawn(
            async {
                let mut db = NodeDb::new();
                let mut conn = StallMock;
                handshake(&mut conn, &mut db).await
            }
            .instrument(tracing::info_span!("spawned_task")),
        );

        // The handshake has a 10 s internal timeout; advance past it.
        tokio::time::advance(Duration::from_secs(11)).await;
        tokio::task::yield_now().await;

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(result.is_err(), "handshake should fail after 10 s timeout");
    }

    #[tokio::test(start_paused = true)]
    async fn configured_timeout_observably_aborts_faster() {
        // WHY: parameterization-observability test — a 2 s timeout must
        // cause handshake_with_config to fail within 2–3 s of virtual time,
        // where the 10 s default would still be running.
        let handle = tokio::spawn(
            async {
                let mut db = NodeDb::new();
                let mut conn = StallMock;
                let cfg = HandshakeConfig { timeout_secs: 2 };
                handshake_with_config(&mut conn, &mut db, &cfg).await
            }
            .instrument(tracing::info_span!("spawned_task")),
        );

        tokio::time::advance(Duration::from_secs(3)).await;
        tokio::task::yield_now().await;

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(
            result.is_err(),
            "handshake with 2 s timeout must fail within 3 s of virtual time"
        );
    }

    #[tokio::test]
    async fn handshake_stores_channels() {
        let mut db = NodeDb::new();
        let mut conn = ChanMock {
            step: 0,
            config_id: None,
        };

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handshake(&mut conn, &mut db).await.unwrap();
        assert_eq!(result.channels.len(), 1);
    }

    #[test]
    fn node_info_zero_hops_away_maps_to_direct_hop_count() {
        // WHY: 0 hops away means "direct neighbor", not "unknown" — must
        // agree with processor.rs's Some(0)-is-direct convention (#201),
        // not collapse to None the way it did before #321.
        let ni = NodeInfo {
            hops_away: 0,
            ..Default::default()
        };

        let node = node_info_to_mesh_node(&ni);

        assert_eq!(node.hop_count, Some(0));
    }

    #[test]
    fn node_info_nonzero_hops_away_maps_to_that_hop_count() {
        let ni = NodeInfo {
            hops_away: 3,
            ..Default::default()
        };

        let node = node_info_to_mesh_node(&ni);

        assert_eq!(node.hop_count, Some(3));
    }

    #[tokio::test]
    async fn mismatched_config_complete_id_does_not_end_the_handshake() {
        // WHY(#229): a ConfigCompleteId that does not match want_config_id is a
        // stale reply from an earlier config dump. The loop must ignore it and
        // keep reading, not break out with a half-populated result. The mock
        // sends the wrong id first and the NodeInfo only afterwards, so a
        // premature break loses that node and the assertion fails.
        let mut db = NodeDb::new();
        let mut conn = MismatchMock {
            step: 0,
            config_id: None,
        };

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handshake(&mut conn, &mut db).await.unwrap();

        assert_eq!(result.my_node_num.0, 0x2222);
        assert_eq!(
            result.known_nodes.len(),
            1,
            "the NodeInfo sent after the mismatched id must still be collected"
        );
    }

    #[test]
    fn node_info_converts_position_timestamps_and_snr() {
        // WHY(#229): covers the scaling and proto3-sentinel rules in
        // node_info_to_mesh_node together — lat/lon are integer degrees x 1e7,
        // a zero altitude/last_heard/snr is "unset" rather than a real zero.
        let ni = NodeInfo {
            num: 0xABCD,
            snr: 4.25,
            last_heard: 1_700_000_000,
            hops_away: 2,
            user: Some(crate::proto::User {
                id: "!abcd".into(),
                long_name: "Long".into(),
                short_name: "SHRT".into(),
                macaddr: vec![],
                hw_model: 9,
                is_licensed: true,
                role: 0,
            }),
            position: Some(crate::proto::Position {
                latitude_i: 515_074_000,
                longitude_i: -1_278_000,
                altitude: 42,
                time: 1_700_000_001,
                ..Default::default()
            }),
            ..Default::default()
        };

        let node = node_info_to_mesh_node(&ni);

        assert_eq!(node.num.0, 0xABCD);
        assert_eq!(node.hop_count, Some(2));
        assert_eq!(node.snr, Some(4.25));

        #[expect(clippy::unwrap_used, reason = "test-only: constructed above")]
        let position = node.position.unwrap();
        assert!((position.latitude - 51.507_4).abs() < 1e-9);
        assert!((position.longitude - -0.127_8).abs() < 1e-9);
        assert_eq!(position.altitude, Some(42));
        assert!(position.timestamp.is_some());

        #[expect(clippy::unwrap_used, reason = "test-only: constructed above")]
        let user = node.user.unwrap();
        assert_eq!(user.short_name, "SHRT");
        assert_eq!(user.hw_model, 9);
        assert!(user.is_licensed);

        #[expect(clippy::unwrap_used, reason = "test-only: nonzero last_heard")]
        let last_heard = node.last_heard.unwrap();
        assert_eq!(last_heard.as_second(), 1_700_000_000);
    }

    #[test]
    fn node_info_proto3_zero_sentinels_map_to_none() {
        // WHY(#229): the falsifiable half of the pair above — 0 means "unset"
        // for snr, last_heard and altitude, so none of them may survive as a
        // real reading.
        let ni = NodeInfo {
            num: 1,
            snr: 0.0,
            last_heard: 0,
            position: Some(crate::proto::Position {
                latitude_i: 0,
                longitude_i: 0,
                altitude: 0,
                time: 0,
                ..Default::default()
            }),
            ..Default::default()
        };

        let node = node_info_to_mesh_node(&ni);

        assert_eq!(node.snr, None);
        assert_eq!(node.last_heard, None);
        #[expect(clippy::unwrap_used, reason = "test-only: constructed above")]
        let position = node.position.unwrap();
        assert_eq!(position.altitude, None);
    }

    // ── akroasis#229: HandshakeResult must mirror every node_db write ─────

    #[tokio::test]
    async fn handshake_result_mirrors_every_node_db_write() {
        // WHY: the collector runs the handshake against a scratch NodeDb and
        // merges HandshakeResult into the shared one afterwards, so the shared
        // lock is not held across the radio's config dump. That is equivalent
        // only while the result carries everything the handshake writes  -  a
        // write with no matching result field would be dropped by the merge.
        let mut direct = NodeDb::new();
        let mut conn = DynamicMock {
            my_info_sent: false,
            node_info_sent: false,
            config_id: None,
        };

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handshake(&mut conn, &mut direct).await.unwrap();

        let mut merged = NodeDb::new();
        merged.set_my_node(result.my_node_num);
        for node in result.known_nodes {
            merged.insert(node);
        }

        assert_eq!(
            merged.my_node(),
            direct.my_node(),
            "merge must reproduce the local node number"
        );
        assert_eq!(
            merged.len(),
            direct.len(),
            "merge must reproduce every node the handshake inserted"
        );
        for (num, node) in direct.iter() {
            #[expect(clippy::expect_used, reason = "test-only")]
            let mirrored = merged.get(*num).expect("node missing from merged DB");
            assert_eq!(mirrored.num, node.num);
            assert_eq!(mirrored.hop_count, node.hop_count);
        }
    }
}
