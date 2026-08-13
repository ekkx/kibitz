import { afterEach, describe, expect, it } from 'vitest';
import { en } from './en.ts';
import { ja } from './ja.ts';
import { UI_LANGUAGES, setUiLanguage, t, tParts, uiLanguage } from './index.ts';

afterEach(() => setUiLanguage('en'));

describe('catalogues', () => {
  it('translates every string the English catalogue has', () => {
    // The type already requires this; the test says why it matters, which is
    // that a missing key is not a crash but an English sentence in the middle
    // of a Japanese interface.
    expect(Object.keys(ja).sort()).toEqual(Object.keys(en).sort());
  });

  it('leaves nothing untranslated except notation and the product name', () => {
    const shared = Object.keys(en).filter(
      (key) => en[key as keyof typeof en] === ja[key as keyof typeof en],
    );
    expect(shared.sort()).toEqual(
      ['app.name', 'eval.mateIn', 'import.fenLabel', 'import.pgnPlaceholder', 'import.tab.pgn', 'opening.eco'].sort(),
    );
  });

  it('offers exactly the languages that have a catalogue', () => {
    expect(UI_LANGUAGES).toEqual(['en', 'ja']);
  });
});

describe('t', () => {
  it('switches catalogue and reports which one is active', () => {
    expect(t('settings.title')).toBe('Settings');
    setUiLanguage('ja');
    expect(uiLanguage()).toBe('ja');
    expect(t('settings.title')).toBe('設定');
  });

  it('fills placeholders in either language', () => {
    expect(t('eval.depth', { n: 12 })).toBe('depth 12');
    setUiLanguage('ja');
    expect(t('eval.depth', { n: 12 })).toBe('深さ 12');
  });

  it('leaves an unknown placeholder alone rather than blanking it', () => {
    expect(t('analysis.failed')).toBe('Analysis failed: {message}');
  });
});

describe('tParts', () => {
  /**
   * The reason `tParts` exists: the count and its label swap places between
   * these two languages, and a component that concatenates them itself is stuck
   * in English order no matter how good the strings are.
   */
  it('takes the word order from the catalogue, not from the caller', () => {
    const order = (): unknown[] =>
      tParts('sweep.count', { n: 3, label: 'X' }).map(
        (part) => (part as { props: { children: unknown } }).props.children,
      );

    expect(order()).toEqual(['', 3, ' ', 'X', '']);
    setUiLanguage('ja');
    expect(order()).toEqual(['', 'X', ' ', 3, '']);
  });
});
