import { useCallback, useEffect, useRef, useState } from 'react';
import { analyzeGame } from '../api/client.ts';
import type { GameTree, PositionAnalysis } from '../api/types.ts';
import {
  accumulate,
  accuracySummary,
  emptyAccumulator,
  mainlineNodes,
  summarize,
  type SweepSummary,
} from '../ui/accuracy.ts';

export type { SweepSummary };

export interface SweepState {
  running: boolean;
  done: number;
  total: number;
  error: string | null;
  summary: SweepSummary | null;
  run: () => void;
  cancel: () => void;
}

export interface SweepInput {
  sessionId: string | null;
  /** The session tree, used to decide whether a sweep is needed at all. */
  tree: GameTree | null;
  recordAnalysis: (nodeId: number, analysis: PositionAnalysis) => void;
  /**
   * Search depth, from the user's setting. It is read when a sweep starts, so
   * changing it never disturbs one that is already running; re-running the
   * sweep by hand is what applies a new depth to a game already analysed, and
   * the "Analyse whole game" button already does exactly that.
   */
  depth: number;
  /**
   * False while the engine is missing or its health is still unknown. Nothing
   * to run in that case, so the automatic sweep waits (and never fires if the
   * engine never comes up).
   */
  enabled: boolean;
}

/**
 * `POST /api/sessions/{id}/analyze-game` over SSE: progress plus one
 * `PositionAnalysis` per mainline node, ending in an accuracy summary.
 *
 * The sweep runs **automatically once per session**, the way every comparable
 * tool evaluates a game the moment it is opened; the button remains for
 * re-running it, and the Stop button still cancels. The decision is taken once,
 * from the tree as it first arrives:
 *
 *   - a session with no moves (opened from a FEN) is never swept — there is
 *     nothing to sweep, and the per-node analysis covers it;
 *   - a session whose mainline is already analysed server-side is not swept
 *     again; its summary is computed from the tree instead, so re-opening an
 *     analysed game is instant and costs no engine time;
 *   - anything else is swept once. Playing moves, navigating, cancelling and
 *     re-running never re-arm it.
 */
export function useSweep({
  sessionId,
  tree,
  recordAnalysis,
  depth,
  enabled,
}: SweepInput): SweepState {
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState(0);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [summary, setSummary] = useState<SweepSummary | null>(null);
  const controllerRef = useRef<AbortController | null>(null);

  const cancel = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
    setRunning(false);
  }, []);

  const run = useCallback(() => {
    if (!sessionId) return;
    controllerRef.current?.abort();
    const controller = new AbortController();
    controllerRef.current = controller;

    const accumulator = emptyAccumulator();
    setRunning(true);
    setDone(0);
    setTotal(0);
    setError(null);
    setSummary(null);

    analyzeGame(
      sessionId,
      { depth },
      {
        onProgress: (event) => {
          setDone(event.done);
          setTotal(event.total);
        },
        onNode: (analysis, nodeId) => {
          if (nodeId !== null) recordAnalysis(nodeId, analysis);
          accumulate(accumulator, analysis);
        },
        onDone: () => {
          setRunning(false);
          setSummary(summarize(accumulator));
        },
        onError: (message) => {
          setError(message);
          setRunning(false);
        },
      },
      controller.signal,
    ).catch((failure: unknown) => {
      if (controller.signal.aborted) return;
      setError(failure instanceof Error ? failure.message : String(failure));
      setRunning(false);
    });
  }, [sessionId, depth, recordAnalysis]);

  // Reset when the session goes away, so a new game does not inherit a summary.
  useEffect(() => {
    if (sessionId) return;
    setRunning(false);
    setDone(0);
    setTotal(0);
    setError(null);
    setSummary(null);
  }, [sessionId]);

  /**
   * The automatic sweep. `decidedFor` holds the session it has already been
   * decided for, which is what makes this fire exactly once per session — under
   * React's StrictMode double-invoked effects, on every later tree update, and
   * when the user cancels or re-runs by hand.
   */
  const decidedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!enabled || !sessionId || !tree) return;
    if (decidedFor.current === sessionId) return;
    decidedFor.current = sessionId;

    const mainline = mainlineNodes(tree);
    if (mainline.length === 0) return;

    const analysed = mainline
      .map((node) => node.analysis)
      .filter((analysis): analysis is PositionAnalysis => analysis !== null);
    if (analysed.length === mainline.length) {
      setSummary(accuracySummary(analysed));
      return;
    }
    run();
  }, [enabled, sessionId, tree, run]);

  return { running, done, total, error, summary, run, cancel };
}
