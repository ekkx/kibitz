import { Chess } from 'chess.js';
import type { Key } from 'chessground/types';
import type { PositionAnalysis } from '../api/types.ts';
import { expandSanLine } from '../chess/rules.ts';

/**
 * Finding the moves inside a written explanation, so that pointing at one draws
 * it on the board.
 *
 * The explanation is prose with SAN in it — "Nxd7 drops a piece: after Qxd7 the
 * knight is simply gone". Reading `Qxg3` means finding g3 by eye, counting
 * files along the bottom of the board, and then working out which queen; the
 * arrow answers all three at once, and the answer costs a hover. That is the
 * whole feature, and everything below is about the two ways it can lie.
 *
 * ## Lie one: a word that is not a move
 *
 * `Be3` is a bishop move, a plausible variable name and, in the middle of a
 * German sentence, a fragment of a word. The pattern here is deliberately
 * narrow — it will not match across an ASCII letter or digit on either side —
 * but a pattern alone can never be enough, because the same string is a legal
 * move in one position and nonsense in the next.
 *
 * So no arrow is drawn on the strength of a token's spelling. Every token is
 * looked up in the *legal moves of an actual position* (`sanFrames`), and a
 * token naming a piece, a capture or a castle that no position on the list
 * accepts stays plain text. That check is what separates a feature from a bug:
 * an arrow drawn for `Be3` in a position where no bishop can reach e3 is a
 * confident, specific, wrong claim, and the user has no way to tell it from a
 * right one.
 *
 * ## A bare coordinate is a square before it is a move
 *
 * `e8` is written for two different things: the pawn push, and the square the
 * black king is standing on. The lookup above sorts them — and the token that
 * fails it is no longer thrown away. It is drawn as a **circle** on the square
 * it names, with nothing validated first, because a coordinate is a real square
 * in every position and a circle on one cannot be wrong. (Same brush as the
 * arrow, in `App`: it is one feature answering two shapes of question, "where
 * is Qxg3?" and "which one is e8?".)
 *
 * That is the case this was reported for. "Your king on e8 has no escape
 * squares" used to produce nothing at all — e8 is not a legal pawn move there,
 * so it failed the lookup and stayed plain text, as did `e6 e7 b7 d7 f7 c8 f8
 * h8` across the two explanations this was first checked against, every one of
 * them a square the reader wanted pointed at.
 *
 * Why legality is a fair test to hang that on is **occupancy**, and it is worth
 * writing down because it is not obvious: a square named as a *location*
 * usually has a piece standing on it, and a square with a piece on it is not a
 * legal pawn destination. "Your king on e8", "the e4 pawn", "the bishop on c8"
 * all name occupied squares, so all of them fail the move lookup and fall
 * through to a circle, which is what they meant. "After e4" and "if he plays
 * d5" name empty squares that really are legal pushes, and they keep their
 * arrow. Nothing that drew an arrow before draws less of one now; only the
 * tokens that were inert have changed.
 *
 * Occupancy is not standing in for anything else here, because a bare
 * coordinate cannot be a capture: a pawn capture is always written with its
 * file, `dxe5`, which is a different branch of the pattern.
 *
 * What is left over is a square that is empty, legal as a push, and meant as a
 * location — "e5 is weak", "he controls d4". Those draw an arrow where a circle
 * was wanted. Nothing in the token distinguishes them, the case is far rarer
 * than the one above, and the arrow at least ends on the square being discussed,
 * so it stays. `e8=Q` and `d4+` are not bare coordinates: they assert a move
 * outright and are validated like any other, with no circle to fall back to.
 *
 * ## Lie two: the wrong frame
 *
 * A move is only a move from somewhere. The explanation is about the move that
 * led to the position on the board, so the moves it names live in several
 * different positions: the played move and its alternatives come from the
 * position *before* it, replies come from the position on the board, and a
 * punishment line walks a position per ply as it goes. `explanationFens` builds
 * that list — see there for how it is bounded and ordered — and a token is
 * resolved in the first position on the list that has such a move.
 *
 * A frame other than the board's own means the arrow's squares are read against
 * pieces that have since moved. That is not new: the board already draws the
 * mistake pair one ply back (`ui/arrows.ts`), for the same reason — the move
 * being talked about is the subject, and the frame it belongs to is the frame
 * it has to be drawn in.
 *
 * Only a mention that resolved to a move carries a frame. A circle is drawn on
 * the square itself, and every position on the list agrees where that is.
 *
 * ## Streaming
 *
 * The text arrives token by token, so this runs against a prefix of a sentence
 * dozens of times. Two consequences, both handled here rather than by the
 * caller. A match that runs to the very end of the text is discarded while more
 * is coming, because `Rd8` becomes `Rd8#` and `e8` becomes `e8=Q` one character
 * later, and a half-arrived word must never become a move. And the expensive
 * half — turning a FEN into its legal moves — is separated out into `sanFrames`
 * so that it can be computed once per position and reused for every delta;
 * `findSanMentionsIn` is then a scan and a map lookup per token.
 */

