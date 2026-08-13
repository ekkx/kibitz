import { Fragment, createElement, type ReactNode } from 'react';
import { en, type UiMessageKey } from './en.ts';
import { ja } from './ja.ts';

/**
 * A deliberately thin i18n layer.
 *
 * Internally there are still two notions of "language", and they are still not
 * the same thing:
 *
 *   1. **UI language** — the language of the interface itself, handled here by
 *      swapping the catalogue `t()` reads from. No component contains a literal
 *      string, so a locale is a catalogue and nothing else.
 *   2. **Explanation language** — the `lang` parameter sent to `/explain` and
 *      `/ask`, which the server writes the explanation in. It lives in
 *      `state/useLanguages.ts`.
 *
 * The *user* sets one thing. `state/useLanguages.ts` owns that single choice
 * and drives both: an interface in one language explaining moves in another is
 * a combination nobody asked for, and offering it made the setting harder to
 * understand than the feature was worth.
 *
 * The active catalogue is module state rather than context because `t()` is
 * called from plain functions as well as components (`ui/format.ts`), and
 * because every string in the app is rendered under the one `App` that owns the
 * language state — a change re-renders all of them.
 */

export type UiLanguage = 'en' | 'ja';

const catalogues: Record<UiLanguage, Partial<Record<UiMessageKey, string>>> = { en, ja };

/** The languages the interface itself can be shown in. */
export const UI_LANGUAGES = Object.keys(catalogues) as UiLanguage[];

export const isUiLanguage = (code: string): code is UiLanguage => code in catalogues;

let active: UiLanguage = 'en';

/**
 * Switch the interface language.
 *
 * Called during render, before the strings of that render are read — an effect
 * would paint one frame of the old language first. It is idempotent, so
 * StrictMode's double render costs nothing.
 */
export function setUiLanguage(next: UiLanguage): void {
  active = next;
}

export const uiLanguage = (): UiLanguage => active;

export type TranslateParams = Record<string, string | number>;

/** `t('eval.mateIn', { n: 3 })` → "M3". Missing keys fall back to English. */
export function t(key: UiMessageKey, params?: TranslateParams): string {
  const template = catalogues[active]?.[key] ?? en[key] ?? key;
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/**
 * `t` for a template whose placeholders are elements rather than text.
 *
 * This exists for word order. "3 blunders" puts the number first and Japanese
 * puts it last (「大悪手 3」), so a component that renders `{count} {label}`
 * itself is stuck in English order however well its strings are translated. The
 * template decides the order; the caller still owns the markup around each
 * placeholder.
 */
export function tParts(key: UiMessageKey, params: Record<string, ReactNode>): ReactNode[] {
  const template = catalogues[active]?.[key] ?? en[key] ?? key;
  return template.split(/(\{\w+\})/).map((piece, index) => {
    const name = /^\{(\w+)\}$/.exec(piece)?.[1];
    const value = name !== undefined && name in params ? params[name] : piece;
    return createElement(Fragment, { key: index }, value);
  });
}

export type { UiMessageKey };
