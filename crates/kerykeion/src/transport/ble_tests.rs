//! Deterministic coverage for the BLE transport over an in-memory peripheral.
//!
//! Every case here runs with no adapter, no radio and no timer: the injected
//! [`FakePeripheral`] is scripted with the exact sequence of reads, failures and
//! connect outcomes each behaviour needs.

#![expect(
    clippy::expect_used,
    reason = "unit test — an unmet expectation is the correct failure mode"
)]

use std::collections::VecDeque;

use super::{BlePeripheral, BleTransport};
use crate::Error;
use crate::config::TransportConfig;
use crate::connection::MeshConnection;
use crate::proto::{FromRadio, ToRadio, from_radio, to_radio};

/// The operation, if any, scripted to report a dropped link.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drops {
    /// `write_to_radio` reports the link is gone.
    Write,
    /// `read_from_radio` reports the link is gone.
    Read,
    /// `wait_for_data` reports the link is gone.
    Wait,
}

/// One scripted outcome of [`BlePeripheral::read_from_radio`].
enum Read {
    /// The radio had nothing queued at this point in the sequence.
    Empty,
    /// The radio returned this encoded body.
    Body(Vec<u8>),
}

/// An in-memory peripheral scripted by the test that builds it.
#[derive(Default)]
struct FakePeripheral {
    /// Names this fake will answer a scan with.
    advertised: Vec<String>,
    /// Remaining scripted read outcomes, consumed in order.
    reads: VecDeque<Read>,
    /// Every body written through `write_to_radio`.
    written: Vec<Vec<u8>>,
    /// Whether the link is currently up.
    connected: bool,
    /// How many times `connect` has been called.
    connect_calls: usize,
    /// Connect attempts still scripted to fail before one succeeds.
    connect_failures: usize,
    /// How many times `wait_for_data` has been called.
    waits: usize,
    /// Which operation, if any, reports a dropped link.
    drops: Option<Drops>,
}

impl FakePeripheral {
    /// A peripheral advertising one device that answers scans for it.
    fn advertising(name: &str) -> Self {
        Self {
            advertised: vec![name.to_owned()],
            ..Self::default()
        }
    }

    /// Script the sequence of read outcomes this fake will return.
    fn with_reads(mut self, reads: Vec<Read>) -> Self {
        self.reads = reads.into();
        self
    }
}

impl BlePeripheral for FakePeripheral {
    async fn connect(&mut self, name_prefix: &str) -> Result<(), Error> {
        self.connect_calls += 1;
        if self.connect_failures > 0 {
            self.connect_failures -= 1;
            return super::no_matching_device(name_prefix);
        }
        if self
            .advertised
            .iter()
            .any(|name| name.starts_with(name_prefix))
        {
            self.connected = true;
            return Ok(());
        }
        super::no_matching_device(name_prefix)
    }

    async fn write_to_radio(&mut self, payload: &[u8]) -> Result<(), Error> {
        if self.drops == Some(Drops::Write) {
            self.connected = false;
            return super::link_dropped("write while disconnected");
        }
        self.written.push(payload.to_vec());
        Ok(())
    }

    async fn read_from_radio(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.drops == Some(Drops::Read) {
            self.connected = false;
            return super::link_dropped("read while disconnected");
        }
        match self.reads.pop_front() {
            Some(Read::Body(body)) => Ok(Some(body)),
            Some(Read::Empty) | None => Ok(None),
        }
    }