/** A position, and the moves that are legal in it, keyed for lookup. */
export interface SanFrame {
  fen: string;
  /** Legal moves by SAN with check and annotation marks stripped. */
  moves: ReadonlyMap<string, ResolvedMove>;
}

export interface ResolvedMove {
  /** SAN as the position spells it, marks and all. */
  san: string;
  from: Key;
  to: Key;
}

/** Where a mention sits in the text, which is all the renderer needs of it. */
export interface MentionSpan {
  /** Index of the first character of the token in the text. */
  start: number;
  /** Index one past its last character. */
  end: number;
  /** The token exactly as written, including any `?!` the model added. */
  text: string;
}

/** A token that is a legal move in one of the frames: the board draws an arrow. */
export interface MoveMention extends MentionSpan, ResolvedMove {
  kind: 'move';
  /** The FEN it turned out to be a legal move in. */
  fen: string;
}

/**
 * A bare coordinate that is not a legal move anywhere on the list, so it names
 * a square and nothing more: the board draws a circle on it.
 */
export interface SquareMention extends MentionSpan {
  kind: 'square';
  square: Key;
}

/** One notation found in the text, and what it points at. */
export type SanMention = MoveMention | SquareMention;

export interface SanMentionOptions {
  /**
   * Whether more text is still arriving. While it is, a token touching the end
   * of the text is not yet known to be finished, so it is left alone.
   */
  streaming?: boolean;
}

/**
 * The SAN pattern.
 *
 * Written out rather than delegated to chess.js's own parser because the parser
 * answers "is this string a move" for a string you already have, and the
 * question here is "where in this paragraph is there a string worth asking
 * about" — and because the boundaries matter more than the body.
 *
 * The boundaries are ASCII-only on purpose. `(?<![A-Za-z0-9])` keeps `e4` out of
 * `Ne4` and out of an identifier, while still allowing the `.` of `12.Nf3` and
 * the `(` of a parenthetical. Refusing to match next to *any* letter would be
 * wrong for Japanese, where 「Nf3の局面」 puts a kana straight against the move
 * with no space, and that is exactly the text this has to work on.
 *
 * Zeroes are accepted for castling, as `ui/sounds.ts` accepts them, and a `0`
 * cannot appear anywhere else in the pattern — ranks are 1-8 — so normalising
 * them away is unambiguous. Trailing `!`/`?` annotations are part of the token
 * so that the whole of `Nxd7??` is one hoverable thing rather than a move with
 * two loose characters after it.
 */
