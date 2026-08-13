/**
 * In-process stand-in for `crates/server`, used when `VITE_MOCK=1`.
 *
 * It implements every route in `docs/API.md` and returns real `Response`
 * objects, so the app's client, SSE parser and rendering code are the same in
 * mock mode as against the Rust server.
 *
 * The SSE responses deliberately chop the byte stream at sizes that have
 * nothing to do with event boundaries — mid-`data:` line, mid-UTF-8 character,
 * and across the blank line that separates events — so that running in mock
 * mode is itself a test of the stream reader.
 */
import type { PositionAnalysis } from '../api/types.ts';
import {
  MOCK_HEALTH,
  MOCK_LANGUAGES,
  SCRIPTED_MOVES,
  genericExplanation,
  type ScriptedMove,
} from './fixtures.ts';
import { analysisFor, buildSession, mainlineIds, playOnTree } from './engine.ts';
import type { GameTree } from '../api/types.ts';

interface MockSession {
  id: string;
  tree: GameTree;
  headers: Record<string, string>;
  /** `${nodeId}:${lang}` → explanation, mirroring the server-side cache. */
  explanations: Map<string, string>;
  analysed: Set<number>;
}

const sessions = new Map<string, MockSession>();
let sessionCounter = 0;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/* ---------- response helpers ---------- */

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

const fail = (status: number, error: string) => json({ error }, status);

function sseEvent(event: string, data: unknown): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
}

/**
 * Turn a script of event strings into a chunked byte stream.
 *
 * Chunk sizes cycle through a list of awkward values and a partial chunk is
 * carried over between events, which guarantees that events get split across
 * chunk boundaries in every way the reader has to survive.
 */
function sseResponse(script: () => AsyncGenerator<string>, signal?: AbortSignal | null): Response {
  const encoder = new TextEncoder();
  const chunkSizes = [7, 1, 43, 3, 17, 2, 91];
  let sizeIndex = 0;

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      let pending = new Uint8Array(0);
      const append = (bytes: Uint8Array) => {
        const merged = new Uint8Array(pending.length + bytes.length);
        merged.set(pending);
        merged.set(bytes, pending.length);
        pending = merged;
      };
      const drain = (keepRemainder: boolean) => {
        while (pending.length > 0) {
          const size = chunkSizes[sizeIndex++ % chunkSizes.length]!;
          if (keepRemainder && pending.length < size) break;
          const take = Math.min(size, pending.length);
          controller.enqueue(pending.slice(0, take));
          pending = pending.slice(take);
        }
      };

      try {
        for await (const event of script()) {
          if (signal?.aborted) break;
          append(encoder.encode(event));
          drain(true);
        }
        drain(false);
      } catch (error) {
        controller.enqueue(
          encoder.encode(sseEvent('error', { error: (error as Error).message ?? 'mock failure' })),
        );
      }
      controller.close();
    },
  });

  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' },
  });
}

/* ---------- explanation streaming ---------- */

/** Split text into human-sized pieces: words for Latin, short runs for CJK. */
function splitForStreaming(text: string): string[] {
  const pieces: string[] = [];
  const tokens = text.match(/\S+\s*|\s+/g) ?? [text];
  let buffer = '';
  for (const token of tokens) {
    buffer += token;
    // CJK has no spaces, so cap by length as well as by word count.
    if (buffer.length >= 12 || /[。、）]$/.test(buffer.trim())) {
      pieces.push(buffer);
      buffer = '';
    }
  }
  if (buffer) pieces.push(buffer);
  return pieces;
}

function explanationText(session: MockSession, nodeId: number, lang: string): string {
  const scripted: ScriptedMove | undefined = SCRIPTED_MOVES.find((move) => move.ply === nodeId);
  if (scripted) return scripted.explanations[lang] ?? scripted.explanations.en ?? '';
  const node = session.tree.nodes.find((candidate) => candidate.id === nodeId);
  const analysis = node ? analysisFor(session.tree, nodeId) : null;
  const played = analysis?.context?.played;
  return genericExplanation(played?.san ?? '(root)', played?.classification ?? 'good', lang);
}

/* ---------- routing ---------- */

