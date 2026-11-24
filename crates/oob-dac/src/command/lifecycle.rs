//! The operation status lifecycle.
//!
//! # Status labels
//!
//! The status **names and definitions** are `documented`: *Basic DAC 6000
//! (V3.1) Operation, Rev 1.2*, p. 11-4 ("Status indications are as
//! follows"), recorded in the DAC Reference §10. They describe an event on
//! the **DAC ↔ RADD** exchange, never a terminal's reply. Oob plays the DAC
//! and RADD roles in one daemon (DAC Reference §1): the "RADD" is the path
//! from the executor through `oob-mux`'s scheduler to the PHY seam.
//!
//! The **transition order** ([`TRANSITIONS`]) and the **Oob meaning** of
//! each status ([`OpStatus::oob_meaning`]) are `hypothesized`: p. 11-4 is a
//! list, not a transition table, and the mapping to pipeline events is
//! design. They live in this one data table on purpose, so a page-confirmed
//! correction changes the table and its tests, not the pipeline
//! (oob-architect design note on #22, follow-up 3).
//!
//! # Overlays
//!
//! *Retrieving Status*, *Unknown* and *Device Offline* are **overlays**:
//! they sit beside the main status ([`OpState::overlay`]) and return to it,
//! so the main path stays a clean state machine. An overlay is never final
//! and never stored on a final operation.
//!
//! # One-way terminals
//!
//! The lifecycle has no dependency on a terminal reply, so an operation on a
//! One-Way terminal ends **Completed** like any other (an inference from the
//! documented definitions, not a quote; oob-expert note on #22). Terminal-side
//! results belong to `terminal_status` (#21).

use super::CommandError;

/// The main operation status (p. 11-4 names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpStatus {
    /// Not yet processed because `start_time` (or the Repeat-Every slot)
    /// has not been reached.
    New,
    /// Read and initiated: planning and encoding have started.
    RequestSent,
    /// The operation manager could not send the event on. Final.
    RequestSentFailed,
    /// Initiated, no acknowledgement. Final.
    Timeout,
    /// Accepted by the RADD: every PDU of the current repetition is
    /// accepted by the scheduler (accepted, not yet sent).
    Downloaded,
    /// Currently executing: emission has begun.
    Active,
    /// Waiting while a Category Epoch operation is active.
    Delayed,
    /// Successfully completed. Final.
    Completed,
    /// An active event past its end time, closed out as completed. Final.
    MarkedCompleted,
    /// The generic final failure: "The operation failed." Final.
    Failed,
}

impl OpStatus {
    /// Every status.
    pub const ALL: [Self; 10] = [
        Self::New,
        Self::RequestSent,
        Self::RequestSentFailed,
        Self::Timeout,
        Self::Downloaded,
        Self::Active,
        Self::Delayed,
        Self::Completed,
        Self::MarkedCompleted,
        Self::Failed,
    ];

    /// The manual's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::RequestSent => "Request Sent",
            Self::RequestSentFailed => "Request Sent Failed",
            Self::Timeout => "Timeout",
            Self::Downloaded => "Downloaded",
            Self::Active => "Active",
            Self::Delayed => "Delayed",
            Self::Completed => "Completed",
            Self::MarkedCompleted => "Marked Completed",
            Self::Failed => "Failed",
        }
    }

    /// Whether no transition leaves this status.
    pub fn is_final(self) -> bool {
        TRANSITIONS
            .iter()
            .find(|(from, _)| *from == self)
            .is_some_and(|(_, to)| to.is_empty())
    }

    /// What the status means in Oob's pipeline (`hypothesized`).
    pub fn oob_meaning(self) -> &'static str {
        match self {
            Self::New => "accepted; start time or Repeat-Every slot not reached",
            Self::RequestSent => {
                "planning and encoding started; PDUs being handed to the scheduler"
            }
            Self::RequestSentFailed => {
                "encoder error or panic, or the scheduler refused for a reason other than capacity"
            }
            Self::Timeout => "the scheduler did not accept the PDUs within the bound (queue full)",
            Self::Downloaded => "every PDU of the current repetition is accepted by the scheduler",
            Self::Active => "the first PDU has been emitted; emission in progress",
            Self::Delayed => "waiting while a Category Epoch operation is active",
            Self::Completed => "the last PDU of the final repetition was emitted",
            Self::MarkedCompleted => "an active operation passed its end time",
            Self::Failed => "a PDU expired or was dropped, or the operation was cancelled",
        }
    }
}

