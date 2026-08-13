/**
 * The only place chess rules are computed in the browser.
 *
 * The server owns analysis and the tree; the client needs rules for exactly
 * three things: which squares a piece may legally go to (chessground's
 * `dests`), turning a SAN line into the from/to pairs the replay animates, and
 * knowing whose turn it is / whether the king is in check so the board can
 * highlight it.
 */
import { Chess, type Square } from 'chess.js';
import type { Key } from 'chessground/types';

export type Color = 'white' | 'black';

export function turnColor(fen: string): Color {
  return fen.split(' ')[1] === 'b' ? 'black' : 'white';
}

/** chessground wants `Map<orig, dest[]>` of legal moves. */
export function legalDests(fen: string): Map<Key, Key[]> {
  const dests = new Map<Key, Key[]>();
  const chess = safeChess(fen);
  if (!chess) return dests;
  for (const move of chess.moves({ verbose: true })) {
    const list = dests.get(move.from as Key);
    if (list) list.push(move.to as Key);
    else dests.set(move.from as Key, [move.to as Key]);
  }
  return dests;
}

export function checkedColor(fen: string): Color | undefined {
  const chess = safeChess(fen);
  if (!chess) return undefined;
  return chess.isCheck() ? turnColor(fen) : undefined;
}

export interface MoveResult {
  uci: string;
  san: string;
  fen: string;
}

/**
 * What a pawn may become, best first.
 *
 * The order is the order the picker stacks them in, and it is not arbitrary: a
 * queen is what almost every promotion wants, so it sits nearest the promotion
 * square where the pointer already is, and the underpromotions run away from it
 * in descending value. A knight is last of the four and still one click away,
 * which is the right cost for a move that is rare and never accidental.
 */
export const PROMOTION_PIECES = ['q', 'r', 'b', 'n'] as const;

export type PromotionPiece = (typeof PROMOTION_PIECES)[number];

/**
 * Whether this from/to is a pawn arriving on the last rank, and so a move the
 * board cannot finish on its own.
 *
 * Asked of the *rules* rather than of the squares, because "a pawn on the
 * seventh moving to the eighth" is not the same question: the move has to be
 * legal in the first place (a pinned pawn promotes into nothing), and it may be
 * a capture onto the last rank rather than a push. chess.js already enumerates
 * exactly the legal promotions, so this reads the answer off that instead of
 * re-deriving it from the FEN and getting one of those cases wrong.
 */
export function needsPromotion(fen: string, from: Key, to: Key): boolean {
  const chess = safeChess(fen);
  if (!chess) return false;
  return chess
    .moves({ verbose: true })
    .some((m) => m.from === from && m.to === to && Boolean(m.promotion));
}

/**
 * Play a from/to on `fen`.
 *
 * `promotion` defaults to a queen, so every call site that does not care keeps
 * the behaviour it had when auto-queening was the only behaviour there was —
 * and it is also the right default for a caller that has already decided (the
 * picker passes the piece the user chose). It is ignored by a move that is not
 * a promotion, exactly as the UCI suffix is.
 */
export function playMove(
  fen: string,
  from: Key,
  to: Key,
  promotion: PromotionPiece = 'q',
): MoveResult | null {
  const chess = safeChess(fen);
  if (!chess) return null;
  const legal = chess
    .moves({ verbose: true })
    .find((m) => m.from === from && m.to === to && (!m.promotion || m.promotion === promotion));
  if (!legal) return null;
  const move = chess.move({ from: from as Square, to: to as Square, promotion });
  return {
    uci: `${move.from}${move.to}${move.promotion ?? ''}`,
    san: move.san,
    fen: chess.fen(),
  };
}

export interface ReplayStep {
  san: string;
  /** Position *before* this move — what the board shows while it is played. */
  fromFen: string;
  /** Position after. */
  toFen: string;
  from: Key;
  to: Key;
}

/**
 * Expand a SAN line (`Counterfactual.pv`) into per-move steps the board can
 * animate. Stops early — returning what it managed to expand — if the server
 * hands us a line that does not fit the position, so a bad PV degrades to a
 * short replay rather than a blank screen.
 */
export function expandSanLine(startFen: string, sanLine: readonly string[]): ReplayStep[] {
  const chess = safeChess(startFen);
  if (!chess) return [];
  const steps: ReplayStep[] = [];
  for (const san of sanLine) {
    const before = chess.fen();
    let move;
    try {
      move = chess.move(san);
    } catch {
      break;
    }
    if (!move) break;
    steps.push({
      san: move.san,
      fromFen: before,
      toFen: chess.fen(),
      from: move.from as Key,
      to: move.to as Key,
    });
  }
  return steps;
}

/**
 * A finished game has no candidate moves, so the analysis carries no score for
 * the eval bar. Checkmate is a certainty rather than a missing value, and the
 * bar should say so.
 */
export function terminalScore(fen: string): { kind: 'mate'; value: 0 } | { kind: 'cp'; value: 0 } | null {
  const chess = safeChess(fen);
  if (!chess || !chess.isGameOver()) return null;
  return chess.isCheckmate() ? { kind: 'mate', value: 0 } : { kind: 'cp', value: 0 };
}

export function uciToSquares(uci: string | null | undefined): [Key, Key] | undefined {
  if (!uci || uci.length < 4) return undefined;
  return [uci.slice(0, 2) as Key, uci.slice(2, 4) as Key];
}

/** Full-move number of the position, and whose move it is — for move numbering. */
export function moveNumberOf(fen: string): number {
  const n = Number(fen.split(' ')[5]);
  return Number.isFinite(n) ? n : 1;
}

function safeChess(fen: string): Chess | null {
  try {
    return new Chess(fen);
  } catch {
    return null;
  }
}
