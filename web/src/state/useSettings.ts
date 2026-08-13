import { useCallback, useEffect, useMemo, useState } from 'react';
import { DEFAULT_ARROWS, MAX_ARROWS } from '../ui/arrows.ts';
import { setSoundEnabled } from '../ui/sounds.ts';

/**
 * The few knobs the user actually needs, persisted in localStorage.
 *
 * The numeric ones are deliberately small enumerations rather than free numbers.
 * An arrow count is bounded by how many candidates the server returns
 * (`MAX_ARROWS`, the MultiPV width — the weight ramp is a separate concern and
 * does not cap it), and a depth is bounded by what a whole-game sweep can cost —
 * neither is a dimension where a text field would help anyone.
 */

/** 0 means a bare board; the ceiling is the server's MultiPV width. */
export const ARROW_COUNTS: readonly number[] = Array.from(
  { length: MAX_ARROWS + 1 },
  (_, count) => count,
);

/**
 * Search depths offered, with the label each is presented under.
 *
 * Measured on the reference machine: a whole-game sweep of the 34-position
 * sample game, cold cache, nothing else competing for the cores.
 *
 *   depth 10 ≈ 6 s    depth 12 ≈ 6 s
 *   depth 14 ≈ 24 s   depth 16 ≈ 72 s   depth 19 ≈ 8 min
 *
 * Cost grows roughly ×3.5 every two ply from 12 upwards, which is why the
 * ladder stops at 16: depth 18 is half an hour for one game, and offering it
 * would be a trap rather than an option. Below 12 the numbers flatten out into
 * process startup and the engine handshake, so 10 buys back no real time.
 */
export const DEPTHS = [10, 12, 14, 16] as const;

export type Depth = (typeof DEPTHS)[number];

/**
 * The default, and the reason for it: this tool explains games to club players,
 * whose mistakes are hanging pieces and two-to-three-move tactics. Those do not
 * need a deep search to find, and the sweep runs the moment a game is opened —
 * so the cost of depth is paid on the first impression, every time.
 *
 * 12 sweeps the sample game in about six seconds from cold — indistinguishable
 * from depth 10, whose remaining cost is startup rather than search — while
 * being deep enough that fixed-depth evaluation noise stops manufacturing
 * "mistakes" out of two adjacent quiet moves. The next rung up costs four times
 * as much for verdicts a club player would not act on differently.
 */
export const DEFAULT_DEPTH = 12;

/**
 * Sound is on by default, which is the one setting here where the default is a
 * real decision rather than a middle value.
 *
 * The argument for defaulting to off is that unexpected noise from a web page is
 * rude. It does not apply: nothing here makes a sound until the user moves a
 * piece or steps through a game, so the first sound is always the direct
 * consequence of an action they just took, and it is feedback for *that action*.
 * A user who does not want it finds the toggle immediately, because they went
 * looking after hearing something. A user who would have liked it, but got a
 * silent board, never learns the feature exists at all.
 */
export const DEFAULT_SOUND = true;

const ARROWS_KEY = 'kibitz.arrowCount';
const DEPTH_KEY = 'kibitz.analysisDepth';
const SOUND_KEY = 'kibitz.sound';

export interface SettingsState {
  /** How many ranked engine arrows the board draws. */
  arrowCount: number;
  /** Search depth sent as `depth` on `/analyze` and `/analyze-game`. */
  depth: number;
  /** Whether moves make a sound (`ui/sounds.ts`). */
  sound: boolean;
  setArrowCount: (count: number) => void;
  setDepth: (depth: number) => void;
  setSound: (on: boolean) => void;
}

export function useSettings(): SettingsState {
  const [arrowCount, setArrowCount] = usePersistentChoice(
    ARROWS_KEY,
    DEFAULT_ARROWS,
    ARROW_COUNTS,
  );
  const [depth, setDepth] = usePersistentChoice(DEPTH_KEY, DEFAULT_DEPTH, DEPTHS);
  const [sound, setSound] = usePersistentFlag(SOUND_KEY, DEFAULT_SOUND);

  /**
   * Push the setting into the sounds module, from here, once.
   *
   * `ui/sounds.ts` is a plain module with a module-level mute flag, not a React
   * value, because it is called from move handlers deep in the board that have
   * no business reading preferences. That design needs *somebody* to keep the
   * flag in step with the setting, and this hook is the only place that can do
   * it correctly: it owns the value, it runs whenever the value changes, and it
   * runs on mount — so the flag is right from the first render, including the
   * case where the user turned sound off in a previous session and localStorage
   * is the only thing that remembers.
   *
   * The alternative — every component that calls `playSound` also checking
   * `settings.sound` — is the same check written N times, and the Nth one is the
   * one that gets forgotten in a code path with no test.
   */
  useEffect(() => {
    setSoundEnabled(sound);
  }, [sound]);

  return useMemo(
    () => ({ arrowCount, depth, sound, setArrowCount, setDepth, setSound }),
    [arrowCount, depth, sound, setArrowCount, setDepth, setSound],
  );
}

/**
 * `useState` over a fixed set of numbers, backed by localStorage.
 *
 * Anything stored that is not one of `allowed` is discarded rather than
 * clamped: it is either a value from an older build whose meaning has changed,
 * or something a user typed into devtools, and in both cases the default is the
 * honest answer. This is why it does not reuse `usePersistent`, which is
 * string-typed and trusts whatever it reads back.
 */
function usePersistentChoice(
  key: string,
  fallback: number,
  allowed: readonly number[],
): [number, (value: number) => void] {
  const [value, setValue] = useState<number>(() => {
    try {
      // `Number(null)` is 0, which is a legitimate arrow count — so a missing
      // key has to be distinguished from a stored zero before parsing.
      const stored = localStorage.getItem(key);
      if (stored === null) return fallback;
      const parsed = Number(stored);
      return allowed.includes(parsed) ? parsed : fallback;
    } catch {
      return fallback;
    }
  });

  const set = useCallback(
    (next: number) => {
      if (!allowed.includes(next)) return;
      setValue(next);
      try {
        localStorage.setItem(key, String(next));
      } catch {
        /* private mode: keep it in memory only */
      }
    },
    [key, allowed],
  );

  return [value, set];
}

/**
 * The same contract as `usePersistentChoice`, for a flag.
 *
 * A sibling rather than a generalisation, because the parse *is* the entire
 * difference between the two: `usePersistentChoice` reads a number and checks it
 * against a list, and there is no honest way to bend that into reading a boolean
 * — `Number('true')` is `NaN` and `Boolean('false')` is `true`, so every route
 * through the numeric version either lies or needs a branch that makes it two
 * functions wearing one name. Twenty lines duplicated is cheaper than a helper
 * that has to be read twice to work out which half applies.
 *
 * The behaviour that does carry over is the important one: only the two strings
 * this writes are recognised, and anything else is discarded in favour of the
 * default rather than coerced. A stored `'1'`, `'yes'` or `'undefined'` is
 * either an older build's encoding or something typed into devtools, and in both
 * cases guessing what the user meant is worse than starting fresh.
 */
function usePersistentFlag(key: string, fallback: boolean): [boolean, (value: boolean) => void] {
  const [value, setValue] = useState<boolean>(() => {
    try {
      const stored = localStorage.getItem(key);
      if (stored === 'true') return true;
      if (stored === 'false') return false;
      return fallback;
    } catch {
      return fallback;
    }
  });

  const set = useCallback(
    (next: boolean) => {
      setValue(next);
      try {
        localStorage.setItem(key, String(next));
      } catch {
        /* private mode: keep it in memory only */
      }
    },
    [key],
  );

  return [value, set];
}
