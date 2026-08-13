import { useCallback, useEffect, useState } from 'react';
import { getLanguages } from '../api/client.ts';
import type { LanguageOption } from '../api/types.ts';
import { isUiLanguage, setUiLanguage, type UiLanguage } from '../i18n/index.ts';

const STORAGE_KEY = 'kibitz.language';

/** What the app falls back to before the server has answered, and if it never does. */
const FALLBACK: UiLanguage = 'en';

/**
 * The language setting — one choice, two effects.
 *
 * The user sets one thing, so there is one stored value. It drives the
 * interface catalogue (`i18n`) and travels as the `lang` parameter on
 * `/explain` and `/ask`, which is what the server writes the explanation in.
 * The two remain separate ideas in the code — one is a catalogue lookup, the
 * other a request parameter, and API.md is explicit that `lang` is per-request
 * — but they are never set apart from each other.
 *
 * Only languages that are in *both* lists are offered: the server's
 * `GET /api/languages` (asking for anything else is a 400) and the UI
 * catalogues (a language the interface cannot speak would silently fall back to
 * English text around a Japanese explanation, which is the split this setting
 * exists to remove).
 *
 * Language names are endonyms straight from the server — "English", "日本語" —
 * and are never translated. A picker that renders its own options in a language
 * you cannot read is useless to the person who needs it.
 */
export interface LanguageState {
  /** The languages that can actually be offered, server order. */
  options: LanguageOption[];
  language: UiLanguage;
  setLanguage: (code: string) => void;
}

/** Used until the server answers, so the setting is never an empty select. */
const OFFLINE_OPTIONS: LanguageOption[] = [
  { code: 'en', name: 'English' },
  { code: 'ja', name: '日本語' },
];

export function useLanguage(): LanguageState {
  const [options, setOptions] = useState<LanguageOption[]>(OFFLINE_OPTIONS);
  const [language, setStored] = useState<UiLanguage>(readStored);

  // Read during render, not in an effect: `t()` is called while this render's
  // children run, and an effect would paint one frame in the old language.
  setUiLanguage(language);

  // The document's own language, for screen readers, hyphenation and spell
  // checking — the one piece of the page that is not rendered by React.
  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const setLanguage = useCallback((code: string) => {
    if (!isUiLanguage(code)) return;
    setStored(code);
    try {
      localStorage.setItem(STORAGE_KEY, code);
    } catch {
      /* private mode: keep it in memory only */
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    getLanguages()
      .then((response) => {
        if (cancelled) return;
        const offered = response.languages.filter((entry) => isUiLanguage(entry.code));
        if (offered.length > 0) setOptions(offered);
        // A stored choice this server does not support would 400 on every
        // explanation, so fall back to the server's default when it can.
        const stored = readStored();
        const supported = offered.some((entry) => entry.code === stored);
        if (!supported && isUiLanguage(response.default)) setLanguage(response.default);
      })
      .catch(() => {
        /* offline: the built-in options stand, and health.ts says so in words */
      });
    return () => {
      cancelled = true;
    };
  }, [setLanguage]);

  return { options, language, setLanguage };
}

/**
 * Anything stored that is not a language the interface can speak is discarded
 * rather than kept: it is a value from a build where this key meant only the
 * explanation language, and honouring it would leave the UI in a language that
 * has no catalogue.
 */
function readStored(): UiLanguage {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored !== null && isUiLanguage(stored) ? stored : FALLBACK;
  } catch {
    return FALLBACK;
  }
}

export type { UiLanguage };
