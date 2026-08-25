//! Allocation-free progression across multiple ordered publication stages.
//!
//! A staged publication is still caller-owned immutable output. This module adds only enough
//! per-connection state to identify the current stage and the next body within that stage. It does
//! not know what a stage means, own publication bytes, select Minecraft packet identities, or add
//! another queue.

use core::mem::size_of;

use crucible_connection_driver::{ConnectionDriver, DriverError};

use crate::{PublicationCursor, PublicationStep, publish_one};

/// Allocation-free progress through an ordered sequence of immutable publication stages.
///
/// `stage` is the next stage that has not completed. `body` is the ordinary one-word cursor within
/// that stage. The caller owns the stage sequence and its semantic meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StagedPublicationCursor {
    stage: usize,
    body: PublicationCursor,
}

impl StagedPublicationCursor {
    /// Cursor positioned before the first body of the first stage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stage: 0,
            body: PublicationCursor::new(),
        }
    }

    /// Index of the next stage that has not completed.
    #[must_use]
    pub const fn stage_index(self) -> usize {
        self.stage
    }

    /// Index of the next body not yet admitted within the current stage.
    #[must_use]
    pub const fn body_index(self) -> usize {
        self.body.next_index()
    }

    /// Whether every supplied stage has completed.
    #[must_use]
    pub const fn is_complete<S>(self, stages: &[S]) -> bool {
        self.stage >= stages.len()
    }
}

/// Result of one bounded staged-publication service opportunity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StagedPublicationStep {
    /// Every stage was already complete; neither cursor nor egress changed.
    Complete,
    /// The current stage was complete and exactly one stage boundary was committed.
    ///
    /// No packet body is queued by this step. Even empty stages therefore consume one explicit
    /// service opportunity instead of being skipped in an unbounded loop.
    StageComplete {
        /// Stage that was completed by this service opportunity.
        stage: usize,
    },
    /// Exactly one body entered bounded egress and the within-stage cursor advanced once.
    Queued {
        /// Stage containing the admitted body.
        stage: usize,
        /// Index of the body within that stage.
        index: usize,
        /// Packet-body bytes admitted, excluding the outer frame-length prefix.
        body_bytes: usize,
    },
}

/// Services at most one body or one stage boundary through the existing bounded egress.
///
/// Each element in `stages` must expose an immutable slice of packet bodies. The semantic meaning
/// and ordering of stages remain caller-owned. This function never loops across stage boundaries:
/// one call either queues one body, commits one completed stage, or observes that the whole plan is
/// complete.
///
/// Queue rejection inherits [`publish_one`]'s transactional law: neither the staged cursor nor
/// existing egress changes. A stage advances only after its ordinary body cursor reports
/// [`PublicationStep::Complete`]. The body cursor is then reset before the next stage becomes
/// eligible.
///
/// # Errors
///
/// Returns the underlying driver error for the next body without advancing either cursor.
pub fn publish_staged_one<E, B, S>(
    stages: &[S],
    cursor: &mut StagedPublicationCursor,
    driver: &mut ConnectionDriver,
) -> Result<StagedPublicationStep, DriverError<E>>
where
    B: AsRef<[u8]>,
    S: AsRef<[B]>,
{
    let stage_index = cursor.stage;
    let Some(stage) = stages.get(stage_index) else {
        return Ok(StagedPublicationStep::Complete);
    };

    match publish_one::<E, _>(stage.as_ref(), &mut cursor.body, driver)? {
        PublicationStep::Queued { index, body_bytes } => Ok(StagedPublicationStep::Queued {
            stage: stage_index,
            index,
            body_bytes,
        }),
        PublicationStep::Complete => {
            // `stages.get(stage_index)` succeeding proves `stage_index < stages.len() <= usize::MAX`,
            // so this increment cannot overflow after the stage-completion decision commits.
            cursor.stage = stage_index + 1;
            cursor.body = PublicationCursor::new();
            Ok(StagedPublicationStep::StageComplete { stage: stage_index })
        }
    }
}

