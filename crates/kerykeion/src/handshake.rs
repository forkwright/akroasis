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

use std::time::Duration;

use rand_core::{OsRng, RngCore as _};
use tokio::time::timeout;

use crate::Error;
use crate::connection::MeshConnection;
use crate::error::HandshakeFailedSnafu;
use crate::node_db::{MeshNode, NodeDb, NodePosition, UserInfo};
use crate::proto::{Channel, ToRadio, from_radio, to_radio};
use crate::types::NodeNum;

/// Maximum time to wait for a complete config dump FROM the radio.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Result of a successful config handshake with the radio.
#[derive(Debug)]
pub struct HandshakeResult {
    /// Node number of the local radio.
    pub my_node_num: NodeNum,
    /// Channel configurations received during the dump.
    pub channels: Vec<Channel>,
    /// Snapshot of all nodes seen during the handshake.
    pub known_nodes: Vec<MeshNode>,
}

/// Run the config handshake and populate `node_db` with discovered nodes.
///
/// Sends `want_config_id` to the radio, then reads `FromRadio` messages until
/// `config_complete_id` is received. Returns a [`HandshakeResult`] on success.
///
/// # Errors
///
/// Returns [`Error::HandshakeFailed`] if the handshake times out or if the
/// radio does not complete the config dump with a matching ID.
pub async fn handshake(
    conn: &mut impl MeshConnection,
    node_db: &mut NodeDb,
) -> Result<HandshakeResult, Error> {
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

    timeout(HANDSHAKE_TIMEOUT, handshake_fut)
        .await
        .map_err(|_| Error::HandshakeFailed {
            detail: format!(
                "config dump timed out after {}s (want_config_id={want_config_id})",
                HANDSHAKE_TIMEOUT.as_secs()
            ),
            location: snafu::Location::new(file!(), line!(), column!()),
        })??;

    let my_node_num = my_node_num.ok_or_else(|| Error::HandshakeFailed {
        detail: "radio did not send MyNodeInfo".to_owned(),
        location: snafu::Location::new(file!(), line!(), column!()),
    })?;

    Ok(HandshakeResult {
        my_node_num,
        channels,
        known_nodes,
    })
}

/// Convert a proto `NodeInfo` INTO a [`MeshNode`] for the in-memory database.
pub fn node_info_to_mesh_node(ni: &crate::proto::NodeInfo) -> MeshNode {
    let user = ni.user.as_ref().map(|u| UserInfo {
        id: u.id.clone(),
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
        timestamp: jiff::Timestamp::from_second(i64::from(p.time)).ok(),
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
        #[expect(
            clippy::cast_possible_truncation,
            reason = "hops_away is bounded by MAX_HOP_LIMIT (7) in Meshtastic firmware"
        )]
        hop_count: if ni.hops_away != 0 {
            Some(ni.hops_away as u8)
        } else {
            None
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::proto::{Channel, FromRadio, MyNodeInfo, NodeInfo, ToRadio, from_radio, to_radio};
    use tracing::Instrument as _;

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
}
