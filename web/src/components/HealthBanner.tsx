import { TriangleAlert } from 'lucide-react';
import { Alert, AlertAction, AlertDescription } from './ui/alert.tsx';
import { Badge } from './ui/badge.tsx';
import { Button } from './ui/button.tsx';
import { cn } from '@/lib/utils';
import type { HealthState } from '../state/useHealth.ts';
import { t } from '../i18n/index.ts';

/**
 * "If the engine did not start, say so plainly." A missing engine is not an
 * error the user can debug from a failed request, so it gets said in words,
 * with the fix, before anything else is attempted.
 *
 * An `Alert`, not a bespoke banner: it is exactly the shape shadcn's alert has
 * — icon, prose, one action — and the destructive variant already carries the
 * "this is wrong" colour. The only thing added is a border in the same red,
 * because the variant tints the text and leaves the frame neutral, and a full-
 * width neutral box does not read as an alarm from across the room.
 */
export function HealthBanner({
  health,
  onRetry,
}: {
  health: HealthState;
  onRetry: () => void;
}): React.JSX.Element | null {
  if (health.status === 'ok' || health.status === 'checking') return null;
  const message = health.status === 'no-engine' ? t('health.noEngine') : t('health.unreachable');
  return (
    <Alert variant="destructive" className="mx-4 mt-4 w-auto border-destructive/30">
      <TriangleAlert />
      <AlertDescription>{message}</AlertDescription>
      <AlertAction>
        <Button variant="outline" size="sm" onClick={onRetry}>
          {t('health.retry')}
        </Button>
      </AlertAction>
    </Alert>
  );
}

/**
 * Engine status in the topbar, as a badge with a status dot.
 *
 * The dot rather than an icon: this is ambient, not something to be read on
 * every glance, and a coloured dot is the smallest mark that still says
 * "running" or "not running" without competing with the game headers next to
 * it. The full sentence stays in the `title`, where it always was.
 */
export function HealthChip({ health }: { health: HealthState }): React.JSX.Element {
  const ok = health.status === 'ok';
  const text =
    health.status === 'ok'
      ? t('health.ok', { engine: health.engine })
      : health.status === 'checking'
        ? t('health.checking')
        : health.status === 'no-engine'
          ? t('health.engineMissing')
          : t('health.offline');
  return (
    <Badge variant={ok ? 'outline' : 'destructive'} title={text} className="gap-1.5">
      <span
        className={cn('size-1.5 shrink-0 rounded-full', ok ? 'bg-primary' : 'bg-destructive')}
      />
      {text}
    </Badge>
  );
}
