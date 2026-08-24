//! Bounded I/O service path for targets with proactive publication work.
//!
//! The ordinary [`crate::PrePlayIo::service_once`] path remains unchanged for inbound-driven
//! Status/Login targets. This module adds an opt-in service law for [`PrePlayPublisher`] targets:
//! buffered inbound work keeps priority, and when it reaches `Incomplete` the adapter may commit at
//! most one proactive publication step before yielding the service turn.

use std::io::{Read, Write};

use crucible_preplay_core::{PrePlayPublicationProcess, PrePlayPublisher, PublicationStep};

use super::{ActionBudget, PrePlayIo, PrePlayIoError, ProcessStop, ReadOutcome, WriteOutcome, add};

/// Why one publication-aware I/O service step returned successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationServiceStop {
    /// Neither buffered inbound work nor proactive publication was ready, and the transport made no
    /// new readable progress.
    InputPending,
    /// Encoded output remains queued after the bounded write attempt.
    OutputPending,
    /// The explicit action budget was exhausted.
    ActionBudgetExhausted,
    /// Exactly one proactive publication state commit completed and the adapter deliberately yielded
    /// before attempting another proactive step.
    PublicationProgress,
    /// The peer reached clean EOF after all admitted inbound bytes were consumed.
    PeerEof,
    /// The target session is closed and all currently queued output has been flushed.
    SessionClosed,
}

/// Evidence from one bounded publication-aware service call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationServiceReport {
    /// Bytes read from the transport during this call.
    pub read_bytes: usize,
    /// Bytes written to the transport during this call.
    pub written_bytes: usize,
    /// Inbound semantic actions plus proactive publication commits completed during this call.
    pub committed_actions: usize,
    /// Outbound packet frames admitted by those committed actions/publication steps.
    pub outbound_frames: usize,
    /// Ingress bytes still buffered after the call.
    pub buffered_ingress: usize,
    /// Egress bytes still queued after the call.
    pub queued_egress: usize,
    /// Boundary that stopped the service call.
    pub stop: PublicationServiceStop,
}

