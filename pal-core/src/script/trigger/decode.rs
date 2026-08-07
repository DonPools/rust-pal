use crate::role::Direction;

pub(super) fn selected_object(selector: u16, current: u16) -> u16 {
    if selector == 0 || selector == 0xffff {
        current
    } else {
        selector
    }
}

pub(super) fn optional_direction(value: u16) -> Option<Direction> {
    (value != 0xffff)
        .then(|| Direction::from_pal(value))
        .flatten()
}

pub(super) fn delay_80ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let milliseconds = u32::from(periods) * 80;
    milliseconds
        .div_ceil(SCRIPT_TICK_MS)
        .max(1)
        .min(u32::from(u16::MAX)) as u16
}

pub(super) fn delay_60ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let periods = u32::from(periods.max(1));
    (periods * 60)
        .div_ceil(SCRIPT_TICK_MS)
        .min(u32::from(u16::MAX)) as u16
}
