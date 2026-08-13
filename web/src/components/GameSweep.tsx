import type { SweepState } from '../state/useSweep.ts';
import type { Classification } from '../api/types.ts';
import { Button } from './ui/button.tsx';
import { Progress, ProgressLabel } from './ui/progress.tsx';
import { CLASSIFICATION_GLYPH, classificationColor, classificationLabel } from '../ui/format.ts';
import { t, tParts } from '../i18n/index.ts';

/**
 * The `POST /analyze-game` control: run, progress, cancel.
 *
 * While it runs, the count is the progress bar's own `ProgressLabel` rather
 * than a `<span>` beside it — so the number and the bar are one labelled
 * control to a screen reader instead of two unrelated things that happen to sit
 * next to each other. The two overrides on the track are because shadcn's
 * default progress is a full-width block meant for a form, and this one lives
 * in a topbar next to 9px of chrome.
 */
export function GameSweepButton({ sweep }: { sweep: SweepState }): React.JSX.Element {
  if (!sweep.running) {
    return (
      <Button variant="outline" size="sm" onClick={sweep.run}>
        {t('sweep.run')}
      </Button>
    );
  }
  const percent = sweep.total > 0 ? (sweep.done / sweep.total) * 100 : 0;
  return (
    <div className="flex items-center gap-2">
      <Progress
        value={percent}
        className="w-fit flex-nowrap items-center gap-2 *:data-[slot=progress-track]:h-1.5 *:data-[slot=progress-track]:w-24"
      >
        <ProgressLabel className="text-xs font-normal text-muted-foreground tabular-nums">
          {t('sweep.running', { done: sweep.done, total: sweep.total })}
        </ProgressLabel>
      </Progress>
      <Button variant="ghost" size="sm" onClick={sweep.cancel}>
        {t('sweep.cancel')}
      </Button>
    </div>
  );
}

const SUMMARY_ORDER: Classification[] = [
  'great',
  'best',
  'excellent',
  'good',
  'inaccuracy',
  'mistake',
  'blunder',
  'miss',
  'book',
];

/**
 * Accuracy summary shown under the move list once a sweep finishes.
 *
 * It is the footer of the moves card, so it carries the card's own top border
 * and padding rather than being a panel of its own — the summary is *about* the
 * list above it, and boxing it separately would say otherwise.
 */
export function SweepSummary({ sweep }: { sweep: SweepState }): React.JSX.Element | null {
  if (sweep.error) {
    return (
      <div className="border-t px-4 py-3 text-sm text-destructive">
        {t('sweep.failed', { message: sweep.error })}
      </div>
    );
  }
  if (!sweep.summary) return null;
  const { whiteAccuracy, blackAccuracy, counts } = sweep.summary;

  return (
    <div className="flex flex-wrap items-end gap-x-6 gap-y-3 border-t px-4 py-3">
      <Accuracy value={whiteAccuracy} color={t('eval.forWhite')} />
      <Accuracy value={blackAccuracy} color={t('eval.forBlack')} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          {SUMMARY_ORDER.filter((classification) => counts[classification]).map(
            (classification) => (
              <span className="text-xs text-muted-foreground" key={classification}>
                {/* "3 Blunder??" in English, 「大悪手?? 3」 in Japanese: the
                    catalogue owns the order, this owns the colour. */}
                {tParts('sweep.count', {
                  n: (
                    <b
                      className="font-mono font-bold"
                      style={{ color: classificationColor(classification) }}
                    >
                      {counts[classification]}
                    </b>
                  ),
                  label: `${classificationLabel(classification)}${CLASSIFICATION_GLYPH[classification]}`,
                })}
              </span>
            ),
          )}
        </div>
        <span className="text-xs text-muted-foreground">{t('sweep.summaryTitle')}</span>
      </div>
    </div>
  );
}

function Accuracy({ value, color }: { value: number; color: string }): React.JSX.Element {
  return (
    <div className="flex flex-col">
      <span className="font-mono text-lg leading-tight font-semibold tabular-nums">
        {value.toFixed(1)}
      </span>
      <span className="text-xs text-muted-foreground">
        {t('sweep.accuracyFor', { color })}
      </span>
    </div>
  );
}