/// The transition table, `hypothesized` (see the module docs). Each row is
/// `(from, legal targets)`; a status with no targets is final. Every pair
/// not listed is illegal ([`CommandError::IllegalTransition`]).
///
/// Main path: New → Request Sent → (Request Sent Failed | Timeout |
/// Downloaded) → Active → (Completed | Failed | Marked Completed).
/// Repeat-Every: Active → Downloaded → Active per repetition. Delayed: a
/// not-yet-started operation waits and returns to where it was; an Active
/// one continues (the manual does not say, so this is the reconstruction).
/// Failed is the catch-all: every non-final status may fail (cancellation,
/// or a failure with no more specific status); the executor picks the more
/// specific status where one applies.
pub const TRANSITIONS: &[(OpStatus, &[OpStatus])] = &[
    (
        OpStatus::New,
        &[OpStatus::RequestSent, OpStatus::Delayed, OpStatus::Failed],
    ),
    (
        OpStatus::RequestSent,
        &[
            OpStatus::RequestSentFailed,
            OpStatus::Timeout,
            OpStatus::Downloaded,
            OpStatus::Delayed,
            OpStatus::Failed,
        ],
    ),
    (OpStatus::RequestSentFailed, &[]),
    (OpStatus::Timeout, &[]),
    (
        OpStatus::Downloaded,
        &[OpStatus::Active, OpStatus::Delayed, OpStatus::Failed],
    ),
    (
        OpStatus::Active,
        &[
            OpStatus::Completed,
            OpStatus::Failed,
            OpStatus::MarkedCompleted,
            OpStatus::Downloaded,
        ],
    ),
    (
        OpStatus::Delayed,
        &[
            OpStatus::New,
            OpStatus::RequestSent,
            OpStatus::Downloaded,
            OpStatus::Failed,
        ],
    ),
    (OpStatus::Completed, &[]),
    (OpStatus::MarkedCompleted, &[]),
    (OpStatus::Failed, &[]),
];

/// Whether `from → to` is in [`TRANSITIONS`].
pub fn is_legal(from: OpStatus, to: OpStatus) -> bool {
    TRANSITIONS
        .iter()
        .find(|(f, _)| *f == from)
        .is_some_and(|(_, targets)| targets.contains(&to))
}

/// The overlay states (p. 11-4), shown beside the main status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// Transient: a Retrieve Operation Status request is reconciling the
    /// ledger with the scheduler's outcomes.
    RetrievingStatus,
    /// The ledger records a non-final operation that the scheduler holds no
    /// PDUs of and has emitted none of (for example after a restart, before
    /// resume re-plans it).
    Unknown,
    /// The PHY seam has no connected puller, so nothing can be emitted.
    DeviceOffline,
}

/// One operation's status and overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpState {
    status: OpStatus,
    overlay: Option<Overlay>,
    delayed_from: Option<OpStatus>,
}

impl Default for OpState {
    fn default() -> Self {
        Self::new()
    }
}

impl OpState {
    /// A new operation, status [`OpStatus::New`], no overlay.
    pub fn new() -> Self {
        Self {
            status: OpStatus::New,
            overlay: None,
            delayed_from: None,
        }
    }

    /// The main status.
    pub fn status(&self) -> OpStatus {
        self.status
    }

    /// The overlay, if any.
    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    /// Moves to `to` if the table allows it. Entering a final status
    /// clears any overlay.
    ///
    /// # Errors
    ///
    /// [`CommandError::IllegalTransition`].
    pub fn transition(&mut self, to: OpStatus) -> Result<(), CommandError> {
        if !is_legal(self.status, to) {
            return Err(CommandError::IllegalTransition {
                from: self.status,
                to,
            });
        }
        if to == OpStatus::Delayed {
            self.delayed_from = Some(self.status);
        }
        self.status = to;
        if to.is_final() {
            self.overlay = None;
        }
        Ok(())
    }

    /// Enters [`OpStatus::Delayed`] from a not-yet-started status.
    ///
    /// # Errors
    ///
    /// [`CommandError::IllegalTransition`] from any status the table does
    /// not let wait (including Active, which continues).
    pub fn delay(&mut self) -> Result<(), CommandError> {
        self.transition(OpStatus::Delayed)
    }

    /// Leaves [`OpStatus::Delayed`] for the status it waited in.
    ///
    /// # Errors
    ///
    /// [`CommandError::IllegalTransition`] when not Delayed.
    pub fn resume(&mut self) -> Result<(), CommandError> {
        let back = match (self.status, self.delayed_from) {
            (OpStatus::Delayed, Some(b)) => b,
            _ => {
                return Err(CommandError::IllegalTransition {
                    from: self.status,
                    to: OpStatus::New,
                });
            }
        };
        self.transition(back)?;
        self.delayed_from = None;
        Ok(())
    }

    /// Sets an overlay. Allowed on a non-final operation that has left
    /// [`OpStatus::New`].
    ///
    /// # Errors
    ///
    /// [`CommandError::IllegalOverlay`] on a final or New operation.
    pub fn set_overlay(&mut self, o: Overlay) -> Result<(), CommandError> {
        if self.status.is_final() || self.status == OpStatus::New {
            return Err(CommandError::IllegalOverlay {
                status: self.status,
            });
        }
        self.overlay = Some(o);
        Ok(())
    }

