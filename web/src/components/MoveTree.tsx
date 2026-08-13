import { Fragment, useEffect, useRef } from 'react';
import { ChevronsDown, ChevronsUp, TriangleAlert } from 'lucide-react';
import type { GameTree, Node, PositionAnalysis } from '../api/types.ts';
import { Button } from './ui/button.tsx';
import { ScrollArea } from './ui/scroll-area.tsx';
import { cn } from '@/lib/utils';
import {
  CLASSIFICATION_GLYPH,
  classificationColor,
  classificationLabel,
  moveLabel,
} from '../ui/format.ts';
import type { MistakeDirection, MistakeJump } from '../ui/mistakes.ts';
import { t } from '../i18n/index.ts';

export interface MoveTreeProps {
  tree: GameTree;
  analyses: Map<number, PositionAnalysis>;
  currentId: number;
  onSelect: (nodeId: number) => void;
  /**
   * Point at a move to see its position; `null` when the pointer leaves the
   * list. `App` owns what that does to the board — this component knows only
   * which move is under the cursor.
   */
  onHover: (nodeId: number | null) => void;
}

/**
 * The variation tree (DESIGN §7): `children[0]` is the mainline and
 * `children[1..]` are branches, rendered indented under the move they diverge
 * from — the same shape PGN export uses for `( … )`.
 *
 * Each move carries its classification glyph in the classification's colour,
 * so scanning the list shows where the game went wrong without reading a word.
 *
 * The moves themselves are hand-built inline elements rather than `Button`s.
 * This is notation, not a toolbar: the moves have to flow and wrap as text at
 * the line height of the list, and every button in the design system is a
 * fixed-height box with its own padding, which is the one thing that cannot be
 * true here. What they do borrow is the system's hover and selection colours,
 * so a move still highlights the way everything else on the page highlights.
 */
export function MoveTree({
  tree,
  analyses,
  currentId,
  onSelect,
  onHover,
}: MoveTreeProps): React.JSX.Element {
  const currentRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  /**
   * Keep the selected move on screen.
   *
   * Without this the list simply stayed where it was: walking to move 40 with
   * the arrow keys or the wheel left the one element that says *where you are*
   * several screens above the fold, which is the same as not having a move list.
   *
   * Two details do the whole job. `block: 'nearest'` scrolls the minimum
   * distance and does nothing at all when the move is already visible, so
   * stepping through the middle of the list does not jerk it to the centre on
   * every ply. And the effect depends on `currentId` alone: an analysis
   * arriving, a sweep event, the language changing — none of those move the
   * list out from under a user who scrolled it themselves, because none of them
   * change which move is selected.
   */
  useEffect(() => {
    const current = currentRef.current;
    if (current) {
      current.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      return;
    }
    // The root is the one selectable position with no move to scroll to — it is
    // the position *before* the first one, and the top of the list is where it
    // is. Without this, ⏮ / Home leaves the list sitting on move 30 while the
    // board shows the opening position, which is the same disagreement this
    // effect exists to prevent.
    listRef.current?.closest('[data-slot="scroll-area-viewport"]')?.scrollTo({ top: 0 });
  }, [currentId]);

  const byId = new Map(tree.nodes.map((node) => [node.id, node]));
  const root = byId.get(tree.root);

  if (!root || root.children.length === 0) {
    return (
      <div className="min-h-0 flex-1 px-4 py-3 text-sm text-muted-foreground">
        {t('tree.empty')}
      </div>
    );
  }

  return (
    <ScrollArea className="min-h-0 flex-1">
      {/*
        One `mouseleave` on the whole list rather than one per move: leaving a
        move for the gap between two moves is not leaving the list, and a
        per-move handler would flicker the board off and on again as the pointer
        crossed. The individual moves only ever say "it is me now".
      */}
      <div
        className="px-3 py-2 font-mono text-[0.8125rem] leading-[1.9]"
        onMouseLeave={() => onHover(null)}
        ref={listRef}
      >
        <Line
          byId={byId}
          fromId={tree.root}
          analyses={analyses}
          currentId={currentId}
          currentRef={currentRef}
          onSelect={onSelect}
          onHover={onHover}
          forceNumber
        />
      </div>
    </ScrollArea>
  );
}

/**
 * Jump to the next or previous move that lost something.
 *
 * It sits in the move list's own header, not in the board toolbar beside
 * ⏮ ◀ ▶ ⏭. Those five controls are one instrument — the transport for the game
 * — and every one of them moves by a fixed structural amount; a sixth and
 * seventh button in that row would read as two more ways to step, which is
 * exactly what this is not. This is a *query over the annotated list*: it goes
 * to the next move the list has already marked with `?!` or `??`, in the list's
 * own vertical direction, and the answer depends on analysis rather than on
 * where the moves are. Putting it against the list's title says that, keeps it
 * next to the glyphs it hunts for, and leaves the transport a transport.
 *
 * A disabled control here has two quite different reasons, and it says which:
 * there are no more mistakes this way, or the sweep has not got that far yet.
 * Collapsing them would let a half-analysed game claim to be clean.
 */
export function MistakeNav({
  prev,
  next,
  onJump,
}: {
  prev: MistakeJump;
  next: MistakeJump;
  onJump: (direction: MistakeDirection) => void;
}): React.JSX.Element {
  return (
    <div className="flex items-center gap-0.5 rounded-4xl border bg-card p-0.5">
      {/* Names the pair. Two bare chevrons in a header say "scroll something",
          and the tooltips are one hover too far away to be the only answer. */}
      <TriangleAlert className="mx-1 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <MistakeButton jump={prev} direction="prev" onJump={onJump} />
      <MistakeButton jump={next} direction="next" onJump={onJump} />
    </div>
  );
}