export async function mockFetch(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  const rawUrl =
    typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;
  const url = new URL(rawUrl, globalThis.location?.origin ?? 'http://localhost');
  const method = (init?.method ?? 'GET').toUpperCase();
  const signal = init?.signal ?? null;
  const path = url.pathname.replace(/^\/api/, '');
  const body = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : {};

  await sleep(90); // enough latency that loading states are visible

  if (path === '/health') return json(MOCK_HEALTH);
  if (path === '/languages') return json(MOCK_LANGUAGES);

  if (path === '/sessions' && method === 'POST') {
    let built;
    try {
      built = buildSession({ pgn: body.pgn as string, fen: body.fen as string });
    } catch {
      return fail(400, 'could not parse the game');
    }
    const id = `mock-${++sessionCounter}`;
    const session: MockSession = {
      id,
      tree: built.tree,
      headers: built.headers,
      explanations: new Map(),
      analysed: new Set(),
    };
    sessions.set(id, session);
    return json({ session_id: id, tree: session.tree, headers: session.headers });
  }

  const match = /^\/sessions\/([^/]+)(\/.*)?$/.exec(path);
  if (!match) return fail(404, 'not found');
  const session = sessions.get(match[1]!);
  if (!session) return fail(404, 'session not found');
  const sub = match[2] ?? '';

  if (sub === '' && method === 'GET') {
    return json({ session_id: session.id, tree: session.tree, headers: session.headers });
  }

  if (sub === '/play' && method === 'POST') {
    const played = playOnTree(session.tree, body.node_id as number, {
      uci: body.uci as string | undefined,
      san: body.san as string | undefined,
    });
    if (!played) return fail(400, 'illegal move');
    return json({ ...played, tree: session.tree });
  }

  if (sub === '/analyze' && method === 'POST') {
    await sleep(320);
    const nodeId = body.node_id as number;
    const analysis = analysisFor(session.tree, nodeId, cachedExplanations(session, nodeId));
    if (!analysis) return fail(404, 'node not found');
    session.analysed.add(nodeId);
    return json(analysis);
  }

  if (sub === '/analyze-game' && method === 'POST') {
    const ids = mainlineIds(session.tree).filter((id) => id !== session.tree.root);
    return sseResponse(async function* () {
      let done = 0;
      for (const id of ids) {
        if (signal?.aborted) return;
        await sleep(120);
        done += 1;
        session.analysed.add(id);
        yield sseEvent('progress', { node_id: id, done, total: ids.length });
        const analysis = analysisFor(session.tree, id, cachedExplanations(session, id));
        if (analysis) {
          storeAnalysis(session, id, analysis);
          yield sseEvent('node', analysis);
        }
      }
      yield sseEvent('done', { total: ids.length });
    }, signal);
  }

  const explainMatch = /^\/explain\/(\d+)$/.exec(sub);
  if (explainMatch && method === 'GET') {
    const nodeId = Number(explainMatch[1]);
    const lang = url.searchParams.get('lang') ?? 'en';
    if (!MOCK_LANGUAGES.languages.some((language) => language.code === lang)) {
      return fail(400, `unsupported language: ${lang}`);
    }
    if (!session.analysed.has(nodeId)) return fail(409, 'not analyzed');

    const key = `${nodeId}:${lang}`;
    const cached = session.explanations.get(key);
    const text = cached ?? explanationText(session, nodeId, lang);

    return sseResponse(async function* () {
      if (cached) {
        // A cached explanation arrives as a single event, per API.md.
        yield sseEvent('done', { text, lang, model: 'claude-haiku-4-5', cached: true });
        return;
      }
      for (const piece of splitForStreaming(text)) {
        if (signal?.aborted) return;
        await sleep(55);
        yield sseEvent('delta', { text: piece });
      }
      session.explanations.set(key, text);
      yield sseEvent('done', { text, lang, model: 'claude-haiku-4-5', cached: false });
    }, signal);
  }

  if (sub === '/ask' && method === 'POST') {
    const lang = (body.lang as string) ?? 'en';
    const question = String(body.question ?? '');
    const answer =
      lang === 'ja'
        ? `「${question}」について。エンジンにその手を解析させたところ、局面の評価は大きく変わらなかった。読み筋は盤上のものと同じ方向を向いている。（モックデータ）`
        : `On "${question}": running that move through the engine leaves the evaluation roughly where it was, and the resulting line points the same way as the one on the board. (Mock data.)`;
    return sseResponse(async function* () {
      yield sseEvent('tool', { name: 'analyze_move', input: { question } });
      for (const piece of splitForStreaming(answer)) {
        if (signal?.aborted) return;
        await sleep(55);
        yield sseEvent('delta', { text: piece });
      }
      yield sseEvent('done', { text: answer, lang, model: 'claude-sonnet-4-5', cached: false });
    }, signal);
  }

  return fail(404, 'not found');
}

function cachedExplanations(session: MockSession, nodeId: number): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of session.explanations) {
    const [id, lang] = key.split(':');
    if (Number(id) === nodeId && lang) out[lang] = value;
  }
  return out;
}

function storeAnalysis(session: MockSession, nodeId: number, analysis: PositionAnalysis): void {
  const node = session.tree.nodes.find((candidate) => candidate.id === nodeId);
  if (node) node.analysis = analysis;
}