const _: () = assert!(size_of::<StagedPublicationCursor>() == 2 * size_of::<usize>());

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
    use crucible_connection_driver::{ConnectionDriver, DriverError};

    use super::{StagedPublicationCursor, StagedPublicationStep, publish_staged_one};

    fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(max_body, max_body + 5, egress)
            .expect("coherent staged-publication limits")
    }

    fn body(value: u8) -> Vec<u8> {
        vec![value]
    }

    #[test]
    fn cursor_is_exactly_two_machine_words() {
        assert_eq!(
            size_of::<StagedPublicationCursor>(),
            2 * size_of::<usize>()
        );
        let cursor = StagedPublicationCursor::new();
        assert_eq!(cursor.stage_index(), 0);
        assert_eq!(cursor.body_index(), 0);
    }

    #[test]
    fn empty_plan_is_stable_noop() {
        let stages: [Vec<Vec<u8>>; 0] = [];
        let mut cursor = StagedPublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(8, 32));

        for _ in 0..3 {
            assert_eq!(
                publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
                Ok(StagedPublicationStep::Complete)
            );
            assert!(cursor.is_complete(&stages));
            assert_eq!(cursor, StagedPublicationCursor::new());
            assert_eq!(driver.queued_egress(), 0);
        }
    }

    #[test]
    fn one_call_never_crosses_more_than_one_stage_boundary() {
        let stages = [Vec::<Vec<u8>>::new(), Vec::new(), vec![body(0x55)]];
        let mut cursor = StagedPublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(8, 32));

        assert_eq!(
            publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
            Ok(StagedPublicationStep::StageComplete { stage: 0 })
        );
        assert_eq!(cursor.stage_index(), 1);
        assert_eq!(driver.queued_egress(), 0);

        assert_eq!(
            publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
            Ok(StagedPublicationStep::StageComplete { stage: 1 })
        );
        assert_eq!(cursor.stage_index(), 2);
        assert_eq!(driver.queued_egress(), 0);

        assert_eq!(
            publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
            Ok(StagedPublicationStep::Queued {
                stage: 2,
                index: 0,
                body_bytes: 1,
            })
        );
    }

    #[test]
    fn stage_completion_resets_body_progress_before_next_stage() {
        let stages = [vec![body(0x11), body(0x12)], vec![body(0x21)]];
        let mut cursor = StagedPublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(8, 64));

        for expected_index in 0..2 {
            assert!(matches!(
                publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
                Ok(StagedPublicationStep::Queued {
                    stage: 0,
                    index,
                    ..
                }) if index == expected_index
            ));
        }
        assert_eq!(cursor.body_index(), 2);

        assert_eq!(
            publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
            Ok(StagedPublicationStep::StageComplete { stage: 0 })
        );
        assert_eq!(cursor.stage_index(), 1);
        assert_eq!(cursor.body_index(), 0);

        assert_eq!(
            publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver),
            Ok(StagedPublicationStep::Queued {
                stage: 1,
                index: 0,
                body_bytes: 1,
            })
        );
    }

    #[test]
    fn backpressure_changes_neither_staged_cursor_nor_existing_egress() {
        let stages = [vec![vec![0x10; 8], vec![0x20; 8]]];
        let mut cursor = StagedPublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(8, 9));

        publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver)
            .expect("first exact-fit frame queues");
        let cursor_before = cursor;
        let egress_before = driver.pending_egress().to_vec();

        let error = publish_staged_one::<(), Vec<u8>, _>(&stages, &mut cursor, &mut driver)
            .expect_err("second frame must observe bounded backpressure");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(cursor, cursor_before);
        assert_eq!(driver.pending_egress(), egress_before);
    }

    #[test]
    fn exhaustive_small_stage_shapes_publish_every_body_once_in_order() {
        // Exhaust all four-stage plans with 0..=3 bodies per stage (4^4 = 256 plans). Each body has
        // a unique stage/index byte, so duplicates, skips and cross-stage reordering are observable.
        for a in 0_usize..=3 {
            for b in 0_usize..=3 {
                for c in 0_usize..=3 {
                    for d in 0_usize..=3 {
                        let lengths = [a, b, c, d];
                        let stages: Vec<Vec<Vec<u8>>> = lengths
                            .into_iter()
                            .enumerate()
                            .map(|(stage, len)| {
                                (0..len)
                                    .map(|index| {
                                        vec![
                                            u8::try_from(stage).expect("small stage"),
                                            u8::try_from(index).expect("small body index"),
                                        ]
                                    })
                                    .collect()
                            })
                            .collect();
                        let expected: Vec<Vec<u8>> =
                            stages.iter().flat_map(|stage| stage.iter().cloned()).collect();

                        let mut cursor = StagedPublicationCursor::new();
                        let mut driver = ConnectionDriver::new(limits(8, 16));
                        let mut observed = Vec::new();
                        let mut stage_completions = 0_usize;
                        let mut calls = 0_usize;

                        loop {
                            calls += 1;
                            assert!(calls <= expected.len() + stages.len() + 1);
                            match publish_staged_one::<(), Vec<u8>, _>(
                                &stages,
                                &mut cursor,
                                &mut driver,
                            )
                            .expect("small plan must fit")
                            {
                                StagedPublicationStep::Queued { .. } => {
                                    // Two-byte bodies have one-byte frame lengths. Decode only the
                                    // deterministic test framing needed to compare publication order.
                                    assert_eq!(driver.pending_egress()[0], 2);
                                    observed.push(driver.pending_egress()[1..3].to_vec());
                                    let queued = driver.queued_egress();
                                    driver.consume_written(queued).expect("drain test frame");
                                }
                                StagedPublicationStep::StageComplete { .. } => {
                                    stage_completions += 1;
                                }
                                StagedPublicationStep::Complete => break,
                            }
                        }

                        assert_eq!(observed, expected, "lengths={lengths:?}");
                        assert_eq!(stage_completions, stages.len(), "lengths={lengths:?}");
                        assert!(cursor.is_complete(&stages));
                        assert_eq!(cursor.body_index(), 0);
                    }
                }
            }
        }
    }
}
