import { useCallback, useRef, useState } from 'react';
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
  onPlaySan: (san: string) => void;
  /** Draw this candidate's line on the board; `null` clears the preview. */
  onPreview: (candidate: Candidate | null) => void;
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
  onPlaySan,
  onPreview,
  onAnalyze,
}: AnalysisPanelProps): React.JSX.Element {
  const played = analysis?.context?.played ?? null;
  const counterfactual = analysis?.context?.counterfactual ?? null;
  const sideToMove = analysis ? turnColor(analysis.fen) : 'white';

  return (
    <Card size="sm" className="flex min-h-0 flex-col gap-0 py-0">
      <CardHeader className="border-b py-3">
        <CardTitle className="text-sm">{t('analysis.title')}</CardTitle>
        {analysis && (
          <CardAction className="self-center font-mono text-xs text-muted-foreground">
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

          {!analysis && status !== 'loading' && (
            <div className="flex flex-col items-start gap-3">
              <p className="text-sm text-muted-foreground">{t('analysis.idle')}</p>
              <Button variant="outline" size="sm" onClick={onAnalyze}>
                {t('analysis.analyzeNow')}
              </Button>
            </div>
          )}

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
              <p className="text-sm leading-relaxed whitespace-pre-wrap">
                {explanation.text}
                {explanation.status === 'streaming' && <Cursor />}
              </p>
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
            <AskBox sessionId={sessionId} nodeId={nodeId} lang={lang} />
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

/** Follow-up questions (`POST /ask`, API.md Phase 3) — the same stream shape. */
function AskBox({
  sessionId,
  nodeId,
  lang,
}: {
  sessionId: string;
  nodeId: number;
  lang: string;
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
        <p className="text-sm leading-relaxed whitespace-pre-wrap">
          {answer}
          {busy && <Cursor />}
        </p>
      )}
    </div>
  );
}
