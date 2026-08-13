import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Key } from 'chessground/types';
import type { Counterfactual } from '../api/types.ts';
import { expandSanLine, type ReplayStep } from '../chess/rules.ts';

/**
 * The counterfactual replay — the reason this tool exists (DESIGN §13).
 *
 * A classified move comes back with a line: how the opponent punishes it, or
 * how the position collapses if the second-best move is played instead. Rather
 * than printing that line as text, we play it on the board one move at a time
 * while the explanation streams in beside it.
 *
 * `start` is only ever called from **Replay line** in the panel. The board is
 * the user's; taking it over unasked is not something a classification earns.
 *
 * The hook owns only the clock. It exposes which position the board should be
 * showing right now; `App` decides whether to render it.
 */

/** Time chessground is given to slide the piece. */
export const REPLAY_MOVE_MS = 420;
/** Total time each ply occupies, so there is a beat between moves to read it. */
const STEP_MS = 820;
/** Pause on the starting position before the first move, so it registers. */
const START_HOLD_MS = 700;
/** How long the final position stays up before the board returns to the game. */
const END_HOLD_MS = 2600;

export interface ReplayState {
  active: boolean;
  steps: ReplayStep[];
  /** -1 = start position; 0..n-1 = after that move; n = line finished. */
  index: number;
  fen: string | null;
  lastMove: [Key, Key] | undefined;
  finished: boolean;
  start: () => void;
  stop: () => void;
}

export function useReplay(counterfactual: Counterfactual | null): ReplayState {
  const steps = useMemo(
    () => (counterfactual ? expandSanLine(counterfactual.start_fen, counterfactual.pv) : []),
    [counterfactual],
  );

  const [active, setActive] = useState(false);
  const [index, setIndex] = useState(-1);

  // A new line always cancels whatever was playing.
  const previous = useRef(counterfactual);
  if (previous.current !== counterfactual) {
    previous.current = counterfactual;
    if (active) setActive(false);
    if (index !== -1) setIndex(-1);
  }

  const start = useCallback(() => {
    if (steps.length === 0) return;
    setIndex(-1);
    setActive(true);
  }, [steps.length]);

  const stop = useCallback(() => {
    setActive(false);
    setIndex(-1);
  }, []);

  useEffect(() => {
    if (!active) return;
    if (steps.length === 0) {
      setActive(false);
      return;
    }
    if (index >= steps.length) {
      const timer = setTimeout(() => {
        setActive(false);
        setIndex(-1);
      }, END_HOLD_MS);
      return () => clearTimeout(timer);
    }
    const timer = setTimeout(
      () => setIndex((current) => current + 1),
      index < 0 ? START_HOLD_MS : STEP_MS,
    );
    return () => clearTimeout(timer);
  }, [active, index, steps]);

  const current = index >= 0 ? steps[Math.min(index, steps.length - 1)] : undefined;
  const fen = !active
    ? null
    : index < 0
      ? (counterfactual?.start_fen ?? null)
      : (current?.toFen ?? null);

  return {
    active,
    steps,
    index,
    fen,
    lastMove: active && current ? [current.from, current.to] : undefined,
    finished: active && index >= steps.length,
    start,
    stop,
  };
}
