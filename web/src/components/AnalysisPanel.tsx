import { useCallback, useMemo, useRef, useState } from 'react';
import { Play, Square } from 'lucide-react';
import type { Candidate, Motif, PositionAnalysis } from '../api/types.ts';
import type { AnalysisStatus } from '../state/useSession.ts';
import type { ExplanationState } from '../state/useExplanation.ts';
import type { ReplayState } from '../hooks/useReplay.ts';
import { Badge } from './ui/badge.tsx';
import { Button } from './ui/button.tsx';
import { Card, CardAction, CardHeader, CardTitle } from './ui/card.tsx';
import { Input } from './ui/input.tsx';
import { ScrollArea } from './ui/scroll-area.tsx';
import { cn } from '@/lib/utils';
import { ask } from '../api/client.ts';
import { turnColor } from '../chess/rules.ts';
import {
  CLASSIFICATION_GLYPH,
  classificationColor,
  classificationLabel,
  formatScore,
  percent,
  signedPercent,
} from '../ui/format.ts';
import { findSanMentionsIn, type SanFrame, type SanMention } from '../ui/sanMentions.ts';
import { t } from '../i18n/index.ts';

export interface AnalysisPanelProps {
  analysis: PositionAnalysis | null;
  status: AnalysisStatus;
  error: string | null;
  replay: ReplayState;
  explanation: ExplanationState;
  sessionId: string | null;
  nodeId: number | null;
  /** Explanation language, forwarded to `/ask`. */
  lang: string;
  /** The positions the written text quotes moves from (`ui/sanMentions.ts`). */
  frames: readonly SanFrame[];
  onPlaySan: (san: string) => void;
  /** Draw this candidate's line on the board; `null` clears the preview. */
  onPreview: (candidate: Candidate | null) => void;
  /** Draw a move named in the text; `null` clears it. */
  onHoverSan: (mention: SanMention | null) => void;
  onAnalyze: () => void;
}

/**
 * Everything the engine and the model have to say about the selected position,
 * in one scrolling column.
 *
 * The column is a stack of unlabelled-to-lightly-labelled sections rather than
 * a stack of cards. It is already inside a card; nesting a second frame around
 * each section would draw eight borders to separate things that are all answers
 * to the same question. The one exception is the counterfactual, which gets a
 * sunken block — it is the only section describing a position that is *not* on
 * the board, and that difference is worth a frame.
 */
