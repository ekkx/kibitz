import { useState } from 'react';
import { Check, Globe } from 'lucide-react';
import type { LanguageOption } from '../api/types.ts';
import { Button } from './ui/button.tsx';
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover.tsx';
import { cn } from '@/lib/utils';
import { t } from '../i18n/index.ts';

export interface LanguageMenuProps {
  /** The languages that have both a server catalogue and a UI one. */
  options: LanguageOption[];
  language: string;
  onChange: (code: string) => void;
}

/**
 * The language control, in the topbar rather than inside Settings.
 *
 * It was one of four groups in the settings popover, which put the one control
 * on the page whose *current value* is worth seeing at a glance behind a click
 * on an unlabelled gear. It is also not really a preference in the same sense
 * as the others: the arrow count and the depth change how the app behaves,
 * whereas this changes what every word on the screen says, including the words
 * inside the panel you would have to open to change it back.
 *
 * **The trigger is a globe.** It read the active endonym — 日本語, English — on
 * the argument that the useful thing to know without clicking is which language
 * is *on*. That argument loses to a simpler one: you already know which language
 * is on, because you are reading the interface in it, and every other control in
 * this bar is an icon of fixed width. A word-labelled button among them is the
 * one thing whose size changes when the language does, and it was answering a
 * question nobody in front of the screen has. The globe says "language" the way
 * the gear says "settings", in the one symbol that needs no alphabet at all.
 *
 * The name that was on the button has not been lost, only moved to where it is
 * actually needed: the menu lists every language in its own endonym and marks
 * the active one with a check and a heavier weight, so opening the control still
 * tells you where you are. The accessible name is the word "Language" in the
 * active interface language, since an icon-only button announces nothing on its
 * own.
 *
 * A menu rather than a two-way toggle, because two languages is where this
 * happens to be rather than what it is: `useLanguage` offers the intersection
 * of the server's list and the UI catalogues, and both grow. Dismissal —
 * Escape, click outside, focus return — is Base UI's, exactly as in the
 * settings popover, so the two behave identically.
 *
 * Endonyms come from the server and are never translated (`state/useLanguage.ts`).
 * A picker that renders its options in a language you cannot read is useless to
 * the person who needs it.
 */
export function LanguageMenu({ options, language, onChange }: LanguageMenuProps): React.JSX.Element {
  // Controlled, for one reason: choosing an option has to close the menu, and
  // an uncontrolled popover has no way to be told that from inside its content.
  const [open, setOpen] = useState(false);
  const active = options.find((option) => option.code === language);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            /* The tooltip carries what the label used to: "Language" alone would
               only restate the glyph, so it names the language that is on. */
            title={active ? `${t('lang.label')} — ${active.name}` : t('lang.label')}
            aria-label={t('lang.label')}
          >
            <Globe />
          </Button>
        }
      />

      <PopoverContent
        align="end"
        sideOffset={8}
        className="w-44 gap-0.5 p-1.5"
        aria-label={t('lang.label')}
      >
        {options.map((option) => (
          <button
            key={option.code}
            type="button"
            /* A row, not a `Button`: these are a list of alternatives one of
               which is already true, so the chosen one carries a mark rather
               than a filled surface, and the whole row is the target. */
            className={cn(
              'flex items-center justify-between gap-2 rounded-2xl px-3 py-2 text-left text-sm transition-colors outline-none hover:bg-muted focus-visible:ring-[3px] focus-visible:ring-ring/40',
              option.code === language && 'font-medium',
            )}
            aria-current={option.code === language}
            onClick={() => {
              onChange(option.code);
              setOpen(false);
            }}
          >
            <span className="truncate">{option.name}</span>
            {option.code === language && <Check className="size-4 shrink-0 text-primary" />}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  );
}
