//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use super::*;
use crate::baofeng::variant::{MAGIC_UV5R_291, bf_f8hp_config, uv5r_config, uv5rm_plus_config};
use crate::serial::mock::MockSerialPort;

// -----------------------------------------------------------------------
// Block planning tests
// -----------------------------------------------------------------------

#[test]
fn uv5r_download_has_no_aux_blocks() {
    let config = uv5r_config();
    let plan = download_plan(&config);
    assert!(
        plan.iter().all(|op| op.addr < MAIN_END),
        "UV-5R should not read beyond main memory"
    );
    assert!(
        !plan.iter().any(|op| op.is_warmup),
        "UV-5R should not have warmup reads"
    );
}

#[test]
fn uv5r_download_covers_full_main_region() {
    let config = uv5r_config();
    let plan = download_plan(&config);
    let expected_blocks = (MAIN_END - MAIN_START) / u16::try_from(BLOCK_SIZE).unwrap_or_default();
    assert_eq!(
        plan.len(),
        usize::try_from(expected_blocks).unwrap_or_default()
    );
    assert_eq!(
        plan.first().expect("download plan is non-empty").addr,
        MAIN_START
    );
    let last = plan.last().unwrap();
    assert_eq!(last.addr + last.size, MAIN_END);
}

#[test]
fn f8hp_download_includes_aux_warmup() {
    let config = bf_f8hp_config();
    let plan = download_plan(&config);
    let warmup_ops: Vec<_> = plan.iter().filter(|op| op.is_warmup).collect();
    assert_eq!(warmup_ops.len(), 1);
    assert_eq!(
        warmup_ops.first().expect("exactly one warm-up op").addr,
        AUX_WARMUP_ADDR
    );
}

#[test]
fn f8hp_download_reads_aux_region() {
    let config = bf_f8hp_config();
    let plan = download_plan(&config);
    let aux_ops: Vec<_> = plan
        .iter()
        .filter(|op| op.addr >= AUX_START && !op.is_warmup)
        .collect();
    assert!(!aux_ops.is_empty(), "F8HP should read aux region");

    // Verify aux region is fully covered
    let mut covered = vec![false; (AUX_END - AUX_START) as usize];
    for op in &aux_ops {
        let start = (op.addr - AUX_START) as usize;
        for slot in covered
            .iter_mut()
            .skip(start)
            .take(usize::try_from(op.size).unwrap_or_default())
        {
            *slot = true;
        }
    }
    assert!(
        covered.iter().all(|&c| c),
        "aux region not fully covered by read ops"
    );
}

#[test]
fn f8hp_download_splits_around_dropped_byte() {
    let config = bf_f8hp_config();
    let plan = download_plan(&config);
    // There should be a 1-byte read at the dropped byte address
    let single_byte_count = plan
        .iter()
        .filter(|op| op.addr == DROPPED_BYTE_ADDR && op.size == 1)
        .count();
    assert_eq!(
        single_byte_count, 1,
        "should have exactly one 1-byte read at dropped byte addr"
    );
}

#[test]
fn uv5r_upload_matches_download_without_warmup() {
    let config = uv5r_config();
    let dl = download_plan(&config);
    let ul = upload_plan(&config);
    assert_eq!(dl, ul, "UV-5R upload and download should be identical");
}

#[test]
fn f8hp_upload_has_no_warmup() {
    let config = bf_f8hp_config();
    let plan = upload_plan(&config);
    assert!(
        !plan.iter().any(|op| op.is_warmup),
        "upload should not have warmup reads"
    );
}

#[test]
fn f8hp_upload_covers_aux_region() {
    let config = bf_f8hp_config();
    let plan = upload_plan(&config);
    let aux_ops: Vec<_> = plan.iter().filter(|op| op.addr >= AUX_START).collect();
    assert!(!aux_ops.is_empty());
    let expected_aux_blocks = (AUX_END - AUX_START) / u16::try_from(BLOCK_SIZE).unwrap_or_default();
    assert_eq!(
        aux_ops.len(),
        usize::try_from(expected_aux_blocks).unwrap_or_default()
    );
}

#[test]
fn uv5rm_plus_download_matches_f8hp_structure() {
    let f8hp_plan = download_plan(&bf_f8hp_config());
    let rm_plan = download_plan(&uv5rm_plus_config());
    assert_eq!(f8hp_plan.len(), rm_plan.len());
    for (f, r) in f8hp_plan.iter().zip(rm_plan.iter()) {
        assert_eq!(f.addr, r.addr);
        assert_eq!(f.size, r.size);
        assert_eq!(f.is_warmup, r.is_warmup);
    }
}

