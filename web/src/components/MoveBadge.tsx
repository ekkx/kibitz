import type { Key } from 'chessground/types';
import type { Classification } from '../api/types.ts';
import type { Color } from '../chess/rules.ts';
import { classificationColor, classificationLabel } from '../ui/format.ts';

/**
 * The verdict on the move just played, as a small coloured disc pinned to the
 * top-right corner of the square it landed on.
 *
 * The classification is already in the move list and in the panel, but both of
 * those are *away from the board*, and the board is where the eye is while a
 * game is being stepped through. The badge closes that gap: it says which move
 * is being judged by sitting on it, so the judgement needs no cross-reference —
 * which is exactly what the arrows cannot do, since an arrow says "this move"
 * and the badge says "and it was a blunder".
 *
 * This is deliberately *not* a chessground shape. `DrawShape.customSvg` renders
 * into a layer translated to the centre of the target square and then nests an
 * `<svg width="1" height="1" viewBox="0 0 100 100">`, so the only drawable area
 * is the square down-and-right of that centre point — the bottom-right quadrant
 * of the destination square, with everything else clipped by the nested
 * viewport. A top-right badge simply is not expressible there. An absolutely
 * positioned element over `.board-wrap` is, because that element *is* the
 * board's box: square, `position: relative`, and nothing between it and the
 * squares, so one board side is 100% and one square is 12.5% of it.
 */

export interface MoveBadgeProps {
  classification: Classification;
  /** Destination square of the move that was just played, e.g. 'e4'. */
  square: Key;
  /** Board orientation, so the badge lands on the right corner when flipped. */
  orientation: Color;
}

/* ------------------------------------------------------------------ *
 * The mark set
 * ------------------------------------------------------------------ */

/**
 * The marks are **drawn here**, as geometry, and not set in a font or pulled
 * from an icon package.
 *
 * They used to be typeset characters — `✓`, `★`, `?!`, `§` — centred in the
 * disc, and that is the one thing this file must never go back to. A glyph is
 * resolved by the OS from whatever font happens to cover that codepoint, so the
 * stroke weight, the optical size and the side bearings all changed between
 * macOS, Windows and Linux, and they changed *per character*: the check came out
 * hairline next to a heavy star, and `?!` was two characters typeset with word
 * spacing in a disc a third of a square wide. A family that has to read at 35 px
 * cannot be assembled out of parts drawn by nine different type designers.
 *
 * They are also not chess.com's or WintrChess's art, and not an icon library.
 * chess.com's badges are proprietary; WintrChess's are AGPL and visibly derived
 * from them, which is a licence on the repository rather than a grant over the
 * drawing. Copying either would be the fastest route to a set that cannot be
 * redistributed at all. Adding `lucide-react` or similar for nine shapes
 * would be a dependency, a bundle, and a second visual language, to get marks
 * that are *still* not tuned for a coloured disc at a third of a square. So:
 * **do not "improve" this by installing an icon package or importing an SVG
 * file.** The whole point is that the set is authored in this repository, in one
 * coordinate system, under one licence.
 *
 * Everything below is built from a shared system rather than drawn one shape at
 * a time, which is what makes nine unrelated symbols read as one family:
 *
 *   - **One grid.** Every path is authored in the same `0 0 24 24` box, on the
 *     same cap height, so "the same size" is a fact about the numbers rather
 *     than something eyeballed per mark.
 *   - **One stroke.** `STROKE` for every line in every mark, with round caps and
 *     round joins throughout. A font cannot promise this; a single constant can.
 *   - **One cap height.** The marks stand about 55–60% of the disc, which is the
 *     size at which a mark reads as a deliberate mark rather than as a character
 *     that happened to be dropped in the middle of a circle.
 *   - **Primitives, not letterforms.** The check is a two-segment polyline, the
 *     cross is two lines, `!` is a tapered bar and a dot, `?` is a 217° arc that
 *     curls into a stem, the star is a polygon on two radii. None of them traces
 *     a typeface, so none of them inherits a typeface's licence or its
 *     proportions.
 */

/** The side of the box every mark is authored in. */
export const GRID = 24;

/** The centre of that box, which is also the centre of the disc. */
const MID = GRID / 2;

/**
 * The stroke width shared by every line in every mark, in grid units — about an
 * eighth of the disc. At the size the badge actually renders (roughly 35 px on a
 * 770 px board) this is 4.5 physical pixels: heavy enough to survive the disc's
 * colour underneath it, thin enough that the counter of a `?` stays open.
 */
