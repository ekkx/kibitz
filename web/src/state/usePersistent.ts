import { useCallback, useEffect, useState } from 'react';

/** `useState` backed by localStorage, so preferences survive a reload. */
export function usePersistent<T extends string>(
  key: string,
  fallback: T,
): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      return (localStorage.getItem(key) as T | null) ?? fallback;
    } catch {
      return fallback;
    }
  });

  const set = useCallback(
    (next: T) => {
      setValue(next);
      try {
        localStorage.setItem(key, next);
      } catch {
        /* private mode: keep it in memory only */
      }
    },
    [key],
  );

  return [value, set];
}

export type ThemeChoice = 'system' | 'light' | 'dark';

/**
 * The theme, as one attribute: `data-theme="dark"` or `data-theme="light"` on
 * `<html>`.
 *
 * Dark is the default for anyone with nothing stored. This is an analysis
 * surface people sit in front of for an hour at a time, and the palette is tuned
 * for that case first (see the milk-coffee tokens in `index.css`).
 *
 * `'system'` survives as a stored value from before dark became the default, and
 * it is resolved *here* — the attribute is always written and always concrete,
 * never removed. That matters because two stylesheets now read the theme:
 * `styles.css` (which also has a `prefers-color-scheme` fallback for the absent
 * attribute) and Tailwind's `dark:` variant, which is redefined in `index.css`
 * as `[data-theme='dark']`. A variant cannot express "…or the OS says dark and
 * nothing is set", so keeping the attribute concrete is what lets a single
 * selector describe the theme everywhere, with no class to keep in sync and no
 * chance of the two stylesheets disagreeing.
 */
export function useTheme(): [ThemeChoice, () => void] {
  const [theme, setTheme] = usePersistent<ThemeChoice>('kibitz.theme', 'dark');

  useEffect(() => {
    const root = document.documentElement;
    if (theme !== 'system') {
      root.setAttribute('data-theme', theme);
      return;
    }
    // Legacy `'system'`: mirror the OS onto the attribute, and keep mirroring it
    // if the OS flips while the tab is open.
    const query = window.matchMedia?.('(prefers-color-scheme: dark)');
    const apply = () => root.setAttribute('data-theme', query?.matches ? 'dark' : 'light');
    apply();
    query?.addEventListener('change', apply);
    return () => query?.removeEventListener('change', apply);
  }, [theme]);

  const toggle = useCallback(() => {
    const prefersDark = window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false;
    const current = theme === 'system' ? (prefersDark ? 'dark' : 'light') : theme;
    setTheme(current === 'dark' ? 'light' : 'dark');
  }, [theme, setTheme]);

  return [theme, toggle];
}