#[test]
fn block_op_debug_format() {
    let op = BlockOp {
        addr: 0x1E80,
        size: 16,
        is_warmup: true,
    };
    let debug = format!("{op:?}");
    assert!(debug.contains("1E80") || debug.contains("7808"));
}

// -----------------------------------------------------------------------
// Protocol driver helpers
// -----------------------------------------------------------------------

fn make_protocol(mock: MockSerialPort) -> Uv5rProtocol<MockSerialPort> {
    Uv5rProtocol::new(mock)
}

/// Build a read-response packet for the given address and data.
fn read_response_packet(addr: u16, data: &[u8]) -> Vec<u8> {
    let mut pkt = vec![
        CMD_READ_RESPONSE,
        (addr >> 8) as u8,
        (addr & 0xFF) as u8,
        data.len() as u8,
    ];
    pkt.extend_from_slice(data);
    pkt
}

// -----------------------------------------------------------------------
// Magic byte / programming mode tests
// -----------------------------------------------------------------------

#[test]
fn enter_programming_mode_sends_magic_bytes() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[ACK]);

    let mut proto = make_protocol(mock);
    proto.enter_programming_mode(&MAGIC_UV5R_291).unwrap();

    // All 7 magic bytes should have been written.
    assert_eq!(&proto.port.written[..7], &MAGIC_UV5R_291);
}

#[test]
fn enter_programming_mode_bad_ack_returns_error() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[0xFF]);

    let mut proto = make_protocol(mock);
    let err = proto.enter_programming_mode(&MAGIC_UV5R_291).unwrap_err();

    assert!(matches!(
        err,
        ProtocolError::BadAck {
            expected: 0x06,
            got: 0xFF
        }
    ));
}

#[test]
fn enter_programming_mode_timeout_returns_error() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    let err = proto.enter_programming_mode(&MAGIC_UV5R_291).unwrap_err();

    assert!(matches!(err, ProtocolError::Timeout));
}

// -----------------------------------------------------------------------
// Ident tests
// -----------------------------------------------------------------------

#[test]
fn identify_parses_8_byte_response() {
    let mut mock = MockSerialPort::new();
    let ident_bytes = [0x42, 0x46, 0x42, 0x32, 0x39, 0x31, 0xAA, 0xBB];
    let mut response = ident_bytes.to_vec();
    response.push(IDENT_TERMINATOR);
    mock.enqueue_response(&response);
    mock.enqueue_response(&[ACK]); // Radio's ready ACK.

    let mut proto = make_protocol(mock);
    let ident = proto.identify().unwrap();

    assert_eq!(ident.raw_bytes.len(), 8);
    assert_eq!(ident.normalized, ident_bytes);
}

#[test]
fn identify_normalizes_12_byte_response() {
    let mut mock = MockSerialPort::new();
    let raw = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
    ];
    let mut response = raw.to_vec();
    response.push(IDENT_TERMINATOR);
    mock.enqueue_response(&response);
    mock.enqueue_response(&[ACK]);

    let mut proto = make_protocol(mock);
    let ident = proto.identify().unwrap();

    assert_eq!(ident.raw_bytes.len(), 12);
    assert_eq!(
        ident.normalized,
        [0x01, 0x04, 0x06, 0x08, 0x09, 0x0A, 0x0B, 0x0C]
    );
}

#[test]
fn identify_timeout_returns_error() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    let err = proto.identify().unwrap_err();
    assert!(matches!(err, ProtocolError::Timeout));
}

#[test]
fn identify_odd_length_returns_ident_failed() {
    let mut mock = MockSerialPort::new();
    let raw = [0x01, 0x02, 0x03, 0x04, 0x05];
    let mut response = raw.to_vec();
    response.push(IDENT_TERMINATOR);
    mock.enqueue_response(&response);

    let mut proto = make_protocol(mock);
    let err = proto.identify().unwrap_err();
    assert!(matches!(err, ProtocolError::IdentFailed));
}

// -----------------------------------------------------------------------
// Read block tests
// -----------------------------------------------------------------------

