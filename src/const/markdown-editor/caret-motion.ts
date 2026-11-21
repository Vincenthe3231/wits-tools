/** Tunes the shared caret head and slower trailing spring. */

// Higher frequencies settle faster. Cap the visible smear across large jumps.
export const CARET_HEAD_FREQUENCY = 36;
export const CARET_TAIL_FREQUENCY = 13;
export const CARET_TRAIL_MAX_LENGTH = 180;
