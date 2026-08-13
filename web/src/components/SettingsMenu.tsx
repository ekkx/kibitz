import { Settings2 } from 'lucide-react';
import type { LanguageOption } from '../api/types.ts';
import { Button } from './ui/button.tsx';
import { Label } from './ui/label.tsx';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover.tsx';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './ui/select.tsx';
import { ToggleGroup, ToggleGroupItem } from './ui/toggle-group.tsx';
import { ARROW_COUNTS, DEPTHS, type Depth, type SettingsState } from '../state/useSettings.ts';
import { t } from '../i18n/index.ts';

export interface SettingsMenuProps {
  settings: SettingsState;
  /** The language picker — interface and explanations both — folded in here. */
  languages: LanguageOption[];
  language: string;
  onLanguageChange: (code: string) => void;
}

/**
 * The settings surface: a popover off the topbar, not a preferences dialog.
 *
 * Everything in it is a single control with an immediate effect, so there is no
 * Save and no Cancel — the arrow count redraws the board as it changes, the
 * language switches the interface under the cursor, and the depth is simply the
 * number the next request carries.
 *
 * Open/closed state, Escape, click-outside, focus return and the trigger's
 * `aria-expanded` are all Base UI's now. The component used to own about thirty
 * lines of `useState` plus two window listeners doing exactly that, and every
 * one of them was a chance to get a detail wrong that a popover primitive gets
 * right by construction — the listeners were, for instance, `mousedown` only,
 * so a touch outside never dismissed it.
 */
export function SettingsMenu({
  settings,
  languages,
  language,
  onLanguageChange,
}: SettingsMenuProps): React.JSX.Element {
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="icon-sm" title={t('settings.open')} aria-label={t('settings.open')}>
            <Settings2 />
          </Button>
        }
      />

      <PopoverContent align="end" sideOffset={8} className="w-80 gap-5" aria-label={t('settings.title')}>
        <Group label={t('settings.arrows.label')} hint={t('settings.arrows.hint')}>
          <ToggleGroup
            {...SEGMENTED}
            aria-label={t('settings.arrows.label')}
            value={[String(settings.arrowCount)]}
            onValueChange={(value) => {
              // Pressing the pressed item would otherwise clear the group. This
              // is a choice among alternatives, not a set of independent
              // switches, so "none selected" is not a state it can reach.
              const next = value[0];
              if (next !== undefined) settings.setArrowCount(Number(next));
            }}
          >
            {ARROW_COUNTS.map((count) => (
              <ToggleGroupItem key={count} value={String(count)} className={SEGMENT}>
                {count === 0 ? t('settings.arrows.none') : count}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Group>

        {/* Next to the arrows rather than next to the depth: both of these are
            about what the board does while you look at it, whereas depth is
            about what the server is asked for. */}
        <Group label={t('settings.sound.label')} hint={t('settings.sound.hint')}>
          <ToggleGroup
            {...SEGMENTED}
            aria-label={t('settings.sound.label')}
            value={[settings.sound ? 'on' : 'off']}
            onValueChange={(value) => {
              const next = value[0];
              if (next !== undefined) settings.setSound(next === 'on');
            }}
          >
            <ToggleGroupItem value="on" className={SEGMENT}>
              {t('settings.sound.on')}
            </ToggleGroupItem>
            <ToggleGroupItem value="off" className={SEGMENT}>
              {t('settings.sound.off')}
            </ToggleGroupItem>
          </ToggleGroup>
        </Group>

        <Group label={t('settings.depth.label')} hint={t('settings.depth.hint')}>
          <Select
            value={settings.depth}
            onValueChange={(value) => {
              if (typeof value === 'number') settings.setDepth(value);
            }}
          >
            <SelectTrigger className="w-full" aria-label={t('settings.depth.label')}>
              {/*
                The trigger's label is computed here rather than left to the
                primitive to read off the selected item. The items live in a
                portal that does not exist until the popup is first opened, so
                the automatic version renders an empty trigger until then.
              */}
              <SelectValue>
                {(depth: Depth) =>
                  t('settings.depth.option', { label: depthLabel(depth), n: depth })
                }
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              {DEPTHS.map((depth) => (
                <SelectItem key={depth} value={depth}>
                  {t('settings.depth.option', { label: depthLabel(depth), n: depth })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Group>

        {languages.length > 0 && (
          <Group label={t('lang.label')} hint={t('lang.hint')}>
            <Select
              value={language}
              onValueChange={(value) => {
                if (typeof value === 'string') onLanguageChange(value);
              }}
            >
              <SelectTrigger className="w-full" aria-label={t('lang.label')}>
                <SelectValue>
                  {(code: string) =>
                    languages.find((option) => option.code === code)?.name ?? code
                  }
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {/* Endonyms from the server: never translated, never localised. */}
                {languages.map((option) => (
                  <SelectItem key={option.code} value={option.code}>
                    {option.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Group>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * The segmented control, twice: pick one of N, where N is small and every
 * option fits on screen at once.
 *
 * This is `ToggleGroup` restyled to look like `TabsList` — a sunken track with
 * the chosen option raised out of it on the page's own background — and the
 * restyling is the point. shadcn's toggle group presses to `bg-muted`, which is
 * correct for a text-formatting toolbar, where "bold is on" is a *state* of
 * something you are looking at elsewhere. Here it is the answer to a question,
 * and on the popover's surface a muted press is a tint so faint that the arrow
 * count genuinely could not be read off it. The import screen already asks a
 * pick-one question and answers it with `Tabs`, so this borrows that answer
 * rather than inventing a third one: two controls, two components, one look.
 */
const SEGMENTED = {
  spacing: 1,
  size: 'sm',
  className: 'w-full rounded-full bg-muted p-1',
} as const;

const SEGMENT =
  'flex-1 rounded-full font-mono hover:bg-background/60 aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm';

/** Label, control, hint — the same three rows for every setting in the panel. */
function Group({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: React.ReactNode;
}): React.JSX.Element {
  return (
    <div className="flex flex-col gap-2">
      <Label>{label}</Label>
      {children}
      <p className="text-xs leading-relaxed text-muted-foreground">{hint}</p>
    </div>
  );
}

/** "Balanced" rather than "12" — the ply count rides along in the option text. */
const depthLabel = (depth: Depth): string => t(`settings.depth.${depth}` as const);
