import type { Score } from '../api/types.ts';
import type { Color } from '../chess/rules.ts';
import { formatScore, whiteWinProb } from '../ui/format.ts';

export interface EvalBarProps {
  score: Score | null;
  /** Win probability of the side to move, as the API reports it. */
  winProb: number | null;
  sideToMove: Color;
  orientation: Color;
}

/**
 * The eval bar. Everything the API gives is from the side to move's point of
 * view; the bar is absolute, so both get converted to White's point of view
 * once, here, and nowhere else.
 */
export function EvalBar({ score, winProb, sideToMove, orientation }: EvalBarProps): React.JSX.Element {
  const share = winProb === null ? 0.5 : whiteWinProb(winProb, sideToMove);
  const whiteAtBottom = orientation === 'white';
  const label = score ? formatScore(score, 'white', sideToMove) : '—';

  return (
    <div className="evalbar" title={label}>
      <div
        className="evalbar__white"
        style={{
          height: `${Math.max(0, Math.min(1, share)) * 100}%`,
          ...(whiteAtBottom ? { bottom: 0, top: 'auto' } : { top: 0, bottom: 'auto' }),
        }}
      />
      <div className="evalbar__mid" />
      <div className={`evalbar__label evalbar__label--${whiteAtBottom ? 'bottom' : 'top'}`}>
        {label}
      </div>
    </div>
  );
}