const STROKE = 3.1;

/**
 * The hairline applied to the *filled* marks — the star, the book, the bar and
 * dot of `!`.
 *
 * A fill has corners where a stroked mark has round caps, and a mark set where
 * half the terminals are round and half are sharp does not read as a family. A
 * same-coloured stroke with `stroke-linejoin: round` rounds every corner of a
 * filled outline by half its width, so the star's points and the book's
 * corners pick up the same softness the polylines get for free. It also grows
 * each filled shape by half of this on every side, which the geometry below
 * already accounts for.
 */
const SOFTEN = 0.8;

/**
 * One drawn element of a mark. A mark is a short list of these because several
 * of the symbols are genuinely two shapes — a bar and a dot, a hook and a dot,
 * two pages — and pretending otherwise would mean one unreadable path per mark.
 */
export interface MarkPart {
  /** Path data, in the `0 0 24 24` grid. */
  readonly d: string;
  /**
   * Stroke width in grid units, or `0` for a shape carried by its fill (which
   * is then softened by `SOFTEN`, see above).
   */
  readonly stroke: number;
  /**
   * Placement, for the halves of a two-mark badge. Absent means the part is
   * drawn exactly as authored, which is the case for every single-mark badge.
   */
  readonly transform?: string;
}

export type Mark = readonly MarkPart[];

/** A filled disc, as a path, so every part of every mark is one element type. */
function dot(cx: number, cy: number, r: number): MarkPart {
  return {
    d: `M ${round(cx - r)} ${round(cy)} A ${r} ${r} 0 1 1 ${round(cx + r)} ${round(cy)} A ${r} ${r} 0 1 1 ${round(cx - r)} ${round(cy)} Z`,
    stroke: 0,
  };
}

/**
 * The polygon of a five-pointed star, on an outer and an inner radius.
 *
 * Written as a loop rather than as ten literal coordinates because the *rule* is
 * the design — two radii and an alternating sweep — and ten hand-typed numbers
 * would be a shape nobody could adjust without redoing the trigonometry. The
 * inner radius is 45% of the outer rather than the 38% a "mathematical"
 * pentagram uses: at this size the classical ratio gives points thin enough to
 * be eaten by the `SOFTEN` rounding, and a star that is slightly fat holds its
 * silhouette all the way down.
 */
function starPath(cx: number, cy: number, outer: number, inner: number): string {
  const points: string[] = [];
  for (let step = 0; step < 10; step += 1) {
    // Start at the top point and alternate radii every 36°. `y` grows downwards
    // in SVG, so this walks clockwise, which the winding rule does not care
    // about but a reader might.
    const angle = (-90 + step * 36) * (Math.PI / 180);
    const radius = step % 2 === 0 ? outer : inner;
    points.push(`${round(cx + radius * Math.cos(angle))} ${round(cy + radius * Math.sin(angle))}`);
  }
  return `M ${points.join(' L ')} Z`;
}

const round = (value: number): number => Math.round(value * 1000) / 1000;

/**
 * **A note on the numbers below: they are optically centred, not geometrically
 * centred, and the difference is deliberate.**
 *
 * Several marks are *not* symmetric about x = 12 and must not be "corrected" to
 * be. The `?` family is the case that forced this. A question mark carries its
 * mass in the bowl, up and to the right, with only a thin tail and a dot
 * underneath; centre its bounding box on the disc and it visibly sits right of
 * centre, which is exactly the complaint the typeset version drew. It is not a
 * side-bearing problem — measured with the old badge's own font the ink was
 * centred in its advance to within a fifth of a pixel — it is the outline.
 *
 * The correction was measured rather than guessed. Each mark was rasterised at
 * 24× and reduced to two numbers: the centre of its ink bounding box, and its
 * alpha-weighted centroid. Neither alone is the visual centre — box-centring
 * ignores where the weight is, centroid-centring lets one far-flung light
 * element drag the whole mark across — so each mark is placed with the *midpoint
 * of the two* on x = 12. On this set that comes out as:
 *
 *     mark        box    centroid   → placed so the midpoint is 12
 *     ✓ ✕ ★ book  12.00  ±0.04        no correction needed
 *     ?           11.85  12.14        whole mark shifted 0.14 left
 *     ??          11.88  12.12        inherits the `?` shift, no extra nudge
 *     ?!          11.75  12.25        pair nudged 0.18 further left
 *
 * The vertical axis was measured the same way and deliberately left alone: every
 * mark's ink box is centred on y = 12, `!` and `?` occupy exactly the same band
 * (4.90 to 19.10) so the two marks with a dot sit at identical heights, and the
 * centroid rule was *not* applied there because it fights the shapes — it would
 * push the `?`'s dot down against the disc's edge and lift the star off centre,
 * for a difference in mass that a star and an exclamation mark have in every
 * typeface ever cut. Consistency of the box is what stops the mark jumping when
 * the user steps from one classification to the next, and that is the property
 * worth holding.
 */

