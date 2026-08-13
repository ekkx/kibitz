import type { OpeningInfo } from '../api/types.ts';
import { t } from '../i18n/index.ts';

export interface OpeningCaptionProps {
  opening: OpeningInfo | null;
}

/**
 * The name of the selected position's opening, as a caption above the move
 * list — an ECO code and a name, nothing else.
 *
 * Deliberately small and quiet: it labels the game, it does not comment on it.
 * With no name to show it renders nothing at all rather than an empty slot,
 * because "no entry in the ECO table" is not a fact worth a line of the layout,
 * and least of all a statement that the game has left theory.
 *
 * That quietness is why the ECO code is *not* a `Badge`, which is the component
 * it otherwise asks for: a badge is a thing you are meant to notice, and this
 * whole row is designed not to be. It is the caption's own text, set in the
 * mono face the rest of the notation uses and one step further muted.
 */
export function OpeningCaption({ opening }: OpeningCaptionProps): React.JSX.Element | null {
  if (!opening) return null;

  return (
    <div
      className="flex min-w-0 items-baseline gap-2 border-b px-4 py-2 text-xs text-muted-foreground"
      aria-label={t('opening.label')}
    >
      <span
        className="shrink-0 font-mono text-[0.6875rem] tracking-wide text-muted-foreground/70"
        title={t('opening.eco', { eco: opening.eco })}
      >
        {opening.eco}
      </span>
      <span className="truncate" title={opening.name}>
        {opening.name}
      </span>
    </div>
  );
}
