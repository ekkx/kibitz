import { useCallback, useEffect, useRef, useState } from 'react';
import { explain } from '../api/client.ts';
import { ApiError } from '../api/transport.ts';

export type ExplanationStatus = 'idle' | 'streaming' | 'done' | 'error' | 'not-analyzed';

export interface ExplanationState {
  text: string;
  status: ExplanationStatus;
  cached: boolean;
  model: string | null;
  error: string | null;
  request: () => void;
}

export interface ExplanationInput {
  sessionId: string | null;
  nodeId: number | null;
  /** Explanation language — the `lang` request parameter, not the UI language. */
  lang: string;
  /** An explanation the analysis already carried for this language. */
  preloaded?: string | undefined;
}

/**
 * Streams `GET /api/sessions/{id}/explain/{node_id}?lang=…`.
 *
 * Text arrives as `delta` events and is appended as it comes, so the panel
 * fills in while the counterfactual replays on the board — the two together
 * are the moment the tool is built around.
 *
 * **Nothing here ever starts a generation on its own.** `request` is the only
 * path to the network, and it is reached from one button. This used to have an
 * `auto` flag that fired on notable moves, and the flag was wrong twice over: a
 * generation costs money and several seconds of somebody else's compute, so
 * spending it on a position the user merely arrowed past is a decision they did
 * not make — and because it fired from an effect keyed on the node, holding the
 * arrow key down through a game full of mistakes started and aborted a request
 * per ply. Text appearing without a press was indistinguishable from the app
 * misbehaving, which is exactly what it was.
 *
 * The effect below therefore only ever *shows* text, never fetches it: an
 * explanation the analysis already carried for this language appears instantly
 * and is marked cached, and everything else is `idle` until the button is
 * pressed. Changing language or node re-reads that cache and abandons any
 * stream still arriving for the position that was left, which is the one thing
 * that must still happen without being asked.
 */
export function useExplanation({
  sessionId,
  nodeId,
  lang,
  preloaded,
}: ExplanationInput): ExplanationState {
  const [text, setText] = useState('');
  const [status, setStatus] = useState<ExplanationStatus>('idle');
  const [cached, setCached] = useState(false);
  const [model, setModel] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const controllerRef = useRef<AbortController | null>(null);

  const run = useCallback(() => {
    if (!sessionId || nodeId === null) return;
    controllerRef.current?.abort();
    const controller = new AbortController();
    controllerRef.current = controller;

    setText('');
    setError(null);
    setCached(false);
    setModel(null);
    setStatus('streaming');

    explain(
      sessionId,
      nodeId,
      lang,
      {
        onDelta: (chunk) => setText((current) => current + chunk),
        onDone: (event) => {
          // `done` carries the full text: adopt it, so a dropped delta cannot
          // leave a hole in the middle of the explanation.
          setText(event.text);
          setCached(event.cached);
          setModel(event.model);
          setStatus('done');
        },
        onError: (message) => {
          setError(message);
          setStatus('error');
        },
      },
      controller.signal,
    ).catch((failure: unknown) => {
      if (controller.signal.aborted) return;
      if (failure instanceof ApiError && failure.isNotAnalyzed) {
        setStatus('not-analyzed');
        return;
      }
      setError(failure instanceof Error ? failure.message : String(failure));
      setStatus('error');
    });
  }, [sessionId, nodeId, lang]);

  useEffect(() => {
    controllerRef.current?.abort();
    setText(preloaded ?? '');
    setStatus(preloaded ? 'done' : 'idle');
    setCached(Boolean(preloaded));
    setError(null);
    return () => controllerRef.current?.abort();
    // `preloaded` is derived from the same node/lang pair, so this list is complete.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, nodeId, lang]);

  return { text, status, cached, model, error, request: run };
}