export function AnalysisPanel({
  analysis,
  status,
  error,
  replay,
  explanation,
  sessionId,
  nodeId,
  lang,
  frames,
  onPlaySan,
  onPreview,
  onHoverSan,
  onAnalyze,
}: AnalysisPanelProps): React.JSX.Element {
  const played = analysis?.context?.played ?? null;
  const counterfactual = analysis?.context?.counterfactual ?? null;
  const sideToMove = analysis ? turnColor(analysis.fen) : 'white';

  return (
    <Card size="sm" className="flex min-h-0 flex-col gap-0 py-0">
      <CardHeader className="border-b py-3">
        <CardTitle className="text-sm">{t('analysis.title')}</CardTitle>
        {/*
          The depth of what is on screen, and — while a search is running — the
          only thing that says so.

          `analysis.depth` is read off whichever object the panel is describing,
          which during a search is the partial result (`state/useSession.ts`), so
          the number here is always the depth of the numbers beside it. It counts
          up as the engine reports iterations.

          The pulse is the searching indicator, and it is the whole of it. A
          spinner or a second "analysing…" line would be a separate claim about
          the same fact, competing with the one piece of information that is
          actually changing; making the changing number itself look unsettled
          says "this is still moving" without adding anything to read.
        */}
        {analysis && (
          <CardAction
            className={cn(
              'self-center font-mono text-xs text-muted-foreground',
              status === 'loading' && 'animate-pulse',
            )}
          >
            {t('eval.depth', { n: analysis.depth })}
          </CardAction>
        )}
      </CardHeader>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col items-start gap-5 px-4 pt-4 pb-5">
          {status === 'error' && (
            <p className="text-sm text-destructive">
              {t('analysis.failed', { message: error ?? '' })}
            </p>
          )}

          {!analysis && status === 'loading' && (
            <p className="text-sm text-muted-foreground">{t('analysis.analyzing')}</p>
          )}

          {/* The button is the whole empty state. It used to be introduced by a
              sentence explaining that selecting or playing a move analyses the
              position — which is what the button underneath it said, in more
              words and one step further away from being pressed. */}
          {!analysis && status !== 'loading' && (
            <Button variant="outline" size="sm" onClick={onAnalyze}>
              {t('analysis.analyzeNow')}
            </Button>
          )}

          {/*
            The verdict, and what stands here while it does not exist yet:
            nothing.

            `/analyze` streams the engine's ranking as it deepens but sends the
            classification, the accuracy and the counterfactual once, at the
            final depth (API.md). So for the couple of hundred milliseconds a
            search takes, this block and the counterfactual below it are simply
            absent, and the panel is a candidate list under a climbing depth.

            Not a skeleton, for two reasons. The shape is unknown — this region
            is a badge plus three figures plus, sometimes, a counterfactual with
            a variable number of motif chips — so a placeholder would be a guess
            that resolves to a different height, which is the jump a skeleton
            exists to prevent. And there is already an honest progress signal a
            few pixels away: the depth in the header, pulsing while it climbs.
            A skeleton would be a second, louder claim that something is coming,
            in the one part of the panel that must not appear to be saying
            anything yet.

            What matters more than either is what is *not* here: the previous
            node's verdict. `analysis` is keyed on the selected node
            (`state/useSession.ts`), so a node with no finished analysis has no
            `context` and no `played` — never the last one's.
          */}
          {played && (
            <div className="flex w-full flex-col gap-2">
              <div className="flex flex-wrap items-center gap-2.5">
                <span className="font-mono text-xl font-semibold tracking-tight">
                  {played.san}
                </span>
                {/*
                  The classification colour is data, not a variant: it is chosen
                  by name at runtime and is deliberately the same in both themes
                  (see `styles.css`). So the badge takes the system's shape and
                  overrides only its fill, with white ink — the same bargain
                  `MoveBadge` strikes on the board, and for the same reason.
                */}
                <Badge
                  className="border-transparent text-white"
                  style={{ backgroundColor: classificationColor(played.classification) }}
                >
                  {CLASSIFICATION_GLYPH[played.classification] && (
                    <span className="font-mono">
                      {CLASSIFICATION_GLYPH[played.classification]}
                    </span>
                  )}
                  {classificationLabel(played.classification)}
                </Badge>
              </div>
              <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                <span>
                  {t('analysis.accuracyLabel')} <Figure>{Math.round(played.accuracy)}%</Figure>
                </span>
                <span>
                  {t('eval.winProb')} <Figure>{percent(played.win_prob_after)}</Figure>{' '}
                  <span
                    style={
                      played.delta < -0.05 ? { color: classificationColor('blunder') } : undefined
                    }
                  >
                    ({signedPercent(played.delta)})
                  </span>
                </span>
                <span>
                  {analysis?.context?.played_rank === null ||
                  analysis?.context?.played_rank === undefined
                    ? t('analysis.notInTop')
                    : t('analysis.rank', { n: analysis.context.played_rank + 1 })}
                </span>
              </div>
            </div>
          )}

          {counterfactual && replay.steps.length > 0 && (
            <div className="w-full rounded-3xl bg-muted/60 p-3.5">
              <div className="mb-2.5 flex items-center justify-between gap-3">
                <span className="text-sm font-medium">
                  {t(`counterfactual.${counterfactual.kind}` as const)}
                </span>
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={replay.active ? replay.stop : replay.start}
                >
                  {replay.active ? <Square /> : <Play />}
                  {replay.active ? t('counterfactual.stop') : t('counterfactual.replay')}
                </Button>
              </div>
              <div className="flex flex-wrap gap-1 font-mono text-xs">
                {replay.steps.map((step, index) => (
                  <span
                    key={`${step.san}-${index}`}
                    className={cn(
                      'rounded-md px-1.5 py-px transition-colors',
                      replay.active && index === replay.index
                        ? 'bg-primary text-primary-foreground'
                        : replay.active && index < replay.index
                          ? 'text-foreground'
                          : 'text-muted-foreground',
                    )}
                  >
                    {step.san}
                  </span>
                ))}
              </div>
              {counterfactual.motifs.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {counterfactual.motifs.map((motif, index) => (
                    <MotifChip key={index} motif={motif} />
                  ))}
                </div>
              )}
            </div>
          )}

          {(explanation.text || explanation.status !== 'idle') && (
            <div className="flex w-full flex-col items-start gap-2">
              <SectionLabel>
                {t('explain.title')}
                {explanation.status === 'streaming' && ` · ${t('explain.streaming')}`}
                {explanation.status === 'done' && explanation.cached && ` · ${t('explain.cached')}`}
                {explanation.model && ` · ${explanation.model}`}
              </SectionLabel>
              <MoveProse
                text={explanation.text}
                frames={frames}
                streaming={explanation.status === 'streaming'}
                onHover={onHoverSan}
              >
                {explanation.status === 'streaming' && <Cursor />}
              </MoveProse>
              {explanation.status === 'not-analyzed' && (
                <p className="text-sm text-muted-foreground">{t('explain.notAnalyzed')}</p>
              )}
              {explanation.status === 'error' && (
                <p className="text-sm text-destructive">
                  {t('explain.failed', { message: explanation.error ?? '' })}
                </p>
              )}
              {explanation.status === 'done' && (
                /* Pulled left by its own padding so the label sits on the text
                   column's edge rather than a button's-worth inside it. A ghost
                   button has no frame to line up, so what the eye lines up is
                   the first letter. */
                <Button variant="ghost" size="sm" className="-ml-3" onClick={explanation.request}>
                  {t('explain.regenerate')}
                </Button>
              )}
            </div>
          )}

          {explanation.status === 'idle' && played && (
            <Button variant="outline" size="sm" onClick={explanation.request}>
              {t('explain.request')}
            </Button>
          )}

          {analysis && analysis.candidates.length > 0 && (
            <div className="flex w-full flex-col gap-2">
              <SectionLabel>{t('analysis.candidates')}</SectionLabel>
              <div className="-mx-1 flex flex-col gap-0.5" onMouseLeave={() => onPreview(null)}>
                {analysis.candidates.map((candidate, index) => (
                  <CandidateRow
                    key={candidate.uci || candidate.san}
                    candidate={candidate}
                    rank={index}
                    sideToMove={sideToMove}
                    onPlay={onPlaySan}
                    onPreview={onPreview}
                  />
                ))}
              </div>
            </div>
          )}

          {sessionId && nodeId !== null && (
            <AskBox
              sessionId={sessionId}
              nodeId={nodeId}
              lang={lang}
              frames={frames}
              onHoverSan={onHoverSan}
            />
          )}
        </div>
      </ScrollArea>
    </Card>
  );
}