const SAN_TOKEN =
  /(?<![A-Za-z0-9])(?:[O0]-[O0](?:-[O0])?|[KQRBN][a-h]?[1-8]?x?[a-h][1-8]|[a-h]x[a-h][1-8](?:=[QRBN])?|[a-h][1-8](?:=[QRBN])?)[+#]?[!?]{0,2}(?![A-Za-z0-9])/g;

/** The key a token and a generated SAN are compared under. */
function lookupKey(san: string): string {
  const bare = san.replace(/[!?]+$/, '').replace(/[+#]+$/, '');
  return bare.startsWith('0') ? bare.replace(/0/g, 'O') : bare;
}

/**
 * The square a token names when it turns out not to be a move — `null` for a
 * token that asserts one and so has no square to fall back to.
 *
 * Only the two bare characters qualify. `!`/`?` are the model's opinion of what
 * it just wrote and are stripped, but `+`, `#` and `=Q` describe a position that
 * only a move can produce, so a token carrying one of those is a claim about a
 * move whether or not any frame agrees; it fails, as `Nf3` fails, to nothing.
 */
function squareOf(token: string): Key | null {
  const bare = token.replace(/[!?]+$/, '');
  return /^[a-h][1-8]$/.test(bare) ? (bare as Key) : null;
}

/**
 * Turn positions into frames — the expensive half, hoisted so a caller can do
 * it once per position rather than once per streamed token.
 *
 * Duplicates and unparseable FENs are dropped, so a caller may pass whatever
 * list is convenient. Order is preserved and it is significant: `findSanMentions`
 * resolves a token in the first frame that accepts it.
 */
export function sanFrames(fens: readonly string[]): SanFrame[] {
  const frames: SanFrame[] = [];
  const seen = new Set<string>();

  for (const fen of fens) {
    if (!fen || seen.has(fen)) continue;
    seen.add(fen);
    let chess: Chess;
    try {
      chess = new Chess(fen);
    } catch {
      continue;
    }
    const moves = new Map<string, ResolvedMove>();
    for (const move of chess.moves({ verbose: true })) {
      moves.set(lookupKey(move.san), {
        san: move.san,
        from: move.from as Key,
        to: move.to as Key,
      });
    }
    frames.push({ fen, moves });
  }
  return frames;
}

/**
 * Every notation in `text` worth pointing at, in the order it appears: the
 * moves that are legal in one of `frames`, and the bare coordinates that are
 * not moves anywhere and so name squares. Everything else is absent, and the
 * caller renders those stretches as the plain text they are.
 *
 * `frames` may be empty. A move needs a position to be a move in and there is
 * none, so nothing resolves to an arrow — but a coordinate still names its
 * square, and that is true of a board this function was told nothing about.
 */
export function findSanMentionsIn(
  text: string,
  frames: readonly SanFrame[],
  options: SanMentionOptions = {},
): SanMention[] {
  const mentions: SanMention[] = [];

  SAN_TOKEN.lastIndex = 0;
  for (let match = SAN_TOKEN.exec(text); match; match = SAN_TOKEN.exec(text)) {
    const token = match[0];
    const start = match.index;
    const end = start + token.length;
    // Still growing: `Rd8` is one character short of `Rd8#`, `e8` of `e8=Q` —
    // which would turn a square into a move — and one more delta could make
    // either of them a longer word entirely.
    if (options.streaming === true && end === text.length) break;

    const key = lookupKey(token);
    const move = firstMove(frames, key);
    if (move) {
      mentions.push({ kind: 'move', ...move.move, start, end, text: token, fen: move.fen });
      continue;
    }

    const square = squareOf(token);
    if (square) mentions.push({ kind: 'square', square, start, end, text: token });
  }
  return mentions;
}

/** The earliest frame that has such a move — see "Lie two" above on the order. */
function firstMove(
  frames: readonly SanFrame[],
  key: string,
): { move: ResolvedMove; fen: string } | null {
  for (const frame of frames) {
    const move = frame.moves.get(key);
    if (move) return { move, fen: frame.fen };
  }
  return null;
}

/** The single-position case: every SAN in `text` that is legal in `fen`. */
export function findSanMentions(
  text: string,
  fen: string,
  options?: SanMentionOptions,
): SanMention[] {
  return findSanMentionsIn(text, sanFrames([fen]), options);
}

/**
 * How deep into a quoted line moves are still resolved. Every ply costs one
 * frame, i.e. one legal-move generation, and the whole point of a punishment
 * line is that the explanation walks it — but a runaway `long_pv` should not
 * turn selecting a move into a hundred of them.
 */
const LINE_PLIES = 12;

/**
 * The positions an explanation's moves can be quoted from, best guess first.
 *
 * The model is given one `AnalysisContext` and forbidden to mention anything
 * outside it (`crates/llm/src/prompt.rs`), so this list is not a guess about
 * prose in general — it is the set of positions the facts in that JSON belong
 * to:
 *
 *   1. **Before the move.** `played.san` and `context.candidates` — the move
 *      that was made and the ones that should have been — are moves from here,
 *      and they are what the first paragraph is about. First, so that a token
 *      legal in two frames is read as the subject of the explanation rather
 *      than as something a line happens to reach later.
 *   2. **The position on the board.** Replies to the played move, and
 *      `analysis.candidates`.
 *   3. **Along the counterfactual line**, a frame per ply. The prompt tells the
 *      model to *walk* this line, so most of the deep moves in the text are on
 *      it, and without these frames the second half of nearly every explanation
 *      would be inert.
 *   4. **Along the line that should have been played**, likewise.
 *   5. **Along `outlook.long_pv`**, when the server sent one.
 *
 * A line that does not fit its starting position expands to nothing
 * (`expandSanLine`) rather than poisoning the list, so a stale or malformed PV
 * costs coverage and never correctness.
 */
export function explanationFens(
  analysis: PositionAnalysis | null,
  beforeFen: string | null,
): string[] {
  if (!analysis) return [];
  const fens: string[] = [];
  if (beforeFen) fens.push(beforeFen);
  fens.push(analysis.fen);

  const line = (startFen: string | undefined, sanLine: readonly string[] | undefined): void => {
    if (!startFen || !sanLine || sanLine.length === 0) return;
    for (const step of expandSanLine(startFen, sanLine.slice(0, LINE_PLIES))) {
      fens.push(step.fromFen);
    }
  };

  const context = analysis.context;
  const counterfactual = context?.counterfactual;
  line(counterfactual?.start_fen, counterfactual?.pv);
  if (beforeFen) line(beforeFen, context?.candidates?.[0]?.pv);
  line(analysis.fen, analysis.context?.outlook?.long_pv);

  return fens;
}
