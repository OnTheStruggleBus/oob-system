//! Campaign pacing: spacing exists **only** as per-PDU release times.
//!
//! r6 F6-11: campaign pacing lives here and nowhere else. This module turns
//! a campaign (`n` terminals, `delay_between`, an optional Repeat-Every)
//! into `release_at` instants, one per PDU, and nothing else. `oob-mux`
//! never sees a campaign; its C1 class only honours the times it is given
//! (#16) and never infers spacing.
//!
//! Planning is a pure function of the clock reading `t0`, so it is tested
//! without a scheduler: for terminal *i*, `release_at = t0 + i × delay`.

use std::time::Duration;

use oob_mux::MonoTime;

use super::CommandError;

/// The minimum inter-terminal delay: 250 ms. `documented` from the DAC
/// manual (Define Operations, Plan §2); a code constant (ADR-003 Layer 3),
/// not a setting.
pub const MIN_INTER_TERMINAL_DELAY: Duration = Duration::from_millis(250);

/// Rejects a delay below [`MIN_INTER_TERMINAL_DELAY`].
///
/// # Errors
///
/// [`CommandError::DelayBelowMinimum`].
pub fn validate_delay(delay: Duration) -> Result<(), CommandError> {
    if delay < MIN_INTER_TERMINAL_DELAY {
        return Err(CommandError::DelayBelowMinimum {
            got: delay,
            min: MIN_INTER_TERMINAL_DELAY,
        });
    }
    Ok(())
}

/// Rejects a Repeat-Every period shorter than one campaign's span
/// (`terminals × delay`). A shorter period would let repetition *k+1* start
/// before repetition *k* finished, putting PDUs for different terminals
/// closer than the minimum delay. With `period ≥ n × delay` the gap between
/// the last PDU of one repetition and the first of the next is at least
/// `delay`.
///
/// # Errors
///
/// [`CommandError::RepeatTooShort`].
pub fn validate_repeat(
    repeat_every: Option<Duration>,
    delay: Duration,
    terminals: usize,
) -> Result<(), CommandError> {
    let Some(period) = repeat_every else {
        return Ok(());
    };
    let n = u32::try_from(terminals).unwrap_or(u32::MAX);
    let min = delay.saturating_mul(n);
    if period < min {
        return Err(CommandError::RepeatTooShort { got: period, min });
    }
    Ok(())
}

/// The release time of each of `terminals` PDUs: `t0 + i × delay`.
/// A single terminal releases at `t0` (now).
pub fn plan_release_times(t0: MonoTime, terminals: usize, delay: Duration) -> Vec<MonoTime> {
    (0..terminals)
        .map(|i| {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            t0.saturating_add(delay.saturating_mul(i))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn delay_below_250_ms_is_rejected_at_250_is_accepted() {
        assert!(validate_delay(ms(250)).is_ok());
        assert!(validate_delay(ms(251)).is_ok());
        assert!(matches!(
            validate_delay(ms(249)),
            Err(CommandError::DelayBelowMinimum { got, min })
                if got == ms(249) && min == ms(250)
        ));
        assert!(validate_delay(Duration::ZERO).is_err());
    }

    #[test]
    fn release_times_are_exactly_delay_apart() {
        let t0 = MonoTime::from_origin(ms(1_000));
        let times = plan_release_times(t0, 50, ms(250));
        assert_eq!(times.len(), 50);
        assert_eq!(times[0], t0, "the first PDU releases now");
        for w in times.windows(2) {
            assert_eq!(w[1].saturating_since(w[0]), ms(250));
        }
        assert_eq!(times[49].saturating_since(t0), ms(49 * 250));
    }

    #[test]
    fn a_single_terminal_releases_now() {
        let t0 = MonoTime::from_origin(ms(7));
        assert_eq!(plan_release_times(t0, 1, ms(900)), vec![t0]);
        assert!(plan_release_times(t0, 0, ms(900)).is_empty());
    }

    #[test]
    fn repeat_must_cover_one_campaign() {
        assert!(validate_repeat(None, ms(250), 50).is_ok());
        assert!(validate_repeat(Some(ms(12_500)), ms(250), 50).is_ok());
        assert!(matches!(
            validate_repeat(Some(ms(12_499)), ms(250), 50),
            Err(CommandError::RepeatTooShort { .. })
        ));
        // The invariant behind the rule: back-to-back repetitions keep the gap.
        let t0 = MonoTime::default();
        let a = plan_release_times(t0, 50, ms(250));
        let b = plan_release_times(t0.saturating_add(ms(12_500)), 50, ms(250));
        assert!(b[0].saturating_since(a[49]) >= ms(250));
    }
}