/**
 * The heading over a section of the column. Muted, small, sentence case — the
 * hand-written version was uppercase micro-caps with letter-spacing, which is a
 * different design system's mannerism and looked like one next to shadcn's own
 * headings.
 */
function SectionLabel({ children }: { children: React.ReactNode }): React.JSX.Element {
  return <div className="text-xs font-medium text-muted-foreground">{children}</div>;
}

/** A number inside a sentence: figures line up, prose does not shift. */
function Figure({ children }: { children: React.ReactNode }): React.JSX.Element {
  return <b className="font-mono font-semibold text-foreground tabular-nums">{children}</b>;
}

/**
 * Written text with the notation in it made hoverable, so that pointing at
 * `Qxg3` draws it on the board.
 *
 * Which spans count is decided by `ui/sanMentions.ts` — including, and this is
 * the part worth reading before changing anything here, whether each one is a
 * *legal* move in a position the text could be quoting from, and so whether it
 * is drawn as that move or as the square it names. Everything else stays
 * exactly the text it was.
 *
 * Both kinds are the same affordance here: this component knows only that a
 * span is worth pointing at, and the board decides what the answer looks like.
 *
 * The affordance is a dotted underline, and it is gated on a fine pointer,
 * because on a touch screen it would advertise something that cannot be done.
 * Nothing is lost there: no move is only reachable through this, the board and
 * the candidate list are unchanged, and the text still reads as text.
 *
 * Hover, not click. This is a lookup — "where is that" — and the answer is an
 * arrow that disappears when the pointer leaves. Playing the move, or
 * navigating to it, is what the move list and the candidate rows are for, and
 * a paragraph of prose is the wrong place to put a dozen tab stops and a dozen
 * ways to lose your position in the game.
 *
 * Rendering is a flat list of strings and spans inside one paragraph, so the
 * text stays one continuous run for selection and copying, and
 * `whitespace-pre-wrap` still governs the whole of it.
 */