    async fn wait_for_data(&mut self) -> Result<(), Error> {
        self.waits += 1;
        if self.drops == Some(Drops::Wait) {
            self.connected = false;
            return super::link_dropped("link dropped while awaiting notification");
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

/// Reconnect tuning with no delay, so a backoff loop runs at test speed without
/// a paused clock.
fn instant_backoff() -> TransportConfig {
    TransportConfig {
        reconnect_initial_delay_secs: 0,
        reconnect_max_backoff_secs: 0,
        ..TransportConfig::default()
    }
}

fn a_from_radio(id: u32) -> FromRadio {
    FromRadio {
        id,
        payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(id)),
    }
}

fn a_to_radio(id: u32) -> ToRadio {
    ToRadio {
        payload_variant: Some(to_radio::PayloadVariant::WantConfigId(id)),
    }
}

fn encoded(message: &FromRadio) -> Vec<u8> {
    prost::Message::encode_to_vec(message)
}

/// Anti-vacuity: every case below leans on this fake reporting failure, so a
/// fake that could only succeed would make them all pass without testing
/// anything. Prove both outcomes are reachable on the same operation.
#[tokio::test]
async fn the_fake_peripheral_can_both_refuse_and_accept_a_scan() {
    let mut peripheral = FakePeripheral::advertising("Meshtastic_1234");

    peripheral
        .connect("Nothing")
        .await
        .expect_err("a prefix nothing advertises must not connect");
    peripheral
        .connect("Meshtastic")
        .await
        .expect("the advertised prefix must connect");
}

#[tokio::test]
async fn connect_fails_closed_when_no_device_advertises_the_prefix() {
    let peripheral = FakePeripheral::advertising("SomeOtherRadio");

    let error = BleTransport::connect("Meshtastic", peripheral)
        .await
        .err()
        .expect("connecting to an unadvertised prefix must fail");

    assert!(
        matches!(error, Error::BleConnect { .. }),
        "an absent device must surface as BleConnect, got {error}"
    );
}

#[tokio::test]
async fn send_writes_exactly_one_encoded_body() {
    let peripheral = FakePeripheral::advertising("Meshtastic_1234");
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    transport.send(a_to_radio(7)).await.expect("send");

    let written = &transport.peripheral().written;
    assert_eq!(written.len(), 1, "one send must write one body");
    let body = written.first().expect("one written body");
    let decoded: ToRadio =
        prost::Message::decode(body.as_slice()).expect("written body must decode");
    assert_eq!(
        decoded,
        a_to_radio(7),
        "the body must survive the round trip"
    );
}

#[tokio::test]
async fn recv_decodes_a_queued_body() {
    let peripheral = FakePeripheral::advertising("Meshtastic_1234")
        .with_reads(vec![Read::Body(encoded(&a_from_radio(42)))]);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    let received = transport.recv().await.expect("recv");

    assert_eq!(received, a_from_radio(42));
    assert_eq!(
        transport.peripheral().waits,
        0,
        "a body already queued must not wait for a notification"
    );
}

/// The behaviour the loop in `recv` exists for: an empty read means the radio is
/// quiet, so the transport must wait rather than return.
#[tokio::test]
async fn recv_waits_through_empty_reads_instead_of_returning() {
    let peripheral = FakePeripheral::advertising("Meshtastic_1234").with_reads(vec![
        Read::Empty,
        Read::Empty,
        Read::Body(encoded(&a_from_radio(9))),
    ]);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    let received = transport.recv().await.expect("recv");

    assert_eq!(received, a_from_radio(9));
    assert_eq!(
        transport.peripheral().waits,
        2,
        "each empty read must be followed by exactly one notification wait"
    );
}

#[tokio::test]
async fn recv_reports_a_dropped_link_rather_than_a_quiet_one() {
    let mut peripheral = FakePeripheral::advertising("Meshtastic_1234");
    peripheral.drops = Some(Drops::Read);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    let error = transport.recv().await.expect_err("a dropped read errors");

    assert!(
        matches!(error, Error::ConnectionLost { .. }),
        "a dropped link must not read as an empty queue, got {error}"
    );
    assert!(
        !transport.is_connected(),
        "a dropped read must mark the transport disconnected"
    );
}

#[tokio::test]
async fn a_failed_notification_wait_disconnects_the_transport() {
    let mut peripheral =
        FakePeripheral::advertising("Meshtastic_1234").with_reads(vec![Read::Empty]);
    peripheral.drops = Some(Drops::Wait);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    transport
        .recv()
        .await
        .expect_err("a dropped wait must error");

    assert!(
        !transport.is_connected(),
        "a dropped wait must mark the transport disconnected"
    );
}

#[tokio::test]
async fn a_failed_write_disconnects_the_transport() {
    let mut peripheral = FakePeripheral::advertising("Meshtastic_1234");
    peripheral.drops = Some(Drops::Write);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    transport
        .send(a_to_radio(1))
        .await
        .expect_err("a dropped write must error");

    assert!(
        !transport.is_connected(),
        "a failed write must mark the transport disconnected"
    );
}

#[tokio::test]
async fn recv_surfaces_an_undecodable_body_as_a_protobuf_error() {
    // A field-1 varint tag with no value: structurally a truncated message.
    let peripheral =
        FakePeripheral::advertising("Meshtastic_1234").with_reads(vec![Read::Body(vec![0x08])]);
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");

    let error = transport
        .recv()
        .await
        .expect_err("a truncated body must not decode");

    assert!(
        matches!(error, Error::ProtobufDecode { .. }),
        "a malformed body must surface as a decode error, got {error}"
    );
}

#[tokio::test]
async fn reconnect_retries_until_the_peripheral_accepts() {
    let peripheral = FakePeripheral::advertising("Meshtastic_1234");
    let config = instant_backoff();
    let mut transport = BleTransport::connect_with_config("Meshtastic", peripheral, &config)
        .await
        .expect("connect");
    assert_eq!(transport.peripheral().connect_calls, 1);

    transport.peripheral.connect_failures = 3;
    transport.reconnect().await.expect("reconnect must succeed");

    assert_eq!(
        transport.peripheral().connect_calls,
        5,
        "one initial connect, three scripted failures, then the accepting attempt"
    );
    assert!(
        transport.is_connected(),
        "a successful reconnect must restore the connected state"
    );
}

/// `is_connected` must not report the transport's own optimism when the
/// peripheral underneath it has gone.
#[tokio::test]
async fn is_connected_follows_the_peripheral() {
    let peripheral = FakePeripheral::advertising("Meshtastic_1234");
    let mut transport = BleTransport::connect("Meshtastic", peripheral)
        .await
        .expect("connect");
    assert!(transport.is_connected());

    transport.peripheral.connected = false;

    assert!(
        !transport.is_connected(),
        "a peripheral that has dropped must make the transport report disconnected"
    );
}
