import { runExclusive } from './engineQueue.ts';
import { parseEventData, readSseStream } from './sse.ts';
import { apiFetch, raiseForStatus } from './transport.ts';
import type {
  ErrorEvent as ApiErrorEvent,
  ExplainDeltaEvent,
  ExplainDoneEvent,
  HealthResponse,
  LanguagesResponse,
  PlayResponse,
  PositionAnalysis,
  SessionResponse,
  SweepDoneEvent,
  SweepProgressEvent,
  ToolEvent,
} from './types.ts';

const BASE = '/api';

/**
 * Every request that can carry a `depth` carries one, taken from the user's
 * setting (`state/useSettings.ts`) — the server has a default of its own, and
 * two defaults that can silently disagree is a bug waiting to be reported as
 * "the numbers change when I click around".
 *
 * One depth everywhere is deliberate for the same reason: a node analysed by
 * the sweep and the same node analysed on its own agree, so navigating cannot
 * make an evaluation jump, and merging two sources of analysis never has to
 * arbitrate between a deep and a shallow answer.
 */

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await apiFetch(`${BASE}${path}`, { signal: signal ?? null });
  await raiseForStatus(res);
  return (await res.json()) as T;
}

async function postJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const res = await apiFetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    signal: signal ?? null,
  });
  await raiseForStatus(res);
  return (await res.json()) as T;
}

/* ---------- plain JSON endpoints ---------- */

export const getHealth = () => getJson<HealthResponse>('/health');

export const getLanguages = () => getJson<LanguagesResponse>('/languages');

export const createSession = (body: { pgn: string } | { fen?: string }) =>
  postJson<SessionResponse>('/sessions', body);

export const getSession = (id: string) => getJson<SessionResponse>(`/sessions/${id}`);

export const play = (
  id: string,
  body: { node_id: number } & ({ uci: string } | { san: string }),
) => postJson<PlayResponse>(`/sessions/${id}/play`, body);

/** `depth` is required here on purpose — see the note above `getJson`. */
export const analyze = (
  id: string,
  body: { node_id: number; depth: number },
  signal?: AbortSignal,
) => postJson<PositionAnalysis>(`/sessions/${id}/analyze`, body, signal);

/* ---------- SSE endpoints ---------- */

export interface SweepHandlers {
  onProgress?: (e: SweepProgressEvent) => void;
  /**
   * API.md's `node` event carries a bare `PositionAnalysis`, which has no node
   * id — so we pass along the id from the preceding `progress` event, which is
   * how the two are correlated. See README "Notes on the API contract".
   */
  onNode?: (analysis: PositionAnalysis, nodeId: number | null) => void;
  onDone?: (e: SweepDoneEvent) => void;
  onError?: (message: string) => void;
}

/**
 * The sweep holds the engine queue for its whole run: a single `analyze` slipped
 * in between two of its nodes would cancel it (see `engineQueue.ts`).
 */
export function analyzeGame(
  id: string,
  body: { depth: number },
  handlers: SweepHandlers,
  signal?: AbortSignal,
): Promise<void> {
  return runExclusive(() => streamGameAnalysis(id, body, handlers, signal), signal);
}

async function streamGameAnalysis(
  id: string,
  body: { depth: number },
  handlers: SweepHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const res = await apiFetch(`${BASE}/sessions/${id}/analyze-game`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
    body: JSON.stringify(body),
    signal: signal ?? null,
  });
  await raiseForStatus(res);

  let lastNodeId: number | null = null;
  await readSseStream(
    res,
    (ev) => {
      switch (ev.event) {
        case 'progress': {
          const data = parseEventData<SweepProgressEvent>(ev.data);
          if (data) {
            lastNodeId = data.node_id;
            handlers.onProgress?.(data);
          }
          break;
        }
        case 'node': {
          const data = parseEventData<PositionAnalysis>(ev.data);
          if (data) handlers.onNode?.(data, lastNodeId);
          break;
        }
        case 'done': {
          const data = parseEventData<SweepDoneEvent>(ev.data);
          handlers.onDone?.(data ?? { total: 0 });
          break;
        }
        case 'error': {
          const data = parseEventData<ApiErrorEvent>(ev.data);
          handlers.onError?.(data?.error ?? 'unknown error');
          break;
        }
        default:
          break;
      }
    },
    signal,
  );
}

export interface StreamTextHandlers {
  onDelta?: (text: string) => void;
  onDone?: (e: ExplainDoneEvent) => void;
  onTool?: (e: ToolEvent) => void;
  onError?: (message: string) => void;
}

function consumeTextStream(
  res: Response,
  handlers: StreamTextHandlers,
  signal?: AbortSignal,
): Promise<void> {
  return readSseStream(
    res,
    (ev) => {
      switch (ev.event) {
        case 'delta': {
          const data = parseEventData<ExplainDeltaEvent>(ev.data);
          if (data?.text) handlers.onDelta?.(data.text);
          break;
        }
        case 'done': {
          const data = parseEventData<ExplainDoneEvent>(ev.data);
          if (data) handlers.onDone?.(data);
          break;
        }
        case 'tool': {
          const data = parseEventData<ToolEvent>(ev.data);
          if (data) handlers.onTool?.(data);
          break;
        }
        case 'error': {
          const data = parseEventData<ApiErrorEvent>(ev.data);
          handlers.onError?.(data?.error ?? 'unknown error');
          break;
        }
        default:
          break;
      }
    },
    signal,
  );
}

/**
 * `GET .../explain/{node_id}?lang=` — a GET, so `EventSource` would work, but
 * we use the same fetch reader as the POST streams: one code path, and it
 * gives us `AbortSignal` (EventSource cannot be aborted mid-event) plus real
 * HTTP status handling for `409 not analyzed`.
 *
 * `lang` here is the *explanation* language, not the UI language.
 */
export async function explain(
  id: string,
  nodeId: number,
  lang: string,
  handlers: StreamTextHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const res = await apiFetch(
    `${BASE}/sessions/${id}/explain/${nodeId}?lang=${encodeURIComponent(lang)}`,
    { headers: { Accept: 'text/event-stream' }, signal: signal ?? null },
  );
  await raiseForStatus(res);
  await consumeTextStream(res, handlers, signal);
}

export async function ask(
  id: string,
  body: { node_id: number; question: string; lang: string },
  handlers: StreamTextHandlers,
  signal?: AbortSignal,
): Promise<void> {
  const res = await apiFetch(`${BASE}/sessions/${id}/ask`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
    body: JSON.stringify(body),
    signal: signal ?? null,
  });
  await raiseForStatus(res);
  await consumeTextStream(res, handlers, signal);
}