function MoveProse({
  text,
  frames,
  streaming,
  onHover,
  children,
}: {
  text: string;
  frames: readonly SanFrame[];
  streaming: boolean;
  onHover: (mention: SanMention | null) => void;
  children?: React.ReactNode;
}): React.JSX.Element {
  const mentions = useMemo(
    () => findSanMentionsIn(text, frames, { streaming }),
    [text, frames, streaming],
  );

  const parts: React.ReactNode[] = [];
  let cursor = 0;
  for (const mention of mentions) {
    if (mention.start > cursor) parts.push(text.slice(cursor, mention.start));
    parts.push(
      <span
        key={mention.start}
        className="rounded-[3px] transition-colors hover:bg-primary/15 pointer-fine:cursor-help pointer-fine:underline pointer-fine:decoration-muted-foreground pointer-fine:decoration-dotted pointer-fine:underline-offset-[3px]"
        onMouseEnter={() => onHover(mention)}
        onMouseLeave={() => onHover(null)}
      >
        {mention.text}
      </span>,
    );
    cursor = mention.end;
  }
  parts.push(text.slice(cursor));

  return (
    /*
      The paragraph clears the hover as well as each span, because a span can
      stop existing under a stationary pointer: the text re-flows as it streams,
      and a token that moves out from under the cursor never fires its own
      `mouseleave`.
    */
    <p
      className="text-sm leading-relaxed whitespace-pre-wrap"
      onMouseLeave={() => onHover(null)}
    >
      {parts}
      {children}
    </p>
  );
}

/** The block that follows streaming text, so a pause reads as "still writing". */
function Cursor(): React.JSX.Element {
  return (
    <span className="ml-0.5 inline-block h-[1.05em] w-1.5 translate-y-[0.15em] animate-caret-blink bg-primary" />
  );
}

/**
 * Pointing at a candidate draws it on the board; clicking still plays it. That
 * split is what a board tool trains people to expect, and it is where the
 * arrows earn their keep — reading "Nd7 c4 Ngf6" is work, seeing it is not.
 * Focus mirrors hover so the keyboard gets the same preview.
 *
 * A four-column grid rather than a `Button`, because the columns have to line
 * up down the list: rank, move, score and line each get a fixed measure so the
 * scores form a column you can read straight down. A row of flex-laid-out
 * buttons cannot promise that.
 */
function CandidateRow({
  candidate,
  rank,
  sideToMove,
  onPlay,
  onPreview,
}: {
  candidate: Candidate;
  rank: number;
  sideToMove: 'white' | 'black';
  onPlay: (san: string) => void;
  onPreview: (candidate: Candidate | null) => void;
}) {
  return (
    <button
      className="grid w-full grid-cols-[1.25rem_3.5rem_3.5rem_1fr] items-baseline gap-2 rounded-2xl px-2 py-1.5 text-left text-xs transition-colors outline-none hover:bg-muted focus-visible:ring-[3px] focus-visible:ring-ring/40"
      onClick={() => onPlay(candidate.san)}
      onMouseEnter={() => onPreview(candidate)}
      onFocus={() => onPreview(candidate)}
      onBlur={() => onPreview(null)}
      title={candidate.pv.join(' ')}
    >
      <span className="font-mono text-muted-foreground/70 tabular-nums">{rank + 1}</span>
      <span className="font-mono text-[0.8125rem] font-semibold">{candidate.san}</span>
      <span className="font-mono text-muted-foreground tabular-nums">
        {formatScore(candidate.score, 'white', sideToMove)}
      </span>
      <span className="truncate font-mono text-muted-foreground/70">
        {candidate.pv.slice(1).join(' ')}
      </span>
    </button>
  );
}

