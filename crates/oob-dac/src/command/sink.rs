//! The only bridge from oob-dac to the oob-mux scheduler.
//!
//! [`PduSink`] is `pub(super)` inside a private module, so no adapter
//! can name it, and [`SchedulerSink`] is the one place a
//! `oob_mux::SchedulerHandle` is held. The daemon (#29) builds oob-mux's
//! `(Scheduler, SchedulerHandle, OutcomeReceiver)` and *moves* the handle
//! into [`CommandBus::new`](super::CommandBus::new); no other oob-dac
//! module receives one. Tests assert this (see the `sole_path` tests).

use oob_mux::{CarouselId, PduId, SchedulerHandle, Submission, SubmitError};

/// What the executor needs from a scheduler.
pub(super) trait PduSink: Send + Sync {
    /// Queues one submission.
    fn submit(&self, s: Submission) -> Result<PduId, SubmitError>;
    /// Withdraws a carousel.
    #[allow(dead_code)] // carousel-backed auto ops arrive with #25
    fn withdraw(&self, id: CarouselId) -> Result<(), SubmitError>;
}

/// The real sink: a scheduler handle, moved in.
pub(super) struct SchedulerSink(pub(super) SchedulerHandle);

impl PduSink for SchedulerSink {
    fn submit(&self, s: Submission) -> Result<PduId, SubmitError> {
        self.0.submit(s)
    }

    fn withdraw(&self, id: CarouselId) -> Result<(), SubmitError> {
        self.0.withdraw(id)
    }
}
