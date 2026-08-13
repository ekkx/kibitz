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
  /** Whether to fetch without being asked (notable moves only). */
  auto: boolean;
  /** An explanation the analysis already carried for this language. */
  preloaded?: string | undefined;
}

/**
 * Streams `GET /api/sessions/{id}/explain/{node_id}?lang=…`.
 *
 * Text arrives as `delta` events and is appended as it comes, so the panel
 * fills in while the counterfactual replays on the board — the two together
 * are the moment the tool is built around.
 */
export function useExplanation({
  sessionId,
  nodeId,
  lang,
  auto,
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
    if (!preloaded && auto) run();
    return () => controllerRef.current?.abort();
    // `preloaded` is derived from the same node/lang pair, so this list is complete.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, nodeId, lang, auto, run]);

  return { text, status, cached, model, error, request: run };
}