#[test]
fn read_block_constructs_correct_packet() {
    let mut mock = MockSerialPort::new();
    let addr: u16 = 0x1234;
    let len: u8 = 0x40;
    let payload = vec![0xAA; usize::from(len)];

    let resp = read_response_packet(addr, &payload);
    mock.enqueue_response(&resp);

    let mut proto = make_protocol(mock);
    let data = proto.read_block(addr, len).unwrap();

    assert_eq!(data, payload);
    // Verify the request packet: [0x53, 0x12, 0x34, 0x40].
    assert_eq!(&proto.port.written[..4], &[CMD_READ, 0x12, 0x34, 0x40]);
}

#[test]
fn read_block_bad_header_returns_error() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[CMD_READ_RESPONSE, 0xFF, 0xFF, 0x40]);
    mock.enqueue_response(&[0u8; 64]);

    let mut proto = make_protocol(mock);
    let err = proto.read_block(0x0100, 0x40).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::BadResponseHeader { addr: 0x0100 }
    ));
}

#[test]
fn read_block_timeout_on_partial_header() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[CMD_READ_RESPONSE]);

    let mut proto = make_protocol(mock);
    let err = proto.read_block(0x0000, 0x40).unwrap_err();
    assert!(matches!(err, ProtocolError::Timeout));
}

// -----------------------------------------------------------------------
// Write block tests
// -----------------------------------------------------------------------

#[test]
fn write_block_constructs_correct_packet() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[ACK]);

    let data = [0xBB; 16];
    let mut proto = make_protocol(mock);
    proto.write_block(0x0100, &data).unwrap();

    // Packet: [0x58, 0x01, 0x00, 0x10, ...16 bytes data]
    assert_eq!(
        proto.port.written.get(0).copied().unwrap_or_default(),
        CMD_WRITE
    );
    assert_eq!(proto.port.written.get(1).copied().unwrap_or_default(), 0x01);
    assert_eq!(proto.port.written.get(2).copied().unwrap_or_default(), 0x00);
    assert_eq!(proto.port.written.get(3).copied().unwrap_or_default(), 0x10);
    assert_eq!(&proto.port.written[4..20], &data);
}

#[test]
fn write_block_receives_ack() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[ACK]);

    let data = [0xCC; 16];
    let mut proto = make_protocol(mock);
    assert!(proto.write_block(0x0200, &data).is_ok());
}

// -----------------------------------------------------------------------
// Forbidden address tests
// -----------------------------------------------------------------------

#[test]
fn write_block_rejects_forbidden_0x1f00() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    let err = proto.write_block(0x1F00, &[0x00; 16]).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::ForbiddenAddress { addr: 0x1F00 }
    ));
}

#[test]
fn write_block_rejects_forbidden_0x1fd0() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    let err = proto.write_block(0x1FD0, &[0x00; 16]).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::ForbiddenAddress { addr: 0x1FD0 }
    ));
}

#[test]
fn write_block_rejects_overlapping_forbidden_range() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    // 0x1EF0 + 32 → 0x1F10, overlaps forbidden 0x1F00–0x1F60.
    let err = proto.write_block(0x1EF0, &[0x00; 32]).unwrap_err();
    assert!(matches!(
        err,
        ProtocolError::ForbiddenAddress { addr: 0x1EF0 }
    ));
}

#[test]
fn write_block_allows_safe_address() {
    let mut mock = MockSerialPort::new();
    mock.enqueue_response(&[ACK]);

    let mut proto = make_protocol(mock);
    assert!(proto.write_block(0x0100, &[0x00; 16]).is_ok());
}

#[test]
fn write_block_no_serial_io_on_forbidden_address() {
    let mock = MockSerialPort::new();
    let mut proto = make_protocol(mock);
    let _ = proto.write_block(0x1F40, &[0x00; 16]);
    assert!(proto.port.written.is_empty());
}

// -----------------------------------------------------------------------
// Retry tests
// -----------------------------------------------------------------------

#[test]
fn read_block_retries_on_first_failure() {
    let mut mock = MockSerialPort::new();
    let addr: u16 = 0x0100;
    let len: u8 = 0x40;
    let payload = vec![0xEE; usize::from(len)];

    // First attempt: timeout (no data) → will fail.
    // (empty queue causes TimedOut on the second read_block's header read)
    // Second attempt: correct response.
    mock.enqueue_response(&read_response_packet(addr, &payload));

    let mut proto = make_protocol(mock);
    let data = proto.read_block_with_retry(addr, len).unwrap();
    assert_eq!(data, payload);
}