function MotifChip({ motif }: { motif: Motif }): React.JSX.Element {
  const label = t(`motif.${motif.kind}` as const);
  let detail = '';
  switch (motif.kind) {
    case 'fork':
      detail = `${motif.attacker} → ${motif.targets.join(', ')}`;
      break;
    case 'pin':
      detail = `${motif.attacker} → ${motif.pinned} / ${motif.behind}`;
      break;
    case 'skewer':
      detail = `${motif.attacker} → ${motif.front} / ${motif.behind}`;
      break;
    case 'discovered_attack':
      detail = `${motif.revealed} → ${motif.targets.join(', ')}`;
      break;
    case 'hanging':
      detail = `${motif.role} ${motif.square}`;
      break;
    case 'back_rank':
      detail = `${motif.attacker} → ${motif.king}`;
      break;
  }
  return (
    <Badge variant="outline" className="gap-1.5 bg-background">
      <span className="font-medium">{label}</span>
      <span className="font-mono text-muted-foreground">{detail}</span>
    </Badge>
  );
}

/**
 * Follow-up questions (`POST /ask`, API.md Phase 3) — the same stream shape,
 * about the same position, so the answer's moves are hoverable on the same
 * terms as the explanation's.
 */
function AskBox({
  sessionId,
  nodeId,
  lang,
  frames,
  onHoverSan,
}: {
  sessionId: string;
  nodeId: number;
  lang: string;
  frames: readonly SanFrame[];
  onHoverSan: (mention: SanMention | null) => void;
}): React.JSX.Element {
  const [question, setQuestion] = useState('');
  const [answer, setAnswer] = useState('');
  const [busy, setBusy] = useState(false);
  const controllerRef = useRef<AbortController | null>(null);

  const submit = useCallback(
    (event: React.FormEvent) => {
      event.preventDefault();
      const text = question.trim();
      if (!text) return;
      controllerRef.current?.abort();
      const controller = new AbortController();
      controllerRef.current = controller;
      setAnswer('');
      setBusy(true);
      ask(
        sessionId,
        { node_id: nodeId, question: text, lang },
        {
          onDelta: (chunk) => setAnswer((current) => current + chunk),
          onDone: (event_) => {
            setAnswer(event_.text);
            setBusy(false);
          },
          onError: (message) => {
            setAnswer(message);
            setBusy(false);
          },
        },
        controller.signal,
      ).catch(() => setBusy(false));
    },
    [question, sessionId, nodeId, lang],
  );

  return (
    <div className="flex w-full flex-col gap-2">
      <SectionLabel>{t('ask.title')}</SectionLabel>
      <form className="flex gap-2" onSubmit={submit}>
        <Input
          type="text"
          value={question}
          placeholder={t('ask.placeholder')}
          // The section heading above is a `div`, not a `<label>`, because it
          // labels the whole block rather than the field — so the field says
          // its own name, out of the same key, rather than borrowing one.
          aria-label={t('ask.title')}
          onChange={(event) => setQuestion(event.target.value)}
          className="flex-1"
        />
        <Button type="submit" disabled={busy || !question.trim()}>
          {busy ? t('ask.busy') : t('ask.submit')}
        </Button>
      </form>
      {answer && (
        <MoveProse text={answer} frames={frames} streaming={busy} onHover={onHoverSan}>
          {busy && <Cursor />}
        </MoveProse>
      )}
    </div>
  );
}
