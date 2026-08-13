import { useEffect, useRef } from 'react';
import { Chessground } from 'chessground';
import type { Api } from 'chessground/api';
import type { Config } from 'chessground/config';
import type { DrawBrushes, DrawShape } from 'chessground/draw';
import type { Key } from 'chessground/types';
import { checkedColor, legalDests, turnColor, type Color } from '../chess/rules.ts';
import { ARROW_BRUSHES } from '../ui/arrows.ts';
import { NO_WHEEL, accumulateWheel } from '../ui/wheelStep.ts';

import 'chessground/assets/chessground.base.css';
import 'chessground/assets/chessground.brown.css';
import 'chessground/assets/chessground.cburnett.css';
// Last, so it wins: our disagreements with the three sheets above.
import './board.css';

export interface BoardProps {
  fen: string;
  orientation: Color;
  lastMove?: [Key, Key] | undefined;
  /** Null disables moving pieces (during a replay, or on a read-only board). */
  onMove?: ((from: Key, to: Key) => void) | null;
  /**
   * Animation length in ms. 0 means "jump", and is the answer for any change of
   * position that is not one movement: jumping to the start or end of the game,
   * clicking a distant node in the move tree, switching variations, or opening a
   * game. Sliding pieces between unrelated positions is noise that hides the
   * position that was asked for.
   *
   * A real duration is passed for the two changes that *are* one movement: a
   * single ply of navigation, in either direction (`hooks/useBoardTransition.ts`
   * decides this, and owns the rule), and each move of a counterfactual replay
   * — that animation is the point of the app.
   */
  animationMs: number;
  shapes?: DrawShape[];
  /**
   * Scrolling over the board walks the game: down is the next ply, up is the
   * previous one.
   *
   * The board reports a *gesture* and says nothing about what it means. It has
   * no idea what a game is — it is handed a FEN — and deciding that a wheel
   * notch means "next node in the tree" is the same decision the ▶ button and
   * the right arrow key make, so it belongs where those are made. Null or
   * absent disables the gesture entirely and lets the page scroll normally,
   * which is what a replay wants: it drives its own position on its own clock
   * and there is nothing here for the wheel to move.
   */
  onWheelStep?: ((direction: 'prev' | 'next') => void) | null;
  /**
   * Bumped to put chessground back in agreement with `fen`.
   *
   * chessground moves the piece *before* it tells anyone (`movable.events.after`
   * fires on an already-moved board), so there is a moment where its DOM shows a
   * move the app has not accepted. Normally the answer arrives as a new `fen`
   * and the update effect below repaints — but when the app *declines* the move
   * nothing in these props changes, the effect has no reason to run, and the
   * board is left showing a position nobody believes in. Today that is a
   * cancelled promotion (`components/PromotionPicker.tsx`); the same applies to
   * any move the app takes back.
   *
   * A counter rather than a method on a ref, because "put the board back" is a
   * fact about the render, not an imperative call: the effect re-runs and
   * re-applies exactly the state these props already describe.
   */
  syncKey?: number;
}

/**
 * chessground wrapper.
 *
 * chessground owns its DOM imperatively, so the component creates the instance
 * once and pushes state through `api.set` afterwards. Only the callback lives
 * in a ref — everything else is derived from props on each update.
 */
export function Board({
  fen,
  orientation,
  lastMove,
  onMove,
  animationMs,
  shapes,
  onWheelStep,
  syncKey,
}: BoardProps): React.JSX.Element {
  const hostRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef<Api | null>(null);
  const onMoveRef = useRef(onMove);
  onMoveRef.current = onMove;
  const onWheelStepRef = useRef(onWheelStep);
  onWheelStepRef.current = onWheelStep;

  useEffect(() => {
    if (!hostRef.current) return;
    const api = Chessground(hostRef.current, {
      fen,
      orientation,
      coordinates: true,
      addPieceZIndex: true,
      highlight: { lastMove: true, check: true },
      animation: { enabled: true, duration: animationMs },
      movable: {
        free: false,
        showDests: true,
        events: {
          after: (from: Key, to: Key) => onMoveRef.current?.(from, to),
        },
      },
      draggable: { showGhost: true },
      premovable: { enabled: false },
      // chessground deep-merges `brushes` over its own palette, so adding the
      // weighted ones leaves `blue` / `paleBlue` — used by the panel preview —
      // in place. The cast is because its type demands the four built-in keys
      // it is about to merge in itself.
      drawable: {
        enabled: true,
        visible: true,
        brushes: ARROW_BRUSHES as unknown as DrawBrushes,
      },
    } satisfies Config);
    apiRef.current = api;
    return () => {
      api.destroy();
      apiRef.current = null;
    };
    // Created once; every later change goes through the update effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * The wheel gesture (`ui/wheelStep.ts` decides how much scrolling is a ply).
   *
   * Attached by hand rather than through React's `onWheel`, because React 19
   * registers its wheel listener as **passive** and a passive listener's
   * `preventDefault` is ignored — the page would scroll out from under the
   * cursor on every notch while the position also changed. `{ passive: false }`
   * is the whole reason this is not four lines of JSX.
   *
   * Registered once, with the handler read from a ref, for the same reason the
   * chessground instance is created once: re-running this effect on every
   * render would tear the listener down and rebuild it mid-gesture, and take
   * the accumulator — which is what makes a trackpad usable — with it. `acc`
   * lives in the closure rather than in a ref because nothing outside this
   * listener has any business reading it.
   */
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let acc = NO_WHEEL;

    const onWheel = (event: WheelEvent) => {
      const handler = onWheelStepRef.current;
      // Disabled: bail out *before* preventDefault, so a page that can scroll
      // still scrolls. Swallowing the event and then doing nothing is the one
      // outcome worse than either. The accumulator is dropped rather than left
      // alone, so scrolling past the board during a replay cannot bank half a
      // move that fires the instant the replay ends.
      if (!handler) {
        acc = NO_WHEEL;
        return;
      }
      event.preventDefault();
      const result = accumulateWheel(acc, event.deltaY, event.deltaMode);
      acc = result.next;
      if (result.step !== 0) handler(result.step > 0 ? 'next' : 'prev');
    };

    host.addEventListener('wheel', onWheel, { passive: false });
    return () => host.removeEventListener('wheel', onWheel);
  }, []);

  useEffect(() => {
    const api = apiRef.current;
    if (!api) return;
    const movable = onMove !== null && onMove !== undefined;
    const check = checkedColor(fen);
    api.set({
      fen,
      orientation,
      turnColor: turnColor(fen),
      lastMove: lastMove ?? undefined,
      check: check ?? undefined,
      animation: { enabled: animationMs > 0, duration: Math.max(animationMs, 1) },
      viewOnly: !movable,
      movable: {
        free: false,
        color: movable ? turnColor(fen) : undefined,
        dests: movable ? legalDests(fen) : new Map<Key, Key[]>(),
        showDests: true,
      },
      drawable: { autoShapes: shapes ?? [] },
    });
  }, [fen, orientation, lastMove, onMove, animationMs, shapes, syncKey]);

  return <div ref={hostRef} style={{ width: '100%', height: '100%' }} />;
}