#[test]
fn read_block_retry_exhaustion_returns_error() {
    let mock = MockSerialPort::new();
    let addr: u16 = 0x0100;
    let len: u8 = 0x40;

    // No data at all → all 3 attempts time out.

    let mut proto = make_protocol(mock);
    let err = proto.read_block_with_retry(addr, len).unwrap_err();
    let ProtocolError::RetryExhausted {
        addr: failed_addr,
        attempts,
        source,
    } = err
    else {
        panic!("expected RetryExhausted, got: {err:?}");
    };
    assert_eq!(failed_addr, 0x0100);
    assert_eq!(attempts, 3);
    assert!(
        matches!(*source, ProtocolError::Timeout),
        "a silent radio should exhaust retries on Timeout, got: {source:?}"
    );
}

// WHY: the falsifying sibling to the test above. Both transfers give up after
// the same three attempts, and before the cause was carried they produced
// byte-identical errors — so "the radio never answered" and "the radio
// answered with the wrong block" were indistinguishable to the operator.
#[test]
fn read_block_retry_exhaustion_carries_a_header_mismatch_cause() {
    let mut mock = MockSerialPort::new();
    let addr: u16 = 0x0100;
    let len: u8 = 0x40;

    // Three attempts, each answered with a header for the wrong address.
    for _ in 0..3 {
        mock.enqueue_response(&[CMD_READ_RESPONSE, 0xFF, 0xFF, len]);
    }

    let mut proto = make_protocol(mock);
    let err = proto.read_block_with_retry(addr, len).unwrap_err();
    let ProtocolError::RetryExhausted { source, .. } = err else {
        panic!("expected RetryExhausted, got: {err:?}");
    };
    assert!(
        matches!(*source, ProtocolError::BadResponseHeader { addr: 0x0100 }),
        "expected the header mismatch to survive the retry loop, got: {source:?}"
    );
}

#[test]
fn write_block_retry_exhaustion_carries_its_cause() {
    let mock = MockSerialPort::new();
    let addr: u16 = 0x0100;

    let mut proto = make_protocol(mock);
    let err = proto.write_block_with_retry(addr, &[0u8; 16]).unwrap_err();
    let ProtocolError::RetryExhausted { source, .. } = err else {
        panic!("expected RetryExhausted, got: {err:?}");
    };
    assert!(
        matches!(*source, ProtocolError::Timeout),
        "expected the underlying timeout, got: {source:?}"
    );
}

// -----------------------------------------------------------------------
// Download tests
// -----------------------------------------------------------------------

#[test]
fn download_image_reads_correct_address_sequence() {
    let mut mock = MockSerialPort::new();

    // Main block: 0x0000..0x1800 in 64-byte chunks.
    let main_blocks = (MAIN_BLOCK_END - MAIN_BLOCK_START) / u16::from(READ_BLOCK_SIZE);
    for i in 0..main_blocks {
        let addr = MAIN_BLOCK_START + i * u16::from(READ_BLOCK_SIZE);
        let data = vec![u8::try_from(i).unwrap_or_default(); usize::from(READ_BLOCK_SIZE)];
        mock.enqueue_response(&read_response_packet(addr, &data));
    }

    // Aux block: 0x1E80..0x2000 in 16-byte chunks.
    let aux_blocks = (AUX_BLOCK_END - AUX_BLOCK_START) / u16::from(AUX_READ_BLOCK_SIZE);
    for i in 0..aux_blocks {
        let addr = AUX_BLOCK_START + i * u16::from(AUX_READ_BLOCK_SIZE);
        let data = vec![(128 + i) as u8; usize::from(AUX_READ_BLOCK_SIZE)];
        mock.enqueue_response(&read_response_packet(addr, &data));
    }

    let mut proto = make_protocol(mock);
    let image = proto.download_image(&bf_f8hp_config()).unwrap();

    assert_eq!(image.len(), usize::from(AUX_BLOCK_END));
    // First main block byte.
    assert_eq!(image.read_bytes(0x0000, 1), &[0u8][..]);
    // Last aux block byte.
    let last_aux_addr = AUX_BLOCK_END - u16::from(AUX_READ_BLOCK_SIZE);
    let expected_val = (128 + aux_blocks - 1) as u8;
    assert_eq!(image.read_bytes(last_aux_addr, 1), &[expected_val][..]);
}