/**
 * **Check** — two segments, the short one down-right, the long one up-right,
 * meeting at a 94° join. The long arm overshoots the short one by better than
 * two to one, which is what stops a check reading as a lazy V.
 */
const CHECK: Mark = [{ d: 'M 6.15 12.2 L 10.1 16.45 L 17.85 7.55', stroke: STROKE }];

/**
 * **Cross** — two lines through the centre. Its diagonals reach into the corners
 * of the cap-height box, so it is drawn a little *shorter* than the check (53%
 * of the disc against 58%): matched box for box, an X always looks the larger of
 * the two.
 */
const CROSS: Mark = [
  { d: 'M 7.0 7.0 L 17.0 17.0', stroke: STROKE },
  { d: 'M 17.0 7.0 L 7.0 17.0', stroke: STROKE },
];

/**
 * **Star** — a five-pointed polygon, sitting slightly low in the box because a
 * star's own centre of area is below the centre of its bounding box and the eye
 * follows the area, not the box.
 */
const STAR: Mark = [{ d: starPath(MID, 12.68, 7.08, 3.19), stroke: 0 }];

/**
 * **Exclamation** — a bar that tapers from 3.5 units at the top to 2.4 at the
 * bottom, plus a dot.
 *
 * The taper is the reason this is a fill and not a stroke. A parallel-sided bar
 * is what you get from a font's `!` at small optical sizes and it looks like a
 * pipe character; the narrowing is what makes the mark feel drawn. The ends are
 * still semicircles — the two `A` commands — so the terminals match the round
 * caps everywhere else, and the dot is drawn just under the bar's widest point,
 * which keeps the whole mark on one weight.
 *
 * The radii look 0.4 too small for those widths because they are: `SOFTEN` grows
 * every filled outline by half its width on each side, so a bar authored at 1.35
 * half-width *renders* at 1.75. Every filled mark in this file is authored
 * pre-shrunk like this, which is the only way the fills and the strokes end up
 * the same weight on screen.
 */
const BANG: Mark = [
  {
    d: 'M 10.65 6.65 A 1.35 1.35 0 0 1 13.35 6.65 L 12.8 13.3 A 0.8 0.8 0 0 1 11.2 13.3 Z',
    stroke: 0,
  },
  dot(MID, 17.5, 1.2),
];

/**
 * **Question** — a 217° arc that curls into a short stem, plus a dot.
 *
 * The bowl's radius (2.75) is set by the one hard constraint in the whole set:
 * with a 3.1 stroke the counter is `2 × 2.75 − 3.1 = 2.4` units, which is 3.5
 * physical pixels at the rendered size. Anything tighter and the hole fills in,
 * and a `?` whose counter has closed is a blob. That is also why the mark has
 * almost no stem — the bowl plus a full-weight dot plus the gap between them
 * already spends the whole cap height, so the tail curls in under the bowl and
 * stops rather than descending. It costs nothing: at 35 px the silhouette people
 * recognise is hook-over-dot, not the stem.
 *
 * The arc starts at 178° — the bowl's left, terminal pointing very slightly
 * down — rather than at the 135° a typeface would use. Starting lower brings the
 * terminal round towards the tail, and the two ends have to stay more than a
 * stroke width apart or the mark closes up into something nearer a `9`.
 *
 * The whole mark is then written 0.14 units left of where the geometry puts it —
 * the bowl's centre is 11.86, not 12 — which is the optical correction described
 * above. Please do not round it back to 12.
 */
const QUERY: Mark = [
  {
    d: 'M 9.11 9.3 A 2.75 2.75 0 1 1 14.11 10.78 C 13.19 12.09 12.16 12.3 12.16 13.0',
    stroke: STROKE,
  },
  dot(12.16, 17.5, 1.2),
];

