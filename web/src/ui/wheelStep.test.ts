import { describe, expect, it } from 'vitest';
import { NO_WHEEL, WHEEL_STEP_PX, accumulateWheel, type WheelAccumulator } from './wheelStep.ts';

/**
 * Feed a whole gesture through the fold and report what the board would have
 * done — which is the only thing worth asserting, since the accumulator itself
 * is an implementation detail of getting there.
 */
function gesture(deltas: readonly number[], deltaMode = 0): {
  steps: number[];
  state: WheelAccumulator;
} {
  let state = NO_WHEEL;
  const steps: number[] = [];
  for (const delta of deltas) {
    const result = accumulateWheel(state, delta, deltaMode);
    state = result.next;
    if (result.step !== 0) steps.push(result.step);
  }
  return { steps, state };
}

/** One flick: 40 events of 15px, which is a plausible trackpad burst. */
const TRACKPAD_FLICK = Array.from({ length: 40 }, () => 15);

describe('a mouse wheel', () => {
  it('steps forward once per detent', () => {
    // The number this is tuned to: Chrome reports 100px for one detent, and
    // one detent has to be exactly one ply or the gesture is useless.
    expect(gesture([100]).steps).toEqual([1]);
    expect(gesture([100, 100, 100]).steps).toEqual([1, 1, 1]);
  });

  it('steps back when the wheel goes up', () => {
    expect(gesture([-100]).steps).toEqual([-1]);
  });

  it('does not bank the overshoot of a heavier detent', () => {
    // 120px detents (Windows, and some mice on macOS) must not accumulate a
    // free extra move every five notches.
    expect(gesture([120, 120, 120, 120, 120]).steps).toEqual([1, 1, 1, 1, 1]);
  });
});

describe('a trackpad', () => {
  it('spends many small events on one move', () => {
    const almost = Array.from({ length: 6 }, () => 15); // 90px, one short
    expect(gesture(almost).steps).toEqual([]);
    expect(gesture([...almost, 15]).steps).toEqual([1]);
  });

  it('turns a flick into a few moves rather than the whole game', () => {
    // 600px of scroll. The point of the test is the *order of magnitude*: this
    // must land in "a handful", not in "the rest of the game".
    const { steps } = gesture(TRACKPAD_FLICK);
    expect(steps.length).toBeGreaterThan(1);
    expect(steps.length).toBeLessThanOrEqual(6);
    expect(steps.every((step) => step === 1)).toBe(true);
  });

  it('caps one event at one move however large it is', () => {
    // A momentum event, or a page-scroll device, must not fire eight plies.
    expect(gesture([5000]).steps).toEqual([1]);
    expect(gesture([-5000]).steps).toEqual([-1]);
  });
});

describe('changing direction', () => {
  it('does not carry stale accumulation across a reversal', () => {
    // 90 forward then 90 back is one change of mind. Adding them would leave 0
    // and lose the gesture; subtracting them the other way round would fire a
    // *backward* step off scroll that was mostly forward.
    const { steps, state } = gesture([90, -90]);
    expect(steps).toEqual([]);
    expect(state.pixels).toBe(-90);
  });

  it('needs a fresh threshold in the new direction', () => {
    expect(gesture([90, -90, -10]).steps).toEqual([-1]);
  });
});

describe('non-pixel delta modes', () => {
  it('converts lines, so Firefox is not a hundred detents behind', () => {
    // deltaMode 1 reports *lines*: raw deltaY of 3 would never reach 100.
    expect(gesture([3], 1).steps).toEqual([]);
    expect(gesture([3, 3, 3], 1).steps).toEqual([1]);
  });

  it('converts pages', () => {
    expect(gesture([1], 2).steps).toEqual([1]);
  });
});

describe('degenerate events', () => {
  it('ignores a zero delta without disturbing what is banked', () => {
    const banked = accumulateWheel(NO_WHEEL, 50);
    const after = accumulateWheel(banked.next, 0);
    expect(after.step).toBe(0);
    expect(after.next.pixels).toBe(50);
  });

  it('ignores a non-finite delta', () => {
    expect(accumulateWheel(NO_WHEEL, Number.NaN).step).toBe(0);
    expect(accumulateWheel(NO_WHEEL, Number.POSITIVE_INFINITY).next.pixels).toBe(0);
  });

  it('resets rather than keeping a remainder once it fires', () => {
    expect(accumulateWheel({ pixels: WHEEL_STEP_PX - 1 }, 500).next).toBe(NO_WHEEL);
  });
});
