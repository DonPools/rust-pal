//! DOS/Classic's process-wide pseudo-random number sequence.

pub(crate) const DEFAULT_RANDOM_SEED: u32 = 0x4d59_5df4;

/// Match Classic's `lsrand`: the external seed is mixed once before `lrand` advances it.
pub(crate) fn seed(initial: u32) -> u32 {
    initial.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Advance the original 32-bit LCG and return its non-negative 31-bit value.
pub(crate) fn next_u31(state: &mut u32) -> u32 {
    *state = seed(*state);
    (((*state as i32) >> 1).wrapping_add(1_073_741_824)) as u32
}

/// Match Classic's inclusive `RandomLong(from, to)` scaling.
pub(crate) fn random_long(state: &mut u32, from: u32, to: u32) -> u32 {
    if to <= from {
        return from;
    }
    let width = to.saturating_sub(from).saturating_add(1);
    let divisor = (i32::MAX as u32 / width).max(1);
    from.saturating_add(next_u31(state) / divisor)
}

/// Match Classic's inclusive floating-point interpolation.
pub(crate) fn random_float(state: &mut u32, from: f32, to: f32) -> f32 {
    if to <= from {
        return from;
    }
    from + next_u31(state) as f32 / (i32::MAX as f32 / (to - from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_lcg_matches_reference_sequence() {
        let mut state = 1;
        assert_eq!(next_u31(&mut state), 1_581_526_198);
        assert_eq!(state, 1_015_568_748);
        assert_eq!(next_u31(&mut state), 1_866_744_557);
        assert_eq!(state, 1_586_005_467);
    }

    #[test]
    fn external_seed_is_mixed_like_classic_lsrand() {
        let mut state = seed(1);
        assert_eq!(state, 1_015_568_748);
        assert_eq!(next_u31(&mut state), 1_866_744_557);
    }

    #[test]
    fn random_long_is_inclusive_without_advancing_degenerate_ranges() {
        let mut state = 7;
        assert_eq!(random_long(&mut state, 4, 4), 4);
        assert_eq!(state, 7);
        for _ in 0..100 {
            assert!(random_long(&mut state, 1, 2) <= 2);
        }
    }
}
