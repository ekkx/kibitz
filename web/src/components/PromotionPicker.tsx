import { useEffect, useRef } from 'react';
import type { Key } from 'chessground/types';
import type { Color } from '../chess/rules.ts';
import { PROMOTION_PIECES, type PromotionPiece } from '../chess/rules.ts';
import type { UiMessageKey } from '../i18n/en.ts';
import { t } from '../i18n/index.ts';

export interface PromotionPickerProps {
  /** The square the pawn is arriving on, e.g. `b8`. */
  dest: Key;
  /** The side promoting — whose pieces are offered. */
  color: Color;
  /** Which way up the board is drawn, so the stack hangs off the right edge. */
  orientation: Color;
  onChoose: (piece: PromotionPiece) => void;
  onCancel: () => void;
}

/** chessground's own class names for the four pieces, and what to call them. */
const PIECES: Record<PromotionPiece, { role: string; label: UiMessageKey }> = {
  q: { role: 'queen', label: 'piece.queen' },
  r: { role: 'rook', label: 'piece.rook' },
  b: { role: 'bishop', label: 'piece.bishop' },
  n: { role: 'knight', label: 'piece.knight' },
};

/**
 * The four pieces a pawn may become, stacked on the promotion square.
 *
 * chessground reports a move through `movable.events.after` — *after* it has
 * already slid the piece — so by the time this appears the pawn is sitting on
 * the last rank underneath the queen tile. That is the right picture: the move
 * has been made on the board and the only open question is what the pawn turned
 * into. Nothing is committed to the session until one of these is clicked, and
 * cancelling puts the board back (`App` re-syncs chessground to the position it
 * still believes in).
 *
 * The shape is the one every board tool uses, and it is not decoration: the four
 * pieces occupy the destination *file*, largest first, starting on the
 * destination square itself. That means the piece you almost always want is
 * already under the pointer that just finished the drag, the other three are one
 * short movement away along a straight line, and the column never leaves the
 * board — the promotion rank is by definition an edge, so the stack can only run
 * inwards, whichever colour is promoting and whichever way round the board is.
 *
 * The tiles are hand-built buttons rather than the design system's, for the same
 * reason the moves in `MoveTree` are: their size is a *fraction of the board*
 * (12.5% square, so the pieces line up with the squares behind them) and every
 * button in the system is a fixed-height box. What they borrow is the system's
 * surface, hover and focus-ring colours.
 *
 * The piece images are chessground's own, addressed the way its stylesheet
 * addresses them — `.cg-wrap piece.queen.white` — which is why there is a
 * `cg-wrap` wrapper here and `<piece>` elements inside it. It is the one way to
 * reuse those embedded SVGs without copying a second set of assets into the
 * app, where they could drift out of step with the board.
 */
export function PromotionPicker({
  dest,
  color,
  orientation,
  onChoose,
  onCancel,
}: PromotionPickerProps): React.JSX.Element | null {
  const containerRef = useRef<HTMLDivElement>(null);

  /**
   * Escape cancels, from anywhere — the pointer may be nowhere near the board,
   * and a picker that can only be dismissed by clicking exactly the right
   * emptiness is a trap. Capturing, because `App`'s own key handler also
   * listens on the window and this is a modal moment: nothing else on the page
   * should act on a key while a move is half-made.
   */
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onCancel]);

  const square = squareOnScreen(dest, orientation);
  if (!square) return null;

  // The promotion rank is an edge of the board, so the stack runs away from it:
  // down from the top row, up from the bottom one.
  const step = square.row === 0 ? 1 : -1;

  /** Up and down walk the stack; the buttons are in the DOM in stack order. */
  const onArrow = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
    const buttons = Array.from(containerRef.current?.querySelectorAll('button') ?? []);
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    // Down the *screen* is down the stack only when the stack hangs downwards.
    const delta = (event.key === 'ArrowDown' ? 1 : -1) * step;
    const next = buttons[index + delta];
    if (!next) return;
    event.preventDefault();
    event.stopPropagation();
    next.focus();
  };

  return (
    /*
      The backdrop is the cancel target and the guard in one: it covers the board
      exactly, so a click anywhere off the four tiles cancels, and no drag can
      start underneath while the question is open. chessground's own wheel
      listener is on its host element, which this is not inside, so scrolling
      over the board is inert here too.
    */
    <div
      className="absolute inset-0 z-20 bg-background/70"
      onClick={onCancel}
      onKeyDown={onArrow}
      role="dialog"
      aria-modal="true"
      aria-label={t('promotion.choose')}
      ref={containerRef}
    >
      <div className="cg-wrap relative size-full">
        {PROMOTION_PIECES.map((piece, index) => {
          const { role, label } = PIECES[piece];
          return (
            <button
              key={piece}
              type="button"
              className="absolute flex items-center justify-center rounded-2xl border bg-card shadow-md transition-colors outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/40"
              style={{
                left: `${square.file * 12.5}%`,
                top: `${(square.row + index * step) * 12.5}%`,
                width: '12.5%',
                height: '12.5%',
              }}
              // The queen is focused on open, so the whole interaction is Enter
              // for the usual answer and one arrow key for any other.
              autoFocus={piece === 'q'}
              onClick={(event) => {
                event.stopPropagation();
                onChoose(piece);
              }}
              title={t(label)}
              aria-label={t(label)}
            >
              {/*
                `piece` is chessground's custom element, and the class pair is
                what carries the image. React needs to be told it is a tag name
                rather than a component, which is all this cast is.
              */}
              <PieceTag
                className={`${role} ${color}`}
                style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }}
              />
            </button>
          );
        })}
      </div>
    </div>
  );
}

const PieceTag = 'piece' as unknown as React.ElementType<{
  className: string;
  style: React.CSSProperties;
}>;

/**
 * Where a square is drawn, as `file` and `row` counted from the top-left of the
 * board *as displayed* — which is the only frame the picker can be positioned
 * in, and the reason flipping the board needs no other code.
 */
export function squareOnScreen(
  square: Key,
  orientation: Color,
): { file: number; row: number } | null {
  const file = square.charCodeAt(0) - 'a'.charCodeAt(0);
  const rank = Number(square[1]);
  if (file < 0 || file > 7 || !Number.isInteger(rank) || rank < 1 || rank > 8) return null;
  return orientation === 'white'
    ? { file, row: 8 - rank }
    : { file: 7 - file, row: rank - 1 };
}