/**
 * **Book**, for `book` — an opening move, still in theory.
 *
 * The old badge used `§`, the section sign, on the argument that it is the mark
 * a written work uses to point at a passage of itself. That is a good argument
 * about *meaning* and a bad one about *drawing*: `§` is two interlocking hooks
 * with a counter in each, which is roughly twice the detail a `?` has, in a mark
 * that has to survive at a third of a square. Redrawn honestly from primitives
 * it is a smudge; redrawn simply enough to read, it is no longer a section sign.
 *
 * So the mark is an open book: two pages splayed from a spine gap down the
 * middle, each with the outer edge full height and the top falling away into the
 * valley where they meet. Nothing else in the app is a pictograph, but nothing
 * else in the app has to say "this came from a reference work" either, and the
 * open book is about as close to a universal sign for that as exists — it needs
 * no key, at any size, in any locale. It is filled rather than stroked because
 * outlining the pages would put four thin lines where there is room for two
 * solid shapes, and the spine gap (1.6 units clear after `SOFTEN` grows each
 * page) is the one piece of negative space that has to survive; giving the pages
 * mass is what protects it.
 */
const BOOK: Mark = [
  {
    d: 'M 10.8 8.2 C 9.4 6.9 7.2 6.4 5.2 6.5 C 4.8 9.6 4.8 14.4 5.2 17.5 C 7.2 17.6 9.4 17.1 10.8 15.8 Z',
    stroke: 0,
  },
  {
    d: 'M 13.2 8.2 C 14.6 6.9 16.8 6.4 18.8 6.5 C 19.2 9.6 19.2 14.4 18.8 17.5 C 16.8 17.6 14.6 17.1 13.2 15.8 Z',
    stroke: 0,
  },
];

/**
 * Two marks in the width of one, which is the hardest thing this file does.
 *
 * Shrinking two copies and butting them together does not work: at 35 px the
 * result is two illegible marks instead of one legible one. What works is
 * treating the pair as a single composition and spending the disc on it —
 *
 *   - **Scale, which thins the stroke with it.** A `transform` scales stroke
 *     width too, so `?!` at 0.92 draws its lines at 2.85 units and `??` at 0.86
 *     draws them at 2.67. That is the "thinner in a pair" a type designer would
 *     do by hand, and here it is guaranteed rather than remembered.
 *   - **Spacing tighter than the marks' own side bearings.** 1.15 units between
 *     the two, roughly a third of what typeset text would leave. The pair has to
 *     read as one badge; a comfortable gap reads as two badges overlapping.
 *   - **A different scale for each pair**, because `!` is 3.5 units wide and `?`
 *     is 8.6. `?!` barely has to shrink at all — the exclamation costs almost
 *     nothing horizontally — while `??` has to find room for two bowls and two
 *     open counters, and 0.86 is the floor: below it the counters close and both
 *     marks turn into hooks.
 *   - **Optically, not metrically, centred.** The two `?`s of `??` balance each
 *     other, so it needs no nudge beyond the shift already baked into the `?`.
 *     `?!` does: the exclamation is narrow, so laying the pair out by *width*
 *     puts a mark of nearly the question mark's weight much further from the
 *     centre than the question mark itself, and the composition's mass ends up
 *     right of the disc's middle even though its box is dead centre. `?!` is
 *     therefore pushed 0.18 further left than the arithmetic asks for.
 *
 * The nudges are measured, not derived — the rasterise-and-weigh procedure in
 * the note above — which is why one of them is zero and the other is not a round
 * number. Changing a scale or the gap changes where the mass lands, so they have
 * to be re-measured together, not adjusted one at a time by eye.
 *
 * The result: `??` draws each counter at 2.06 units — 3.0 physical pixels of
 * clear space at the rendered size. That is thin, and it is the number this
 * design lives or dies on, but it holds: the two bowls stay open, and even where
 * a viewer cannot resolve the counters the doubled silhouette still says "worse
 * than one `?`", which is the whole semantic load a blunder badge carries.
 */
const PAIR_GAP = 1.15;

/** Measured optical corrections. Kept explicit — a zero here is a result too. */
const QUERY_BANG_NUDGE = -0.18;
const QUERY_QUERY_NUDGE = 0;

/** How wide a mark is at scale 1, outside edge to outside edge of the ink. */
const QUERY_WIDTH = 8.6;
const BANG_WIDTH = 3.5;

/**
 * Place a mark inside the disc: scaled about the grid's centre, then slid
 * sideways. Every mark being authored in the same box, at the same size, is what
 * makes this one line rather than a per-mark special case.
 */