function MistakeButton({
  jump,
  direction,
  onJump,
}: {
  jump: MistakeJump;
  direction: MistakeDirection;
  onJump: (direction: MistakeDirection) => void;
}) {
  const label = t(direction === 'next' ? 'tree.mistakeNext' : 'tree.mistakePrev');
  const reason = jump.pending ? t('tree.mistakeUnknown') : t('tree.mistakeNone');

  return (
    <Button
      variant="ghost"
      size="icon-sm"
      className="size-6"
      disabled={jump.target === null}
      onClick={() => onJump(direction)}
      title={jump.target === null ? `${label} — ${reason}` : label}
      aria-label={label}
    >
      {direction === 'next' ? <ChevronsDown /> : <ChevronsUp />}
    </Button>
  );
}

interface LineProps {
  byId: Map<number, Node>;
  fromId: number;
  analyses: Map<number, PositionAnalysis>;
  currentId: number;
  currentRef: React.RefObject<HTMLButtonElement | null>;
  onSelect: (nodeId: number) => void;
  onHover: (nodeId: number | null) => void;
  /** Variations always restate the move number, even for a Black move. */
  forceNumber: boolean;
}

function Line({
  byId,
  fromId,
  analyses,
  currentId,
  currentRef,
  onSelect,
  onHover,
  forceNumber,
}: LineProps) {
  const items: React.JSX.Element[] = [];
  let cursor = byId.get(fromId);
  let first = true;

  while (cursor && cursor.children.length > 0) {
    const mainId = cursor.children[0]!;
    const main = byId.get(mainId);
    if (!main) break;

    items.push(
      <MoveButton
        key={`m${mainId}`}
        node={main}
        analyses={analyses}
        current={currentId === mainId}
        currentRef={currentRef}
        showNumber={first && forceNumber}
        onSelect={onSelect}
        onHover={onHover}
      />,
    );

    for (const altId of cursor.children.slice(1)) {
      const alt = byId.get(altId);
      if (!alt) continue;
      items.push(
        <span
          className="my-0.5 ml-2.5 block border-l-2 pl-2.5 text-xs leading-[1.75] text-muted-foreground"
          key={`v${altId}`}
        >
          <span className="pr-1 text-[0.625rem] tracking-wide text-muted-foreground/70">
            {t('tree.variation')}
          </span>
          <MoveButton
            node={alt}
            analyses={analyses}
            current={currentId === altId}
            currentRef={currentRef}
            showNumber
            onSelect={onSelect}
            onHover={onHover}
          />
          <Line
            byId={byId}
            fromId={altId}
            analyses={analyses}
            currentId={currentId}
            currentRef={currentRef}
            onSelect={onSelect}
            onHover={onHover}
            forceNumber={false}
          />
        </span>,
      );
    }

    cursor = main;
    first = false;
  }

  return <Fragment>{items}</Fragment>;
}

function MoveButton({
  node,
  analyses,
  current,
  currentRef,
  showNumber,
  onSelect,
  onHover,
}: {
  node: Node;
  analyses: Map<number, PositionAnalysis>;
  current: boolean;
  currentRef: React.RefObject<HTMLButtonElement | null>;
  showNumber: boolean;
  onSelect: (nodeId: number) => void;
  onHover: (nodeId: number | null) => void;
}) {
  const label = moveLabel(node);
  const classification = analyses.get(node.id)?.context?.played.classification;
  const glyph = classification ? CLASSIFICATION_GLYPH[classification] : '';

  return (
    <>
      {(label.white || showNumber) && (
        <span className="pr-0.5 text-muted-foreground/70 select-none">
          {label.number}
          {label.white ? '.' : '…'}
        </span>
      )}
      <button
        // Exactly one move is `current`, so this ref is never contested; it is
        // what the scroll effect above reaches for.
        ref={current ? currentRef : undefined}
        className={cn(
          'mr-px inline-flex items-baseline gap-px rounded-md px-1.5 font-[inherit] text-[inherit] transition-colors outline-none',
          'focus-visible:ring-[3px] focus-visible:ring-ring/40',
          current
            ? 'bg-primary text-primary-foreground'
            : 'hover:bg-muted hover:text-foreground',
        )}
        onClick={() => onSelect(node.id)}
        /*
         * Pointing at a move shows its position; clicking still selects it.
         * Only the pointer previews. Focus deliberately does not: the moves are
         * in the tab order so the keyboard can reach them, and previewing on
         * focus would mean Tab silently moved the board past a dozen positions
         * on its way somewhere else. The keyboard already has ← → for that, and
         * they select rather than glance.
         */
        onMouseEnter={() => onHover(node.id)}
        // The tooltip is the only place the glyph is spelled out, so it is the
        // translated label — not the raw classification off the wire.
        title={classification ? classificationLabel(classification) : undefined}
      >
        {node.san}
        {glyph && (
          <span
            className="text-[0.92em] font-bold"
            /*
             * The selected move inverts to the primary colour, and a saturated
             * classification hue on that ground is unreadable — so on the
             * current move the glyph simply inherits, and the selection is what
             * it says instead. This is why the colour is applied conditionally
             * rather than always and then fought with `!important`, which is
             * what the hand-written stylesheet had to do.
             */
            style={current ? undefined : { color: classificationColor(classification!) }}
          >
            {glyph}
          </span>
        )}
      </button>
    </>
  );
}
