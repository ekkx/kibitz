import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { DrawShape } from 'chessground/draw';
import type { Key } from 'chessground/types';
import {
  ChevronFirst,
  ChevronLast,
  ChevronLeft,
  ChevronRight,
  FlipVertical2,
  Moon,
  Sun,
} from 'lucide-react';
import { Badge } from './components/ui/badge.tsx';
import { Button } from './components/ui/button.tsx';
import { Card, CardAction, CardHeader, CardTitle } from './components/ui/card.tsx';
import { Separator } from './components/ui/separator.tsx';
import { cn } from '@/lib/utils';
import { Board } from './components/Board.tsx';
import { EvalBar } from './components/EvalBar.tsx';
import { MistakeNav, MoveTree } from './components/MoveTree.tsx';
import { PromotionPicker } from './components/PromotionPicker.tsx';
import { AnalysisPanel } from './components/AnalysisPanel.tsx';
import { ImportScreen } from './components/ImportScreen.tsx';
import { MoveBadge } from './components/MoveBadge.tsx';
import { SettingsMenu } from './components/SettingsMenu.tsx';
import { GameSweepButton, SweepSummary } from './components/GameSweep.tsx';
import { OpeningCaption } from './components/OpeningCaption.tsx';
import { HealthBanner, HealthChip } from './components/HealthBanner.tsx';
import { useHealth } from './state/useHealth.ts';
import { useLanguage } from './state/useLanguage.ts';
import { useSession } from './state/useSession.ts';
import { useExplanation } from './state/useExplanation.ts';
import { useSweep } from './state/useSweep.ts';
import { useSettings } from './state/useSettings.ts';
import { useTheme, usePersistent } from './state/usePersistent.ts';
import { REPLAY_MOVE_MS, useReplay } from './hooks/useReplay.ts';
import { useBoardTransition } from './hooks/useBoardTransition.ts';
import {
  needsPromotion,
  terminalScore,
  turnColor,
  uciToSquares,
  type Color,
  type PromotionPiece,
} from './chess/rules.ts';
import { formatSanLine, isNotable } from './ui/format.ts';
import { positionShapes, previewShapes } from './ui/arrows.ts';
import { findMistake, type MistakeDirection } from './ui/mistakes.ts';
import { playSound, soundForSan } from './ui/sounds.ts';
import { stepSound } from './ui/stepSound.ts';
import { deepestOpening } from './ui/opening.ts';
import { IS_MOCK } from './api/transport.ts';
import type { Candidate, Motif } from './api/types.ts';
import { t } from './i18n/index.ts';

