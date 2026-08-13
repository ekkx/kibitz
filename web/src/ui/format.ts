import type { Classification, Node, Score } from '../api/types.ts';
import { t } from '../i18n/index.ts';

/**
 * Presentation rules that several components share: how a `Score` reads, what
 * glyph a classification gets, and how a node turns into "12." / "12…".
 */

/** The suffix chess notation puts after the move itself. Empty for the quiet ones. */
export const CLASSIFICATION_GLYPH: Record<Classification, string> = {
  book: '',
  great: '!',
  best: '',
  excellent: '',
  good: '',
  inaccuracy: '?!',
  mistake: '?',
  blunder: '??',
  miss: '×',
};

/** CSS custom property carrying the colour for each classification. */
export const classificationColor = (classification: Classification): string =>
  `var(--${classification})`;

export const classificationLabel = (classification: Classification): string =>
  t(`class.${classification}` as const);

/** Classifications worth drawing attention to in the move list. */
export function isNotable(classification: Classification): boolean {
  return (
    classification === 'great' ||
    classification === 'inaccuracy' ||
    classification === 'mistake' ||
    classification === 'blunder' ||
    classification === 'miss'
  );
}

/**
 * Classifications that lost something, as opposed to the merely notable ones:
 * `great` is worth pointing at too, but nothing was thrown away, so nothing
 * should be drawn on the board in red.
 */
export function isMistake(classification: Classification): boolean {
  return (
    classification === 'inaccuracy' ||
    classification === 'mistake' ||
    classification === 'blunder' ||
    classification === 'miss'
  );
}

/**
 * `Score` is a tagged union and the two arms read differently: centipawns are
 * a signed pawn count, mate is a move count whose sign says who is delivering
 * it. `pov` flips a side-to-move score to White's point of view.
 */
export function formatScore(score: Score, pov: 'stm' | 'white', sideToMove: 'white' | 'black'): string {
  const flip = pov === 'white' && sideToMove === 'black';
  if (score.kind === 'mate') {
    if (score.value === 0) return '#'; // mate is already on the board
    const value = flip ? -score.value : score.value;
    return `${value >= 0 ? '+' : '−'}${t('eval.mateIn', { n: Math.abs(value) })}`;
  }
  const cp = (flip ? -score.value : score.value) / 100;
  const sign = cp > 0 ? '+' : cp < 0 ? '−' : '';
  return `${sign}${Math.abs(cp).toFixed(2)}`;
}

/** Win probability of the side to move, expressed for White. */
export function whiteWinProb(winProb: number, sideToMove: 'white' | 'black'): number {
  return sideToMove === 'white' ? winProb : 1 - winProb;
}

export const percent = (value: number): string => `${Math.round(value * 100)}%`;

export function signedPercent(delta: number): string {
  const points = Math.round(delta * 100);
  return `${points > 0 ? '+' : points < 0 ? '−' : '±'}${Math.abs(points)}%`;
}

export interface MoveLabel {
  /** Full-move number, e.g. 9. */
  number: number;
  /** True when this node's move was White's. */
  white: boolean;
}

/**
 * The move that *led to* `node` was made by the side that is no longer to
 * move, so the node's own FEN tells us both the colour and the number.
 */
export function moveLabel(node: Node): MoveLabel {
  const parts = node.fen.split(' ');
  const white = parts[1] === 'b'; // black to move now → White just moved
  const fullmove = Number(parts[5] ?? '1') || 1;
  return { number: white ? fullmove : fullmove - 1, white };
}

/** SAN line rendered with move numbers, e.g. "9…b5 10.Nxb5 cxb5". */
export function formatSanLine(startFen: string, sanLine: readonly string[]): string {
  const parts = startFen.split(' ');
  let number = Number(parts[5] ?? '1') || 1;
  let whiteToMove = parts[1] !== 'b';
  const out: string[] = [];
  for (const san of sanLine) {
    if (whiteToMove) out.push(`${number}.${san}`);
    else out.push(out.length === 0 ? `${number}…${san}` : san);
    if (!whiteToMove) number += 1;
    whiteToMove = !whiteToMove;
  }
  return out.join(' ');
}