    /// Clears the overlay (idempotent).
    pub fn clear_overlay(&mut self) {
        self.overlay = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pair_is_checked_against_the_table() {
        // 10 x 10: the exhaustive legal/illegal table test.
        let mut legal = 0;
        for from in OpStatus::ALL {
            for to in OpStatus::ALL {
                let mut s = OpState {
                    status: from,
                    overlay: None,
                    delayed_from: Some(OpStatus::New),
                };
                let listed = TRANSITIONS
                    .iter()
                    .any(|(f, ts)| *f == from && ts.contains(&to));
                match s.transition(to) {
                    Ok(()) => {
                        assert!(listed, "{from:?} -> {to:?} accepted but not in table");
                        assert_eq!(s.status(), to);
                        legal += 1;
                    }
                    Err(CommandError::IllegalTransition { from: f, to: t }) => {
                        assert!(!listed, "{from:?} -> {to:?} in table but refused");
                        assert_eq!((f, t), (from, to));
                        assert_eq!(s.status(), from, "a refused transition changes nothing");
                    }
                    Err(e) => panic!("unexpected error {e:?}"),
                }
            }
        }
        assert_eq!(
            legal,
            TRANSITIONS.iter().map(|(_, t)| t.len()).sum::<usize>()
        );
    }

    #[test]
    fn the_table_has_one_row_per_status() {
        for s in OpStatus::ALL {
            assert_eq!(
                TRANSITIONS.iter().filter(|(f, _)| *f == s).count(),
                1,
                "{s:?}"
            );
        }
    }

    #[test]
    fn named_legal_transitions() {
        use OpStatus::*;
        for (a, b) in [
            (New, RequestSent),
            (RequestSent, Downloaded),
            (RequestSent, RequestSentFailed),
            (RequestSent, Timeout),
            (Downloaded, Active),
            (Active, Completed),
            (Active, MarkedCompleted),
            (Active, Failed),
            (Active, Downloaded),
            (Downloaded, Failed),
        ] {
            assert!(is_legal(a, b), "{a:?} -> {b:?}");
        }
    }

    #[test]
    fn named_illegal_transitions() {
        use OpStatus::*;
        for (a, b) in [
            (New, Completed),
            (New, Active),
            (RequestSent, Active),
            (RequestSent, Completed),
            (Downloaded, Completed),
            (Downloaded, RequestSent),
            (Active, RequestSent),
            (Active, Delayed),
            (Completed, Active),
            (Completed, Failed),
            (Failed, Completed),
            (Timeout, Downloaded),
            (RequestSentFailed, RequestSent),
            (MarkedCompleted, Completed),
        ] {
            assert!(!is_legal(a, b), "{a:?} -> {b:?}");
        }
    }

    #[test]
    fn final_statuses_are_exactly_the_five() {
        use OpStatus::*;
        let finals: Vec<_> = OpStatus::ALL.into_iter().filter(|s| s.is_final()).collect();
        assert_eq!(
            finals,
            vec![
                RequestSentFailed,
                Timeout,
                Completed,
                MarkedCompleted,
                Failed
            ]
        );
    }

    #[test]
    fn labels_are_the_manual_names() {
        let labels: Vec<_> = OpStatus::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            [
                "New",
                "Request Sent",
                "Request Sent Failed",
                "Timeout",
                "Downloaded",
                "Active",
                "Delayed",
                "Completed",
                "Marked Completed",
                "Failed"
            ]
        );
    }

    #[test]
    fn delayed_is_entered_and_exited_around_a_wait() {
        for waiting in [OpStatus::New, OpStatus::RequestSent, OpStatus::Downloaded] {
            let mut s = OpState {
                status: waiting,
                overlay: None,
                delayed_from: None,
            };
            s.delay().unwrap();
            assert_eq!(s.status(), OpStatus::Delayed);
            s.resume().unwrap();
            assert_eq!(s.status(), waiting, "returns to where it waited");
        }
    }

    #[test]
    fn an_active_operation_continues_and_does_not_delay() {
        let mut s = OpState {
            status: OpStatus::Active,
            overlay: None,
            delayed_from: None,
        };
        assert!(matches!(
            s.delay(),
            Err(CommandError::IllegalTransition { .. })
        ));
        assert!(s.resume().is_err(), "resume needs Delayed");
    }

    #[test]
    fn overlay_set_and_clear() {
        let mut s = OpState::new();
        assert!(
            s.set_overlay(Overlay::DeviceOffline).is_err(),
            "New has none"
        );
        s.transition(OpStatus::RequestSent).unwrap();
        for o in [
            Overlay::RetrievingStatus,
            Overlay::Unknown,
            Overlay::DeviceOffline,
        ] {
            s.set_overlay(o).unwrap();
            assert_eq!(s.overlay(), Some(o));
            assert_eq!(s.status(), OpStatus::RequestSent, "main status untouched");
            s.clear_overlay();
            assert_eq!(s.overlay(), None);
        }
    }

    #[test]
    fn an_overlay_is_never_stored_on_a_final_operation() {
        let mut s = OpState::new();
        s.transition(OpStatus::RequestSent).unwrap();
        s.set_overlay(Overlay::Unknown).unwrap();
        s.transition(OpStatus::Failed).unwrap();
        assert_eq!(s.overlay(), None, "entering a final status clears it");
        assert!(matches!(
            s.set_overlay(Overlay::RetrievingStatus),
            Err(CommandError::IllegalOverlay {
                status: OpStatus::Failed
            })
        ));
    }
}