// -----------------------------------------------------------------------
// Upload tests
// -----------------------------------------------------------------------

#[test]
fn upload_only_writes_safe_ranges() {
    let total_safe_bytes: usize = UPLOAD_RANGES_MAIN
        .iter()
        .chain(UPLOAD_RANGES_AUX.iter())
        .map(|(s, e)| usize::from(e - s))
        .sum();
    let total_blocks = total_safe_bytes / usize::from(WRITE_BLOCK_SIZE);

    let mut mock = MockSerialPort::new();
    for _ in 0..total_blocks {
        mock.enqueue_response(&[ACK]);
    }

    let image = MemoryImage::new(usize::from(AUX_BLOCK_END));
    let mut progress_calls = Vec::new();
    let mut proto = make_protocol(mock);

    proto
        .upload_image(&bf_f8hp_config(), &image, &mut |current, total| {
            progress_calls.push((current, total));
        })
        .unwrap();

    assert_eq!(progress_calls.len(), total_blocks);
    assert_eq!(progress_calls.last(), Some(&(total_blocks, total_blocks)));
}

#[test]
fn upload_calls_progress_callback() {
    let total_safe_bytes: usize = UPLOAD_RANGES_MAIN
        .iter()
        .chain(UPLOAD_RANGES_AUX.iter())
        .map(|(s, e)| usize::from(e - s))
        .sum();
    let total_blocks = total_safe_bytes / usize::from(WRITE_BLOCK_SIZE);

    let mut mock = MockSerialPort::new();
    for _ in 0..total_blocks {
        mock.enqueue_response(&[ACK]);
    }

    let image = MemoryImage::new(usize::from(AUX_BLOCK_END));
    let mut called = false;
    let mut proto = make_protocol(mock);

    proto
        .upload_image(&bf_f8hp_config(), &image, &mut |_current, _total| {
            called = true;
        })
        .unwrap();

    assert!(called);
}

// -----------------------------------------------------------------------
// MemoryImage tests
// -----------------------------------------------------------------------

#[test]
fn memory_image_read_write_roundtrip() {
    let mut img = MemoryImage::new(256);
    let data = [0xDE, 0xAD, 0xBE, 0xEF];
    img.write_bytes(0x10, &data);
    assert_eq!(img.read_bytes(0x10, 4), &data[..]);
}

#[test]
// NOTE: read_bytes documents a panic-on-out-of-bounds contract (see `image.rs` `# Panics`);
// this verifies the invariant rather than a graceful `None` the API no longer returns.
#[should_panic]
fn memory_image_out_of_bounds_read_panics() {
    let img = MemoryImage::new(16);
    let _ = img.read_bytes(0x10, 1);
}

#[test]
// NOTE: write_bytes documents a panic-on-out-of-bounds contract (see `image.rs` `# Panics`);
// this verifies the invariant rather than a graceful `false` the API no longer returns.
#[should_panic]
fn memory_image_out_of_bounds_write_panics() {
    let mut img = MemoryImage::new(16);
    img.write_bytes(0x10, &[0xFF]);
}

#[test]
// NOTE: MemoryImage::new represents erased EEPROM state (0xFF), not zero.
fn memory_image_ff_filled_on_creation() {
    let img = MemoryImage::new(8);
    assert_eq!(img.as_slice(), &[0xFF; 8]);
}

#[test]
fn memory_image_len_and_is_empty() {
    let img = MemoryImage::new(42);
    assert_eq!(img.len(), 42);
    assert!(!img.is_empty());

    let empty = MemoryImage::new(0);
    assert!(empty.is_empty());
}

#[test]
fn memory_image_from_bytes() {
    let data = vec![1, 2, 3, 4];
    let img = MemoryImage::from_bytes(data.clone());
    assert_eq!(img.as_slice(), &data);
}

// -----------------------------------------------------------------------
// is_forbidden tests
// -----------------------------------------------------------------------

#[test]
fn is_forbidden_detects_exact_range_start() {
    assert!(is_forbidden(0x1F00, 1));
}

#[test]
fn is_forbidden_detects_overlap_from_below() {
    assert!(is_forbidden(0x1EF0, 32));
}

#[test]
fn is_forbidden_allows_address_before_range() {
    assert!(!is_forbidden(0x0100, 16));
}

#[test]
fn is_forbidden_allows_gap_between_ranges() {
    // 0x1F60..0x1F70 is between forbidden ranges.
    assert!(!is_forbidden(0x1F60, 16));
}

