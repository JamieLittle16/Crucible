//! Bounded I/O service path for targets with proactive publication work.
//!
//! The ordinary [`crate::PrePlayIo::service_once`] path remains unchanged for inbound-driven
//! Status/Login targets. This module adds an opt-in service law for [`PrePlayPublisher`] targets:
//! buffered inbound work keeps priority, and when it reaches `Incomplete` the adapter may commit at
//! most one proactive publication step before yielding the service turn.

use std::io::{Read, Write};

use helve_preplay_core::{
    PrePlayConnection, PrePlayPublicationProcess, PrePlayPublisher, PrePlayTarget, PublicationStep,
};

use super::{
    ActionBudget, PrePlayIo, PrePlayIoError, ProcessReport, ProcessStop, ReadOutcome, WriteOutcome,
    add,
};

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

/// Evidence from one bounded publication-aware I/O service call.
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

#[derive(Clone, Copy, Debug)]
struct ServiceTally {
    read_bytes: usize,
    written_bytes: usize,
    committed_actions: usize,
    outbound_frames: usize,
    remaining_actions: usize,
}

impl ServiceTally {
    const fn new(budget: ActionBudget) -> Self {
        Self {
            read_bytes: 0,
            written_bytes: 0,
            committed_actions: 0,
            outbound_frames: 0,
            remaining_actions: budget.get(),
        }
    }

    fn account_process<E>(&mut self, report: ProcessReport) -> Result<(), PrePlayIoError<E>> {
        self.committed_actions = add(self.committed_actions, report.committed_actions)?;
        self.outbound_frames = add(self.outbound_frames, report.outbound_frames)?;
        self.remaining_actions = self
            .remaining_actions
            .checked_sub(report.committed_actions)
            .ok_or(PrePlayIoError::AccountingOverflow)?;
        Ok(())
    }

    fn account_publication<E>(&mut self, admitted_frames: usize) -> Result<(), PrePlayIoError<E>> {
        self.committed_actions = add(self.committed_actions, 1)?;
        self.outbound_frames = add(self.outbound_frames, admitted_frames)?;
        self.remaining_actions = self
            .remaining_actions
            .checked_sub(1)
            .ok_or(PrePlayIoError::AccountingOverflow)?;
        Ok(())
    }

    fn account_read<E>(&mut self, read: usize) -> Result<(), PrePlayIoError<E>> {
        self.read_bytes = add(self.read_bytes, read)?;
        Ok(())
    }

    fn account_write<E>(&mut self, written: usize) -> Result<(), PrePlayIoError<E>> {
        self.written_bytes = add(self.written_bytes, written)?;
        Ok(())
    }

    const fn publication_progress_stop(self) -> PublicationServiceStop {
        if self.remaining_actions == 0 {
            PublicationServiceStop::ActionBudgetExhausted
        } else {
            PublicationServiceStop::PublicationProgress
        }
    }
}

impl<T> PrePlayIo<T>
where
    T: PrePlayTarget,
{
    /// Consumes the adapter while retaining both expensive connection allocations for a new owner.
    ///
    /// The caller receives the complete target-bound connection, the already-allocated retained
    /// read scratch, and the EOF observation bit. This method performs no semantic handoff checks;
    /// callers must establish their target-specific phase boundary and may use
    /// [`PrePlayConnection::try_into_drained_driver`] to require empty userspace queues before
    /// transferring the driver itself.
    #[must_use]
    pub fn into_parts(self) -> (PrePlayConnection<T>, Box<[u8]>, bool) {
        (self.connection, self.read_scratch, self.peer_eof)
    }
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
        let mut tally = ServiceTally::new(budget);

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                tally.account_write(written)?;
                if remaining != 0 {
                    return Ok(self
                        .publication_service_report(tally, PublicationServiceStop::OutputPending));
                }
            }
            WriteOutcome::Pending => {
                return Ok(
                    self.publication_service_report(tally, PublicationServiceStop::OutputPending)
                );
            }
            WriteOutcome::Empty => {}
        }

        let first = self.process_limit(context, tally.remaining_actions)?;
        tally.account_process(first)?;
        let mut stop = match first.stop {
            ProcessStop::SessionClosed => PublicationServiceStop::SessionClosed,
            ProcessStop::ActionBudgetExhausted => PublicationServiceStop::ActionBudgetExhausted,
            ProcessStop::Incomplete if self.peer_eof => PublicationServiceStop::PeerEof,
            ProcessStop::Incomplete => self.service_incomplete(transport, context, &mut tally)?,
        };

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                tally.account_write(written)?;
                if remaining != 0 {
                    stop = PublicationServiceStop::OutputPending;
                }
            }
            WriteOutcome::Pending => stop = PublicationServiceStop::OutputPending,
            WriteOutcome::Empty => {}
        }

        if matches!(
            stop,
            PublicationServiceStop::SessionClosed | PublicationServiceStop::PeerEof
        ) && self.connection.queued_egress() != 0
        {
            stop = PublicationServiceStop::OutputPending;
        }

        Ok(self.publication_service_report(tally, stop))
    }

    fn service_incomplete<RW>(
        &mut self,
        transport: &mut RW,
        context: &T::Context,
        tally: &mut ServiceTally,
    ) -> Result<PublicationServiceStop, PrePlayIoError<T::Error>>
    where
        RW: Read + Write + ?Sized,
    {
        if let Some(admitted) = self.publish_ready(context)? {
            tally.account_publication(admitted)?;
            return Ok(tally.publication_progress_stop());
        }

        match self.read_once(transport)? {
            ReadOutcome::Data(read) => {
                tally.account_read(read)?;
                if tally.remaining_actions == 0 {
                    return Ok(PublicationServiceStop::ActionBudgetExhausted);
                }

                let second = self.process_limit(context, tally.remaining_actions)?;
                tally.account_process(second)?;
                match second.stop {
                    ProcessStop::SessionClosed => Ok(PublicationServiceStop::SessionClosed),
                    ProcessStop::ActionBudgetExhausted => {
                        Ok(PublicationServiceStop::ActionBudgetExhausted)
                    }
                    ProcessStop::Incomplete => {
                        if let Some(admitted) = self.publish_ready(context)? {
                            tally.account_publication(admitted)?;
                            Ok(tally.publication_progress_stop())
                        } else {
                            Ok(PublicationServiceStop::InputPending)
                        }
                    }
                }
            }
            ReadOutcome::Pending => Ok(PublicationServiceStop::InputPending),
            ReadOutcome::Eof => Ok(PublicationServiceStop::PeerEof),
        }
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
        tally: ServiceTally,
        stop: PublicationServiceStop,
    ) -> PublicationServiceReport {
        PublicationServiceReport {
            read_bytes: tally.read_bytes,
            written_bytes: tally.written_bytes,
            committed_actions: tally.committed_actions,
            outbound_frames: tally.outbound_frames,
            buffered_ingress: self.connection.buffered_ingress(),
            queued_egress: self.connection.queued_egress(),
            stop,
        }
    }
}