impl<T> PrePlayIo<T>
where
    T: PrePlayPublisher,
{
    /// Services one bounded unit of transport, inbound target work and proactive publication.
    ///
    /// The order preserves the ordinary pre-play adapter's transport law while adding exactly one
    /// publication opportunity at an `Incomplete` boundary:
    ///
    /// 1. make at most one write attempt for already-queued egress;
    /// 2. process already-buffered inbound actions under the remaining action budget;
    /// 3. if inbound processing is incomplete, try **one** proactive publication step;
    /// 4. only when publication is idle, make at most one transport read;
    /// 5. process newly completed inbound actions under the remaining budget;
    /// 6. if that processing is incomplete, try **one** proactive publication step; and
    /// 7. make at most one final write attempt for newly queued egress.
    ///
    /// A publication commit consumes one action-budget unit even when the primitive reports
    /// [`PublicationStep::Complete`] and queues no bytes. Once any proactive step commits, this
    /// method does not attempt another proactive step in the same call. Consequently zero-byte stage
    /// transitions cannot spin and a large publication cannot monopolize one service turn.
    ///
    /// # Errors
    ///
    /// Propagates the same fail-closed transport/connection/target/accounting failures as the
    /// ordinary adapter. A failed publication proposal or bounded egress admission does not commit
    /// target-local publication progression.
    pub fn service_once_with_publication<RW>(
        &mut self,
        transport: &mut RW,
        context: &T::Context,
        budget: ActionBudget,
    ) -> Result<PublicationServiceReport, PrePlayIoError<T::Error>>
    where
        RW: Read + Write + ?Sized,
    {
        let mut read_bytes = 0usize;
        let mut written_bytes = 0usize;
        let mut committed_actions = 0usize;
        let mut outbound_frames = 0usize;
        let mut remaining_actions = budget.get();

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                written_bytes = add(written_bytes, written)?;
                if remaining != 0 {
                    return Ok(self.publication_service_report(
                        read_bytes,
                        written_bytes,
                        committed_actions,
                        outbound_frames,
                        PublicationServiceStop::OutputPending,
                    ));
                }
            }
            WriteOutcome::Pending => {
                return Ok(self.publication_service_report(
                    read_bytes,
                    written_bytes,
                    committed_actions,
                    outbound_frames,
                    PublicationServiceStop::OutputPending,
                ));
            }
            WriteOutcome::Empty => {}
        }

        let first = self.process_limit(context, remaining_actions)?;
        committed_actions = add(committed_actions, first.committed_actions)?;
        outbound_frames = add(outbound_frames, first.outbound_frames)?;
        remaining_actions = remaining_actions
            .checked_sub(first.committed_actions)
            .ok_or(PrePlayIoError::AccountingOverflow)?;

        let mut stop = match first.stop {
            ProcessStop::SessionClosed => PublicationServiceStop::SessionClosed,
            ProcessStop::ActionBudgetExhausted => PublicationServiceStop::ActionBudgetExhausted,
            ProcessStop::Incomplete if self.peer_eof => PublicationServiceStop::PeerEof,
            ProcessStop::Incomplete => {
                if let Some(admitted) = self.publish_ready(context)? {
                    committed_actions = add(committed_actions, 1)?;
                    outbound_frames = add(outbound_frames, admitted)?;
                    remaining_actions = remaining_actions
                        .checked_sub(1)
                        .ok_or(PrePlayIoError::AccountingOverflow)?;
                    if remaining_actions == 0 {
                        PublicationServiceStop::ActionBudgetExhausted
                    } else {
                        PublicationServiceStop::PublicationProgress
                    }
                } else {
                    match self.read_once(transport)? {
                        ReadOutcome::Data(read) => {
                            read_bytes = add(read_bytes, read)?;
                            if remaining_actions == 0 {
                                PublicationServiceStop::ActionBudgetExhausted
                            } else {
                                let second = self.process_limit(context, remaining_actions)?;
                                committed_actions =
                                    add(committed_actions, second.committed_actions)?;
                                outbound_frames = add(outbound_frames, second.outbound_frames)?;
                                remaining_actions = remaining_actions
                                    .checked_sub(second.committed_actions)
                                    .ok_or(PrePlayIoError::AccountingOverflow)?;
                                match second.stop {
                                    ProcessStop::SessionClosed => {
                                        PublicationServiceStop::SessionClosed
                                    }
                                    ProcessStop::ActionBudgetExhausted => {
                                        PublicationServiceStop::ActionBudgetExhausted
                                    }
                                    ProcessStop::Incomplete => {
                                        if let Some(admitted) = self.publish_ready(context)? {
                                            committed_actions = add(committed_actions, 1)?;
                                            outbound_frames = add(outbound_frames, admitted)?;
                                            remaining_actions = remaining_actions
                                                .checked_sub(1)
                                                .ok_or(PrePlayIoError::AccountingOverflow)?;
                                            if remaining_actions == 0 {
                                                PublicationServiceStop::ActionBudgetExhausted
                                            } else {
                                                PublicationServiceStop::PublicationProgress
                                            }
                                        } else {
                                            PublicationServiceStop::InputPending
                                        }
                                    }
                                }
                            }
                        }
                        ReadOutcome::Pending => PublicationServiceStop::InputPending,
                        ReadOutcome::Eof => PublicationServiceStop::PeerEof,
                    }
                }
            }
        };

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                written_bytes = add(written_bytes, written)?;
                if remaining != 0 {
                    stop = PublicationServiceStop::OutputPending;
                }
            }
            WriteOutcome::Pending => stop = PublicationServiceStop::OutputPending,
            WriteOutcome::Empty => {}
        }

        if stop == PublicationServiceStop::SessionClosed && self.connection.queued_egress() != 0 {
            stop = PublicationServiceStop::OutputPending;
        }
        if stop == PublicationServiceStop::PeerEof && self.connection.queued_egress() != 0 {
            stop = PublicationServiceStop::OutputPending;
        }

        Ok(self.publication_service_report(
            read_bytes,
            written_bytes,
            committed_actions,
            outbound_frames,
            stop,
        ))
    }

    fn publish_ready(
        &mut self,
        context: &T::Context,
    ) -> Result<Option<usize>, PrePlayIoError<T::Error>> {
        match self.connection.service_publication(context)? {
            PrePlayPublicationProcess::Idle => Ok(None),
            PrePlayPublicationProcess::Progress(PublicationStep::Queued { .. }) => Ok(Some(1)),
            PrePlayPublicationProcess::Progress(PublicationStep::Complete) => Ok(Some(0)),
        }
    }

    fn publication_service_report(
        &self,
        read_bytes: usize,
        written_bytes: usize,
        committed_actions: usize,
        outbound_frames: usize,
        stop: PublicationServiceStop,
    ) -> PublicationServiceReport {
        PublicationServiceReport {
            read_bytes,
            written_bytes,
            committed_actions,
            outbound_frames,
            buffered_ingress: self.connection.buffered_ingress(),
            queued_egress: self.connection.queued_egress(),
            stop,
        }
    }
}
