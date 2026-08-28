#[path = "../src/config_publication.rs"]
mod config_publication;

use std::mem::size_of;

use config_publication::{PublicationCursor, PublicationImage, PublicationStep, publish_one};
use helve_connection_core::{ConnectionBufferError, ConnectionLimits};
use helve_connection_driver::{ConnectionDriver, DriverError};

fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
    ConnectionLimits::new(max_body, max_body + 5, egress).expect("coherent publication limits")
}

fn body(packet_id: u8, len: usize) -> Vec<u8> {
    assert!(len >= 1, "synthetic body includes one-byte packet id");
    assert!(
        packet_id < 0x80,
        "synthetic helper uses one-byte packet ids"
    );
    let mut body = Vec::with_capacity(len);
    body.push(packet_id);
    for index in 1..len {
        let value = u8::try_from(index % 251).expect("modulo fits u8");
        body.push(value ^ packet_id);
    }
    body
}

fn image(lengths: &[usize]) -> PublicationImage {
    let bodies = lengths
        .iter()
        .enumerate()
        .map(|(index, length)| {
            let packet_id = u8::try_from(index + 1).expect("small synthetic packet id");
            body(packet_id, *length)
        })
        .collect();
    PublicationImage::from_bodies(bodies).expect("small synthetic image")
}

fn reference_stream(image: &PublicationImage, max_body: usize) -> Vec<u8> {
    let mut driver = ConnectionDriver::new(limits(max_body, 64 * 1_024));
    for body in image.bodies() {
        driver
            .queue_frame::<()>(body.as_ref())
            .expect("reference image fits");
    }
    driver.pending_egress().to_vec()
}

fn drain_with_pattern(
    image: &PublicationImage,
    max_body: usize,
    egress: usize,
    pattern: &[usize],
) -> Vec<u8> {
    assert!(!pattern.is_empty());
    let mut driver = ConnectionDriver::new(limits(max_body, egress));
    let mut cursor = PublicationCursor::new();
    let mut observed = Vec::new();
    let mut pattern_index = 0usize;

    while !cursor.is_complete(image) || driver.queued_egress() != 0 {
        if !cursor.is_complete(image) {
            match publish_one::<()>(image, &mut cursor, &mut driver) {
                Ok(PublicationStep::Queued { .. } | PublicationStep::Complete)
                | Err(DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })) => {}
                Err(error) => panic!("unexpected publication error: {error:?}"),
            }
        }

        let pending = driver.pending_egress();
        if pending.is_empty() {
            continue;
        }
        let requested = pattern[pattern_index % pattern.len()];
        pattern_index += 1;
        let written = requested.min(pending.len()).max(1);
        observed.extend_from_slice(&pending[..written]);
        driver
            .consume_written::<()>(written)
            .expect("simulated partial write is exact");
    }

    observed
}

#[test]
fn empty_publication_is_a_noop() {
    let image = image(&[]);
    let mut cursor = PublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(16, 17));

    assert_eq!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Ok(PublicationStep::Complete)
    );
    assert_eq!(cursor.next_index(), 0);
    assert!(cursor.is_complete(&image));
    assert_eq!(driver.queued_egress(), 0);
}

#[test]
fn candidate_is_byte_exact_with_repeated_reference_queueing() {
    let image = image(&[1, 2, 15, 16, 31, 63, 127]);
    let expected = reference_stream(&image, 127);
    let mut cursor = PublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(127, 4 * 1_024));

    for expected_index in 0..image.frame_count() {
        assert!(matches!(
            publish_one::<()>(&image, &mut cursor, &mut driver),
            Ok(PublicationStep::Queued { index, .. }) if index == expected_index
        ));
    }
    assert_eq!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Ok(PublicationStep::Complete)
    );
    assert!(cursor.is_complete(&image));
    assert_eq!(driver.pending_egress(), expected);
}

#[test]
fn capacity_rejection_preserves_cursor_and_existing_egress() {
    let image = image(&[16, 16]);
    let mut cursor = PublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(16, 17));

    assert!(matches!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Ok(PublicationStep::Queued { index: 0, .. })
    ));
    assert_eq!(cursor.next_index(), 1);
    let before = driver.pending_egress().to_vec();

    assert!(matches!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Err(DriverError::Buffer(
            ConnectionBufferError::EgressLimitExceeded {
                queued: 17,
                frame_bytes: 17,
                maximum: 17,
            }
        ))
    ));
    assert_eq!(cursor.next_index(), 1);
    assert_eq!(driver.pending_egress(), before);
}

#[test]
fn partial_drain_then_compaction_allows_exact_resume() {
    let image = image(&[16, 16]);
    let expected = reference_stream(&image, 16);
    let mut cursor = PublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(16, 17));
    let mut observed = Vec::new();

    publish_one::<()>(&image, &mut cursor, &mut driver).expect("first frame queues");
    observed.extend_from_slice(&driver.pending_egress()[..5]);
    driver
        .consume_written::<()>(5)
        .expect("first partial write commits");

    assert!(matches!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Err(DriverError::Buffer(
            ConnectionBufferError::EgressLimitExceeded { .. }
        ))
    ));
    assert_eq!(cursor.next_index(), 1);

    let remaining = driver.pending_egress().to_vec();
    observed.extend_from_slice(&remaining);
    driver
        .consume_written::<()>(remaining.len())
        .expect("first frame drains");

    publish_one::<()>(&image, &mut cursor, &mut driver).expect("second frame queues after drain");
    assert!(cursor.is_complete(&image));
    let final_bytes = driver.pending_egress().to_vec();
    observed.extend_from_slice(&final_bytes);
    driver
        .consume_written::<()>(final_bytes.len())
        .expect("second frame drains");

    assert_eq!(observed, expected);
    assert_eq!(driver.queued_egress(), 0);
}

#[test]
fn irregular_partial_writes_preserve_complete_stream() {
    let image = image(&[5, 16, 7, 31, 3, 63, 8, 24]);
    let expected = reference_stream(&image, 63);
    let observed = drain_with_pattern(&image, 63, 64, &[1, 7, 2, 13, 5, 3, 17]);
    assert_eq!(observed, expected);
}

#[test]
fn two_connections_share_image_but_not_cursor_or_backpressure() {
    let image = image(&[8, 16, 31, 4, 63, 7]);
    let original = image
        .bodies()
        .iter()
        .map(|body| body.to_vec())
        .collect::<Vec<_>>();
    let expected = reference_stream(&image, 63);

    let fast = drain_with_pattern(&image, 63, 128, &[127]);
    let slow = drain_with_pattern(&image, 63, 64, &[1, 1, 2, 1, 3]);

    assert_eq!(fast, expected);
    assert_eq!(slow, expected);
    for (body, before) in image.bodies().iter().zip(&original) {
        assert_eq!(body.as_ref(), before);
    }
}

#[test]
fn oversized_next_body_is_rejected_without_cursor_progress() {
    let image = image(&[17]);
    let mut cursor = PublicationCursor::new();
    let mut driver = ConnectionDriver::new(limits(16, 17));

    assert!(matches!(
        publish_one::<()>(&image, &mut cursor, &mut driver),
        Err(DriverError::Buffer(ConnectionBufferError::Wire(_)))
    ));
    assert_eq!(cursor.next_index(), 0);
    assert_eq!(driver.queued_egress(), 0);
}

#[test]
fn publication_cursor_is_one_machine_word_and_allocation_free_state() {
    assert_eq!(size_of::<PublicationCursor>(), size_of::<usize>());
    let image = image(&[1, 2, 3]);
    assert_eq!(image.body_bytes(), 6);
}