function place(mark: Mark, scale: number, dx: number): Mark {
  const shift = `translate(${round(MID + dx - MID * scale)} ${round(MID - MID * scale)}) scale(${scale})`;
  return mark.map((part) => ({ ...part, transform: shift }));
}

/** The two halves of a pair, laid out around the disc's centre and then nudged. */
function pairOffsets(
  leftWidth: number,
  rightWidth: number,
  scale: number,
  nudge: number,
): [number, number] {
  const total = (leftWidth + rightWidth) * scale + PAIR_GAP;
  return [
    round(-total / 2 + (leftWidth * scale) / 2 + nudge),
    round(total / 2 - (rightWidth * scale) / 2 + nudge),
  ];
}

const QUERY_BANG_SCALE = 0.92;
const QUERY_QUERY_SCALE = 0.86;

const [queryBangLeft, queryBangRight] = pairOffsets(
  QUERY_WIDTH,
  BANG_WIDTH,
  QUERY_BANG_SCALE,
  QUERY_BANG_NUDGE,
);
const [queryQueryLeft, queryQueryRight] = pairOffsets(
  QUERY_WIDTH,
  QUERY_WIDTH,
  QUERY_QUERY_SCALE,
  QUERY_QUERY_NUDGE,
);

const QUERY_BANG: Mark = [
  ...place(QUERY, QUERY_BANG_SCALE, queryBangLeft),
  ...place(BANG, QUERY_BANG_SCALE, queryBangRight),
];

const QUERY_QUERY: Mark = [
  ...place(QUERY, QUERY_QUERY_SCALE, queryQueryLeft),
  ...place(QUERY, QUERY_QUERY_SCALE, queryQueryRight),
];

/**
 * The mark per classification.
 *
 * This is a second table alongside `CLASSIFICATION_GLYPH` in `ui/format.ts`
 * rather than a reuse of it, because the two answer different questions. That
 * one is *chess notation*: it appends to a move in the list, so `best` and
 * `good` are silent ('') — annotating every ordinary move with a mark would
 * make the list unreadable and claim more than the annotation convention does.
 * Here every classification must produce something, since the badge only exists
 * when there is a verdict to show and an empty disc is not a verdict. Merging
 * the two would mean one table with a hole in it and a caller-specific fallback,
 * which is two tables with extra steps.
 *
 * `excellent` and `good` share the check mark on purpose and are told apart by
 * colour alone. They differ by degree, not in kind — both are "fine, play on" —
 * and inventing a distinct symbol for each would dress a gradient up as a
 * category. chess.com draws them the same way for the same reason. Both point at
 * the *same* `CHECK` constant rather than at two copies of it, so the fact that
 * they share a drawing is enforced by identity and not by two path strings that
 * have to be kept equal by hand.
 */
export const BADGE_MARK: Record<Classification, Mark> = {
  great: BANG,
  best: STAR,
  excellent: CHECK,
  good: CHECK,
  book: BOOK,
  inaccuracy: QUERY_BANG,
  mistake: QUERY,
  blunder: QUERY_QUERY,
  miss: CROSS,
};

/* ------------------------------------------------------------------ *
 * Placement on the board
 * ------------------------------------------------------------------ */

/** One square as a fraction of the board, in percent. */
const SQUARE = 12.5;

/**
 * The badge's diameter, as a percent of one board side — a little over a third
 * of a square, which is big enough to carry two marks and small enough to leave
 * the piece under it recognisable.
 *
 * Exported because it is the single source of truth for the size: the component
 * writes it into the element's `width`/`height`, and `badgePlacement` needs it
 * to centre the disc on a corner and to clamp it. A copy of it in the
 * stylesheet would be a number that silently has to match this one.
 *
 * The value is chosen to halve exactly in binary (4.5 → 2.25), so every offset
 * this file computes is an exact double and the tests can compare with `toBe`
 * rather than an epsilon.
 */
export const BADGE_SIZE = 4.5;

export interface BadgePlacement {
  /** Offset from the board's left edge, in percent of one board side. */
  left: number;
  /** Offset from the board's top edge, in the same units. */
  top: number;
}