export function App(): React.JSX.Element {
  const { health, recheck } = useHealth();
  // One setting, two effects: the catalogue every `t()` below reads from, and
  // the language the server is asked to write explanations in.
  const { options: languages, language, setLanguage } = useLanguage();
  const [, toggleTheme] = useTheme();
  const [orientation, setOrientation] = usePersistent<Color>('kibitz.orientation', 'white');
  const settings = useSettings();

  const session = useSession({ depth: settings.depth });
  const sweep = useSweep({
    sessionId: session.sessionId,
    tree: session.tree,
    recordAnalysis: session.recordAnalysis,
    depth: settings.depth,
    // No engine, nothing to sweep — and `checking` is not yet a yes.
    enabled: health.status === 'ok',
  });

  const analysis = session.analysis;
  const played = analysis?.context?.played ?? null;
  const counterfactual = analysis?.context?.counterfactual ?? null;
  const notable = played ? isNotable(played.classification) : false;

  // The opening the *selected* position belongs to, which is the deepest named
  // node on its path — a position past the end of a named line keeps the name.
  const opening = useMemo(
    () => deepestOpening(session.tree, session.currentId),
    [session.tree, session.currentId],
  );

  const replay = useReplay(counterfactual);
  const explanation = useExplanation({
    sessionId: session.sessionId,
    nodeId: session.currentNode?.id ?? null,
    lang: language,
    auto: notable,
    preloaded: analysis?.explanations?.[language],
  });

  /**
   * The counterfactual replay is started by hand, from **Replay line** in the
   * panel, and never by selecting a move. Taking the board away from the user
   * the instant they click a blunder — for two seconds of animation they did
   * not ask for, hiding the arrows that answer the question they clicked to ask
   * — is a worse trade than one button press. The explanation still arrives on
   * its own (`auto: notable` above): it streams into the panel and leaves the
   * board alone.
   */

  /**
   * Whether this render moved the board by exactly one ply, and so whether the
   * pieces slide or the board repaints (`hooks/useBoardTransition.ts`). Disabled
   * while the replay owns the board, which animates on its own clock below.
   */
  const transition = useBoardTransition(session.tree, session.currentId, !replay.active);

  /**
   * Sound rides on the same detector as the animation, and for the same reason:
   * a sound is a claim that a piece moved, so it has to be true exactly when the
   * board shows one moving. Jumping to move 34 makes no sound — there is no one
   * move to have made it — and neither does an analysis arriving or the board
   * being flipped.
   *
   * `transition` is referentially stable between transitions (the hook returns
   * the same object for every re-render at the same node), so this fires once per
   * crossed ply rather than once per render.
   */
  useEffect(() => {
    if (transition.move) playSound(stepSound(transition.move));
  }, [transition]);

  /**
   * The replay drives the board itself, outside the detector above, so its moves
   * would otherwise be the one silent thing on screen — and it is the one place
   * the user is watching rather than driving, where the sound is doing the most
   * work. `index` is -1 on the starting position and runs one past the end, and
   * both of those land outside `steps`, so only real moves are heard.
   */
  useEffect(() => {
    if (!replay.active) return;
    const step = replay.index >= 0 ? replay.steps[replay.index] : undefined;
    if (step) playSound(soundForSan(step.san));
  }, [replay.active, replay.index, replay.steps]);

  /**
   * A move the user is pointing at in the move list, and the two states that
   * make the board follow it.
   *
   * `hoverId` is what the pointer is over; `hoverNode` is when that actually
   * moves the board. Pointing at the move you are already on is not a preview
   * of anything — and treating it as one would strip the arrows off the board
   * for as long as the pointer stayed where the click left it.
   *
   * The preview reaches the *board* and stops there: the eval bar, the analysis
   * panel and the explanation keep describing the selected position throughout.
   * That is not an omission. A glance is not a selection, and re-flowing the
   * whole right-hand column — including a streaming explanation — for every
   * move the pointer crosses on its way down the list would be unreadable. The
   * board is where a position can change and change back with no cost; the
   * panel is where the selection lives, which is exactly what "leaving restores
   * what was selected" means.
   */
  const [hoverId, setHoverId] = useState<number | null>(null);

  /**
   * A promotion the board has started and the session has not been told about.
   * See `components/PromotionPicker.tsx`: chessground has already moved the
   * pawn, and nothing is committed until a piece is chosen.
   */
  const [promotion, setPromotion] = useState<{ from: Key; to: Key } | null>(null);
  const [boardSync, setBoardSync] = useState(0);

  const hoverNode = useMemo(() => {
    if (hoverId === null || hoverId === session.currentId) return null;
    // A replay owns the board, and a half-made move owns it too — neither wants
    // a third position arriving from a pointer somewhere else on the page.
    if (replay.active || promotion) return null;
    return session.tree?.nodes.find((node) => node.id === hoverId) ?? null;
  }, [hoverId, session.currentId, session.tree, replay.active, promotion]);

  /**
   * The move whose verdict the badge is showing: the hovered one while a hover
   * is on the board, the selected one otherwise. The badge belongs to the
   * position being displayed, so it follows the FEN rather than the selection.
   */
  const boardPlayed = hoverNode
    ? (session.analyses.get(hoverNode.id)?.context?.played ?? null)
    : played;
  /** Where the classification badge sits: the square the played move landed on. */
  const badgeSquare = uciToSquares(boardPlayed?.uci)?.[1] ?? null;

  const boardFen = replay.fen ?? hoverNode?.fen ?? session.currentNode?.fen ?? '';
  const lastMove = replay.active
    ? replay.lastMove
    : uciToSquares((hoverNode ?? session.currentNode)?.uci ?? undefined);

  /**
   * A hover is a glance, not navigation, so neither entering nor leaving one is
   * animated: the board cuts to the position and cuts back. Sliding a dozen
   * pieces to a position the pointer merely passed over — and a dozen more back
   * — is the twitch this feature would otherwise be made of.
   *
   * Leaving needs the ref. `useBoardTransition` answers a question about the
   * *selected* node, and a hover does not change it, so on the render where the
   * hover ends it is still reporting whatever ply the user last stepped.
   * `wasHovering` is what that render is missing: it holds what was on screen
   * when this component was last committed, which is precisely "did the board
   * just come back from a preview".
   */
  const hovering = hoverNode !== null;
  const wasHovering = useRef(false);
  useEffect(() => {
    wasHovering.current = hovering;
  });
  const boardAnimationMs = replay.active
    ? REPLAY_MOVE_MS
    : hovering || wasHovering.current || promotion
      ? 0
      : transition.animationMs;

  const onMove = useCallback(
    (from: Key, to: Key) => {
      replay.stop();
      // chessground has already moved the pawn; what it cannot know is what the
      // pawn became. Ask, and commit nothing until there is an answer.
      if (session.currentNode && needsPromotion(session.currentNode.fen, from, to)) {
        setPromotion({ from, to });
        return;
      }
      void session.playMove(from, to);
    },
    [replay, session],
  );

  /**
   * Both ways out of the picker put the board back where the app still has it.
   * chessground moved the pawn before asking, and until a piece is chosen the
   * app has not accepted that move — so on the way out, whichever way, the
   * board is re-synced to the position the app still believes in.
   *
   * Cancelling ends there. Choosing follows it with the real move, which comes
   * back from the server as a new position and slides the pawn onto the last
   * rank as the piece that was chosen — one movement, drawn once, in the right
   * direction.
   */
  const closePromotion = useCallback(() => {
    setPromotion(null);
    setBoardSync((sync) => sync + 1);
  }, []);

  const choosePromotion = useCallback(
    (piece: PromotionPiece) => {
      if (!promotion) return;
      const { from, to } = promotion;
      closePromotion();
      void session.playMove(from, to, piece);
    },
    [promotion, session, closePromotion],
  );

  // Navigating away is also an answer: the question was about a position the
  // user has left. (Selecting a node changes the FEN, so the board resets
  // itself — there is nothing to re-sync.)
  useEffect(() => setPromotion(null), [session.currentId]);

  const onPlaySan = useCallback(
    (san: string) => {
      replay.stop();
      void session.playSan(san);
    },
    [replay, session],
  );

  /**
   * The line the user is pointing at in the candidate list. It belongs to one
   * position, so selecting another node drops it.
   */
  const [preview, setPreview] = useState<Candidate | null>(null);
  useEffect(() => setPreview(null), [session.currentId]);

  /**
   * The board's two hovers, kept apart.
   *
   * Pointing at a *candidate* draws a line on the position that is on the
   * board; pointing at a *move* puts a different position on the board. They
   * are answers in different frames, and a board doing both at once would be
   * drawing one position's engine line over another position's pieces. So each
   * hover cancels the other on the way in: whichever the pointer is in now is
   * the only one live.
   */
  const previewCandidate = useCallback((candidate: Candidate | null) => {
    if (candidate) setHoverId(null);
    setPreview(candidate);
  }, []);

  const hoverMove = useCallback((nodeId: number | null) => {
    if (nodeId !== null) setPreview(null);
    setHoverId(nodeId);
  }, []);

  /**
   * Board shapes, in strict precedence — only one language speaks at a time
   * (see `ui/arrows.ts`):
   *
   *   1. a replay owns the board while it runs, and marks its motifs on the
   *      final position;
   *   2. otherwise nothing at all while a move in the list is being previewed:
   *      the board is showing a position the user is *glancing at*, and every
   *      arrow this app draws is about the selected position — the ranking from
   *      it, or the mistake into it. Drawn over a previewed position they would
   *      be a claim about the wrong board, and the one that reads worst is the
   *      red mistake arrow, which would appear to accuse a move three plies
   *      away. A preview changes the position and says nothing else;
   *   3. otherwise a previewed candidate, because the user is asking for it;
   *   4. otherwise the engine's ranked arrows for the position on the board.
   *
   * Only the last is governed by the arrow-count setting: the others are
   * answers to something the user did, not board density chosen in advance.
   */
  const shapes = useMemo<DrawShape[]>(() => {
    if (replay.active) return replay.finished ? motifShapes(counterfactual?.motifs ?? []) : [];
    if (hoverNode) return [];
    if (preview && analysis) return previewShapes(analysis.fen, preview);
    return positionShapes(analysis, settings.arrowCount);
  }, [
    replay.active,
    replay.finished,
    counterfactual,
    hoverNode,
    preview,
    analysis,
    settings.arrowCount,
  ]);

  /**
   * Where the next and previous mistakes are (`ui/mistakes.ts`), recomputed as
   * the sweep fills the tree in — which is what turns the controls on, one
   * verdict at a time, instead of leaving them dead until it finishes.
   */
  const prevMistake = useMemo(
    () => findMistake(session.tree, session.analyses, session.currentId, 'prev'),
    [session.tree, session.analyses, session.currentId],
  );
  const nextMistake = useMemo(
    () => findMistake(session.tree, session.analyses, session.currentId, 'next'),
    [session.tree, session.analyses, session.currentId],
  );

  const jumpToMistake = useCallback(
    (direction: MistakeDirection) => {
      const target = (direction === 'next' ? nextMistake : prevMistake).target;
      if (target === null) return;
      replay.stop();
      session.goto(target);
    },
    [nextMistake, prevMistake, replay, session],
  );

  // Keyboard navigation, the way every board tool works.
  const step = session.step;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
      // A half-made move is a modal moment: the board is mid-question, and
      // stepping the game out from under it would answer something else. The
      // picker takes Escape and the arrow keys for itself while it is open.
      if (promotion) return;
      switch (event.key) {
        case 'ArrowLeft':
          step('prev');
          break;
        case 'ArrowRight':
          step('next');
          break;
        /*
         * The vertical axis is "jump", against the horizontal "step" — and it
         * is the move list's own axis, which is where the mistakes are marked
         * and where the jump lands. ↓ goes forward through the game because the
         * list runs downwards.
         */
        case 'ArrowUp':
          jumpToMistake('prev');
          break;
        case 'ArrowDown':
          jumpToMistake('next');
          break;
        case 'Home':
          step('first');
          break;
        case 'End':
          step('last');
          break;
        case 'f':
          setOrientation(orientation === 'white' ? 'black' : 'white');
          break;
        default:
          return;
      }
      event.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [step, orientation, setOrientation, jumpToMistake, promotion]);

  const best = analysis?.candidates[0] ?? null;
  const sideToMove: Color = session.currentNode ? turnColor(session.currentNode.fen) : 'white';
  // A mated position has no candidates; the bar still has something to say.
  const terminal = session.currentNode ? terminalScore(session.currentNode.fen) : null;
  const barScore = best?.score ?? terminal;
  const barWinProb = best?.win_prob ?? (terminal ? (terminal.kind === 'mate' ? 0 : 0.5) : null);

  return (
    <div className="flex h-full flex-col">
      <header className="flex min-h-14 flex-none flex-wrap items-center gap-x-3 gap-y-2 border-b px-4 py-2">
        <div className="flex items-baseline gap-2.5">
          <span className="text-[0.9375rem] font-semibold tracking-tight">{t('app.name')}</span>
          {/* First thing to go when the bar gets tight: it is the only text up
              here that nobody needs twice. */}
          <span className="hidden text-xs text-muted-foreground xl:inline">
            {t('app.tagline')}
          </span>
        </div>

        {session.status === 'ready' && (
          <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
            <span className="truncate font-medium text-foreground">
              {session.headers.White ?? t('session.white')}
            </span>
            <span aria-hidden="true">·</span>
            <span className="truncate font-medium text-foreground">
              {session.headers.Black ?? t('session.black')}
            </span>
            {session.headers.Result && (
              <Badge variant="outline" className="font-mono">
                {session.headers.Result}
              </Badge>
            )}
          </div>
        )}

        {/*
          One group, pushed right by an auto margin, rather than a spacer div
          followed by loose buttons. It matters when the bar runs out of room:
          a spacer takes its free space on the first line only, so the controls
          that wrap fall to the *left* of the second line, under the brand. An
          auto margin belongs to the group, so the group wraps as a unit and the
          margin re-collects the free space on whichever line it lands on.
        */}
        <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
          {IS_MOCK && <Badge variant="secondary">{t('health.mockBadge')}</Badge>}
          <HealthChip health={health} />
          {session.status === 'ready' && <GameSweepButton sweep={sweep} />}
          <SettingsMenu
            settings={settings}
            languages={languages}
            language={language}
            onLanguageChange={setLanguage}
          />
          {/*
            Two icons, one shown at a time, decided in CSS rather than in React.
            `useTheme` still carries the legacy `'system'` choice, so the
            component cannot name the theme it is currently *rendering* without
            re-resolving `matchMedia` on every render — but `data-theme` on the
            root is always concrete, and the `dark:` variant reads exactly that.
            So the icon is right by construction, including on first paint.
          */}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={toggleTheme}
            title={t('theme.toggle')}
            aria-label={t('theme.toggle')}
          >
            <Sun className="dark:hidden" />
            <Moon className="hidden dark:block" />
          </Button>
          {session.status === 'ready' && (
            <Button variant="outline" size="sm" onClick={session.reset}>
              {t('session.newGame')}
            </Button>
          )}
        </div>
      </header>

      <HealthBanner health={health} onRetry={recheck} />

      {session.status !== 'ready' || !session.tree || !session.currentNode ? (
        <ImportScreen
          busy={session.status === 'opening'}
          error={session.openError}
          onOpen={(input) => void session.open(input)}
        />
      ) : (
        /*
          Two layouts, and they are genuinely different rather than one squeezed.

          Wide: a two-column grid the height of the window, where the board
          column is elastic with a floor and the sidebar has a floor *and* a
          ceiling — a move list three hundred pixels wider than its longest line
          is not a better move list — and each panel scrolls inside itself, so
          the board never moves while you read.

          Narrow: a plain column that scrolls as a page. This is a flex column,
          not the same grid with one track, because a fixed-height grid *shares
          out* its height: the board would take the space it asked for and the
          two panels below would be handed the leftovers, which was 57 pixels of
          analysis panel with a two-pixel scroller inside it. Stacked, the panels
          take the height their contents need and the page grows.
        */
        <main className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4 lg:grid lg:grid-cols-[minmax(420px,1fr)_minmax(380px,470px)] lg:overflow-hidden">
          {/*
            The board sizes itself from the available *height* — the square is
            derived from it via `aspect-square`, and `max-w-full` pulls it back
            in when the column is narrower than it is tall. Stacked there is no
            column height to derive from, so one is named, and the column is
            fixed at it rather than being allowed to shrink.
          */}
          <div className="flex h-[min(78vw,70vh)] min-h-0 shrink-0 flex-col items-center gap-3 lg:h-full">
            <div className="flex min-h-0 w-full flex-1 items-stretch justify-center gap-3">
              <EvalBar
                score={barScore}
                winProb={barWinProb}
                sideToMove={sideToMove}
                orientation={orientation}
              />
              {/*
                `@container` is not decoration: `MoveBadge` places and sizes
                itself in `cqw` — one percent of the board's width — so that it
                scales with a board free to be any size. That unit only exists
                if something declares itself a container, and this element is
                the only one that is exactly the board's box. Inline-size
                containment is safe here, because the width comes from the
                aspect ratio and the row's height, never from the contents.
              */}
              <div
                className={cn(
                  'relative aspect-square h-full max-w-full overflow-hidden rounded-2xl shadow-md @container',
                  replay.active && 'ring-2 ring-primary',
                )}
              >
                <Board
                  fen={boardFen}
                  orientation={orientation}
                  lastMove={lastMove}
                  onMove={replay.active ? null : onMove}
                  animationMs={boardAnimationMs}
                  syncKey={boardSync}
                  shapes={shapes}
                  /*
                    Scrolling over the board is the same navigation the ▶ / ◀
                    buttons and the arrow keys perform — `step` itself, not a
                    parallel path — so it inherits the single-ply animation
                    (`useBoardTransition`) and the move sound without either of
                    them learning that a wheel exists. Off during a replay, on
                    the same condition that takes the pieces away from the
                    mouse: the replay owns the position while it runs.
                  */
                  onWheelStep={replay.active ? null : step}
                />
                {/*
                  How the move that led to this position was classified, pinned
                  to the square it landed on. It is the verdict on a move that
                  has already been made, so unlike the arrows it is not board
                  density the user chose in advance and it ignores the
                  arrow-count setting — turning the arrows off asks not to be
                  told what to play, which is a different question.

                  Suppressed during a replay, where the board is showing a line
                  that was never played and no move on it has a classification.

                  It follows a move-list preview onto the previewed position,
                  because unlike the arrows it is a verdict *on the move that
                  led here* — which is the move being previewed. It is the one
                  mark that is still about the right board.
                */}
                {!replay.active && boardPlayed && badgeSquare && (
                  <MoveBadge
                    classification={boardPlayed.classification}
                    square={badgeSquare}
                    orientation={orientation}
                  />
                )}

                {promotion && session.currentNode && (
                  <PromotionPicker
                    dest={promotion.to}
                    color={turnColor(session.currentNode.fen)}
                    orientation={orientation}
                    onChoose={choosePromotion}
                    onCancel={closePromotion}
                  />
                )}
                {/*
                  The replay's own caption, over the board it has taken over.
                  `z-[3]` clears chessground's pieces and its highlight layers
                  and stays under the badge at 10 — though the badge is hidden
                  while a replay runs, so the two never actually meet.
                */}
                {replay.active && counterfactual && (
                  <div className="absolute inset-x-0 bottom-0 z-[3] flex items-center justify-between gap-2.5 bg-primary px-3 py-1.5 text-xs text-primary-foreground">
                    <span className="shrink-0">
                      {t(`counterfactual.${counterfactual.kind}` as const)} ·{' '}
                      {t('counterfactual.playing')}
                    </span>
                    <span className="truncate font-mono opacity-90">
                      {formatSanLine(counterfactual.start_fen, counterfactual.pv)}
                    </span>
                  </div>
                )}
              </div>
            </div>

            {/*
              One bordered toolbar rather than five loose buttons: these are a
              single instrument — the transport for the game — and grouping
              them says so, the way every other cluster of related controls on
              the page is grouped. The separator marks the one that is not
              navigation: flipping the board changes the view, not the position.
            */}
            <div className="flex flex-none items-center gap-0.5 rounded-4xl border bg-card p-1 shadow-sm">
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => step('first')}
                title={t('board.first')}
                aria-label={t('board.first')}
              >
                <ChevronFirst />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => step('prev')}
                title={t('board.prev')}
                aria-label={t('board.prev')}
              >
                <ChevronLeft />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => step('next')}
                title={t('board.next')}
                aria-label={t('board.next')}
              >
                <ChevronRight />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => step('last')}
                title={t('board.last')}
                aria-label={t('board.last')}
              >
                <ChevronLast />
              </Button>
              <Separator orientation="vertical" className="mx-1 h-4" />
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setOrientation(orientation === 'white' ? 'black' : 'white')}
                title={t('board.flip')}
                aria-label={t('board.flip')}
              >
                <FlipVertical2 />
              </Button>
            </div>
          </div>

          {/* Same story one level down: a column that grows when stacked, a
              height-sharing grid when there is a window height to share. */}
          <aside className="flex flex-col gap-4 lg:grid lg:min-h-0 lg:grid-rows-[minmax(140px,0.85fr)_minmax(0,1.6fr)]">
            <Card size="sm" className="flex min-h-0 flex-col gap-0 py-0">
              <CardHeader className="border-b py-3">
                <CardTitle className="text-sm">{t('tree.title')}</CardTitle>
                <CardAction className="flex flex-wrap items-center justify-end gap-2 self-center">
                  {sweep.summary && (
                    <span className="text-xs text-muted-foreground">
                      {t('sweep.done', { total: sweep.summary.moves })}
                    </span>
                  )}
                  <MistakeNav
                    prev={prevMistake}
                    next={nextMistake}
                    onJump={jumpToMistake}
                  />
                </CardAction>
              </CardHeader>
              <OpeningCaption opening={opening} />
              <MoveTree
                tree={session.tree}
                analyses={session.analyses}
                currentId={session.currentId}
                onSelect={(nodeId) => {
                  replay.stop();
                  session.goto(nodeId);
                }}
                onHover={hoverMove}
              />
              <SweepSummary sweep={sweep} />
            </Card>

            <AnalysisPanel
              analysis={analysis}
              status={session.analysisStatus}
              error={session.analysisError}
              replay={replay}
              explanation={explanation}
              sessionId={session.sessionId}
              nodeId={session.currentNode.id}
              lang={language}
              onPlaySan={onPlaySan}
              onPreview={previewCandidate}
              onAnalyze={session.analyzeCurrent}
            />
          </aside>
        </main>
      )}
    </div>
  );
}

/**
 * Motifs (DESIGN §8.5) drawn on the board once the replay reaches the end of
 * the line — the tactic the whole line was about, marked on the squares it
 * actually happens on.
 */
function motifShapes(motifs: Motif[]): DrawShape[] {
  const shapes: DrawShape[] = [];
  const arrow = (orig: string, dest: string, brush = 'red') =>
    shapes.push({ orig: orig as Key, dest: dest as Key, brush });
  const circle = (square: string, brush = 'red') => shapes.push({ orig: square as Key, brush });

  for (const motif of motifs) {
    switch (motif.kind) {
      case 'fork':
        circle(motif.attacker, 'blue');
        for (const target of motif.targets) arrow(motif.attacker, target);
        break;
      case 'pin':
        arrow(motif.attacker, motif.behind, 'yellow');
        circle(motif.pinned);
        break;
      case 'skewer':
        arrow(motif.attacker, motif.behind, 'yellow');
        circle(motif.front);
        break;
      case 'discovered_attack':
        for (const target of motif.targets) arrow(motif.revealed, target);
        break;
      case 'hanging':
        circle(motif.square);
        break;
      case 'back_rank':
        arrow(motif.attacker, motif.king);
        break;
    }
  }
  return shapes;
}
