/**
 * Turning a stream of wheel events into moves.
 *
 * Scrolling over the board walks the game, one ply per notch — the gesture
 * every board tool has, and the fastest way to read a game. The hard part is
 * that "one notch" is not a thing the platform reports.
 *
 * A mouse wheel emits one event per detent, with a large `deltaY` (~100px in
 * Chrome). A trackpad emits a *stream*: one flick is dozens of events carrying
 * a few pixels each, with a momentum tail that keeps firing after the fingers
 * have left the glass. Stepping once per event would make a mouse feel right
 * and send a trackpad through twenty moves at a touch; stepping once per fixed
 * number of pixels alone would fix the trackpad and make a fast mouse scroll —
 * five detents, 500 pixels — jump five moves, which is what it asked for, but
 * a fast *flick* would still cross the threshold ten times over.
 *
 * So there are two rules, and both are needed:
 *
 *   1. **A step costs `WHEEL_STEP_PX` of accumulated scroll.** This is what
 *      makes a trackpad feel like scrolling a list rather than like a slot
 *      machine: a dozen small events buy one move.
 *   2. **At most one step per event, and the accumulator resets when it fires.**
 *      This is what bounds a flick. A single 800px momentum event is one move,
 *      not eight, and the leftover is not banked towards the next one — a
 *      gesture that has already been answered should not keep paying out.
 *
 * Together they make a mouse detent exactly one move (rule 1 is satisfied by
 * one event, rule 2 stops it being more) and a trackpad flick a handful.
 *
 * The state is a plain value and the transition is a pure function, so the
 * behaviour above is testable without a DOM, a wheel, or a trackpad — which
 * matters, because the failure modes here are all "feels wrong" and only two
 * of the three input devices are ever in the room.
 */

/**
 * How much scrolling buys one move.
 *
 * 100 is Chrome's pixel value for one mouse detent, so a detent is one move on
 * the nose. It is deliberately not lower: every pixel below this is a pixel of
 * trackpad flick that turns into an extra ply nobody asked for.
 */
export const WHEEL_STEP_PX = 100;

/**
 * `WheelEvent.deltaY` is only in pixels when `deltaMode` is `DOM_DELTA_PIXEL`.
 * Firefox reports lines, and a page-scroll device reports pages; in those units
 * a detent is `3` or `1` and the threshold above would never be reached. These
 * are the conventional pixel equivalents — they only have to be close, because
 * what they feed is a threshold and not a scroll position.
 */
const LINE_PX = 16;
const PAGE_PX = 800;

/** Scroll banked towards the next move. Opaque; start from `NO_WHEEL`. */
export interface WheelAccumulator {
  readonly pixels: number;
}

export const NO_WHEEL: WheelAccumulator = { pixels: 0 };

export interface WheelStepResult {
  readonly next: WheelAccumulator;
  /**
   * `1` to go forward a ply, `-1` to go back, `0` for "not yet". Never more
   * than one, however large the event — see rule 2 above.
   */
  readonly step: -1 | 0 | 1;
}

/**
 * Fold one wheel event into the accumulator.
 *
 * Down / away from you is forward, which is both the direction lichess uses and
 * the direction "further down the list" already means everywhere else.
 *
 * Reversing direction discards whatever was banked the other way rather than
 * subtracting from it. Scrolling 90px forward and then 90px back is one change
 * of mind, not 180px of travel, and carrying the first 90 would fire a move
 * *backwards* on the second event — the opposite of what the hand just did.
 */
export function accumulateWheel(
  state: WheelAccumulator,
  deltaY: number,
  deltaMode: number = 0,
): WheelStepResult {
  const pixels = deltaY * (deltaMode === 1 ? LINE_PX : deltaMode === 2 ? PAGE_PX : 1);
  if (pixels === 0 || !Number.isFinite(pixels)) return { next: state, step: 0 };

  // `Math.sign(0)` is 0, so an empty accumulator never matches and carries
  // nothing — which is what we want and saves a separate zero case.
  const carried = Math.sign(pixels) === Math.sign(state.pixels) ? state.pixels : 0;
  const total = carried + pixels;

  if (Math.abs(total) < WHEEL_STEP_PX) return { next: { pixels: total }, step: 0 };
  return { next: NO_WHEEL, step: total > 0 ? 1 : -1 };
}