// -----------------------------------------------------------------------
// RadioIdent unit tests
// -----------------------------------------------------------------------

#[test]
fn radio_ident_firmware_prefix_from_ascii() {
    let raw = [b'B', b'F', b'B', b'2', b'9', b'1', 0x00, 0x01];
    let ident = RadioIdent::from_raw(&raw).unwrap();
    assert_eq!(ident.firmware_prefix, "BFB291");
}

#[test]
fn radio_ident_rejects_odd_length() {
    assert!(RadioIdent::from_raw(&[1, 2, 3]).is_none());
}

// -----------------------------------------------------------------------
// Variant-aware live-driver I/O (#225)
// -----------------------------------------------------------------------

/// Script exactly the main-block reads a UV-5R download should issue.
fn enqueue_main_block_reads(mock: &mut MockSerialPort) {
    let mut addr = MAIN_BLOCK_START;
    while addr < MAIN_BLOCK_END {
        let data = vec![0u8; usize::from(READ_BLOCK_SIZE)];
        mock.enqueue_response(&read_response_packet(addr, &data));
        addr += u16::from(READ_BLOCK_SIZE);
    }
}

#[test]
fn download_skips_aux_block_for_variant_without_one() {
    // WARNING: the mock is scripted with main-block reads ONLY. If the driver
    // issues the aux warm-up the way it did before #225, the queue is empty by
    // then and the read fails — which is what makes this assertion able to go
    // red rather than merely describing current behaviour.
    let mut mock = MockSerialPort::new();
    enqueue_main_block_reads(&mut mock);

    let mut proto = make_protocol(mock);
    let image = proto
        .download_image(&uv5r_config())
        .expect("plain UV-5R download must not touch the aux block");

    assert_eq!(image.len(), usize::from(MAIN_BLOCK_END));
}

#[test]
fn download_still_reads_aux_block_for_f8hp() {
    let mut mock = MockSerialPort::new();
    enqueue_main_block_reads(&mut mock);
    let aux_blocks = (AUX_BLOCK_END - AUX_BLOCK_START) / u16::from(AUX_READ_BLOCK_SIZE);
    for i in 0..=aux_blocks {
        let addr = AUX_BLOCK_START + i * u16::from(AUX_READ_BLOCK_SIZE);
        let data = vec![0u8; usize::from(AUX_READ_BLOCK_SIZE)];
        mock.enqueue_response(&read_response_packet(addr, &data));
    }

    let mut proto = make_protocol(mock);
    let image = proto
        .download_image(&bf_f8hp_config())
        .expect("F8HP download covers the aux block");

    assert_eq!(image.len(), usize::from(AUX_BLOCK_END));
}

#[test]
fn upload_skips_aux_ranges_for_variant_without_one() {
    let main_blocks: usize = UPLOAD_RANGES_MAIN
        .iter()
        .map(|(s, e)| usize::from(e - s) / usize::from(WRITE_BLOCK_SIZE))
        .sum();

    // Only enough ACKs for the main ranges: an aux write would run the mock dry.
    let mut mock = MockSerialPort::new();
    for _ in 0..main_blocks {
        mock.enqueue_response(&[ACK]);
    }

    let image = MemoryImage::new(usize::from(MAIN_BLOCK_END));
    let mut progress_calls = Vec::new();
    let mut proto = make_protocol(mock);

    proto
        .upload_image(&uv5r_config(), &image, &mut |current, total| {
            progress_calls.push((current, total));
        })
        .expect("plain UV-5R upload must not write the aux ranges");

    assert_eq!(progress_calls.len(), main_blocks);
    assert_eq!(progress_calls.last(), Some(&(main_blocks, main_blocks)));
}

#[test]
fn upload_refuses_an_image_shorter_than_the_ranges_it_would_write() {
    let mut proto = make_protocol(MockSerialPort::new());
    // A standard `.img` import is 0x1800 — short of the aux ranges an F8HP writes.
    let image = MemoryImage::new(usize::from(MAIN_BLOCK_END));

    let err = proto
        .upload_image(&bf_f8hp_config(), &image, &mut |_, _| {})
        .expect_err("a short image must be refused, not panic mid-transfer");

    assert!(
        matches!(err, ProtocolError::ImageTooShort { .. }),
        "expected ImageTooShort, got {err:?}"
    );
}