/**
 * Where the disc goes, in percent of one board side, for its top-left corner.
 *
 * The badge is centred on the *corner* between four squares rather than tucked
 * inside its own square: half of it overhangs the neighbours, which is what
 * makes it read as a marker attached to the square instead of a piece of the
 * position. The corner it wants is the destination square's top-right one, and
 * "top-right" is a fact about the picture, not about the board — flipping the
 * orientation flips which file and rank end up there, hence `orientation`.
 *
 * Both offsets are then clamped into the board. `.board-wrap` is
 * `overflow: hidden`, so a badge on the edge nearest the corner it hangs off —
 * the h-file and the 8th rank when White is at the bottom, the a-file and the
 * 1st rank when Black is — would have its outer half cut off, and a half-disc
 * with a clipped mark is a worse marker than one nudged a couple of percent
 * inwards. Clamping moves it just enough to sit flush against the board edge;
 * it stays over its own square either way, because the shift is never more than
 * half a badge and a badge is barely a third of a square.
 *
 * Returns `null` for a key that is not a square: chessground's `Key` includes
 * `'a0'`, its sentinel for "off the board", and the caller should render nothing
 * rather than a badge parked in a corner.
 */
export function badgePlacement(square: Key, orientation: Color): BadgePlacement | null {
  if (square.length !== 2) return null;
  const file = square.charCodeAt(0) - 'a'.charCodeAt(0);
  const rank = square.charCodeAt(1) - '1'.charCodeAt(0);
  if (file < 0 || file > 7 || rank < 0 || rank > 7) return null;

  // Column and row of the square as drawn, counted from the top-left of the
  // picture. White at the bottom means the a-file is the leftmost column and the
  // 8th rank the topmost row; Black at the bottom mirrors both.
  const column = orientation === 'white' ? file : 7 - file;
  const row = orientation === 'white' ? 7 - rank : rank;

  return {
    left: clamp((column + 1) * SQUARE - BADGE_SIZE / 2),
    top: clamp(row * SQUARE - BADGE_SIZE / 2),
  };
}

/** Keep the whole disc inside the board's box, which clips what leaves it. */
const clamp = (offset: number): number => Math.min(Math.max(offset, 0), 100 - BADGE_SIZE);

/**
 * One absolutely positioned element and nothing else, so it can be dropped
 * inside `.board-wrap` next to `<Board>` without disturbing it. It never takes
 * pointer events (`.move-badge` in the stylesheet): the badge sits over a real
 * square, and a marker that made that square undraggable would break the board
 * to describe it.
 *
 * Offsets are written in `cqw` rather than `%`, which is why the stylesheet
 * makes `.board-wrap` a container. Percentages would work for `left` — that is
 * a percentage of the wrap's width — but `top` would resolve against its height
 * and the disc's own `width`/`height` against two different axes, so a square
 * disc would depend on the wrap staying exactly square. `cqw` is one percent of
 * the board's *width* wherever it appears, so every number here is in the same
 * unit and the badge is round by construction.
 *
 * The mark is an `<svg>` scaled to the disc by CSS rather than sized in pixels,
 * so it is resolution-independent by construction: the same geometry is what
 * renders at 35 px on a laptop board and at twice that on a large one, and there
 * is no size at which it becomes a bitmap. It is `aria-hidden` because the
 * wrapper already carries `role="img"` and the classification's own label —
 * exposing the drawing separately would announce the verdict twice.
 */
export function MoveBadge({
  classification,
  square,
  orientation,
}: MoveBadgeProps): React.JSX.Element | null {
  const placement = badgePlacement(square, orientation);
  if (!placement) return null;

  const label = classificationLabel(classification);
  return (
    <div
      className="move-badge"
      role="img"
      aria-label={label}
      title={label}
      style={{
        left: `${placement.left}cqw`,
        top: `${placement.top}cqw`,
        width: `${BADGE_SIZE}cqw`,
        height: `${BADGE_SIZE}cqw`,
        // `backgroundColor` and not the `background` shorthand, so this sets the
        // fill and only the fill. The shorthand is inline and therefore
        // unbeatable by a class, which would silently make the stylesheet's
        // half of the disc unwritable — and the disc being *entirely* flat is a
        // decision the stylesheet documents, not an accident of specificity.
        backgroundColor: classificationColor(classification),
      }}
    >
      <svg
        className="move-badge-mark"
        viewBox={`0 0 ${GRID} ${GRID}`}
        aria-hidden="true"
        focusable="false"
      >
        <g stroke="currentColor" strokeLinecap="round" strokeLinejoin="round">
          {BADGE_MARK[classification].map((part, index) => (
            <path
              key={index}
              d={part.d}
              transform={part.transform}
              fill={part.stroke === 0 ? 'currentColor' : 'none'}
              strokeWidth={part.stroke === 0 ? SOFTEN : part.stroke}
            />
          ))}
        </g>
      </svg>
    </div>
  );
}
