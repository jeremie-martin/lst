// lst patch: one checked boundary for XRandR mode timings. Invalid input falls
// back at the caller; zero periods must never reach the timer catch-up loop.
use std::time::Duration;

pub(super) fn mode_refresh_period(dot_clock: u32, htotal: u16, vtotal: u16) -> Option<Duration> {
    if dot_clock == 0 || htotal == 0 || vtotal == 0 {
        return None;
    }
    // Compute the period directly: rounding to millihertz first can produce
    // a zero divisor. The full product fits u64 for XRandR's field widths.
    let micros = u64::from(htotal) * u64::from(vtotal) * 1_000_000 / u64::from(dot_clock);
    (micros > 0).then(|| Duration::from_micros(micros))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_modes_preserve_their_refresh_period() {
        assert_eq!(
            mode_refresh_period(148_500_000, 2200, 1125),
            Some(Duration::from_micros(16_666))
        );
        assert_eq!(
            mode_refresh_period(297_000_000, 2200, 1125),
            Some(Duration::from_micros(8_333))
        );
    }

    #[test]
    fn invalid_and_sub_microsecond_timings_are_rejected() {
        for fields in [
            (0, 2200, 1125),
            (148_500_000, 0, 1125),
            (148_500_000, 2200, 0),
            (u32::MAX, 1, 1),
        ] {
            assert_eq!(mode_refresh_period(fields.0, fields.1, fields.2), None);
        }
    }

    #[test]
    fn extreme_fields_cannot_overflow_or_round_the_divisor_to_zero() {
        assert_eq!(
            mode_refresh_period(1, u16::MAX, u16::MAX),
            Some(Duration::from_micros(4_294_836_225_000_000))
        );
    }
}
