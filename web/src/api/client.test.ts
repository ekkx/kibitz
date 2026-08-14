import { afterEach, describe, expect, it, vi } from 'vitest';
import { analyze } from './client.ts';
import { ApiError } from './transport.ts';
import type { AnalyzeCandidatesEvent, Candidate, PositionAnalysis } from './types.ts';

/**
 * `POST /analyze` is a stream, and this file is about the two things that makes
 * hard: telling a partial from a result, and failing *after* output has already
 * been delivered.
 *
 * `analyze` is exercised through the real transport — the global `fetch` is
 * stubbed rather than the module — so `raiseForStatus`, the SSE reader and the
 * event dispatch all run exactly as they do in the app.
 */

const FEN = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';

function candidate(san: string, uci: string, cp: number): Candidate {
  return {
    san,
    uci,
    score: { kind: 'cp', value: cp },
    win_prob: 0.5 + cp / 1000,
    pv: [san],
  };
}

const FINAL: PositionAnalysis = {
  fen: FEN,
  depth: 12,
  candidates: [candidate('e4', 'e2e4', 42), candidate('d4', 'd2d4', 30)],
  context: {
    candidates: [],
    played_rank: 0,
    played: {
      san: 'e4',
      uci: 'e2e4',
      win_prob_before: 0.52,
      win_prob_after: 0.53,
      delta: 0.01,
      classification: 'best',
      accuracy: 99,
    },
    counterfactual: null,
  },
  explanations: {},
};

const frame = (event: string, data: unknown) =>
  `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;

/**
 * An SSE response whose bytes are chopped at sizes that have nothing to do with
 * event boundaries — the same property `mock/mockServer.ts` gives its streams,
 * and the reason this suite tests the reader rather than merely the dispatch.
 */
function sseResponse(events: string[], status = 200): Response {
  const bytes = new TextEncoder().encode(events.join(''));
  const sizes = [7, 1, 43, 3, 17, 2, 91];
  let offset = 0;
  let step = 0;

  const body = new ReadableStream<Uint8Array>({
    pull(controller) {
      if (offset >= bytes.length) {
        controller.close();
        return;
      }
      const size = sizes[step++ % sizes.length]!;
      controller.enqueue(bytes.slice(offset, offset + size));
      offset += size;
    },
  });

  return new Response(body, {
    status,
    headers: { 'Content-Type': 'text/event-stream' },
  });
}

function stub(response: Response | (() => Response)): void {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => (typeof response === 'function' ? response() : response)),
  );
}

afterEach(() => vi.unstubAllGlobals());

describe('analyze', () => {
  it('reports every partial in order and resolves with the final analysis', async () => {
    stub(
      sseResponse([
        frame('candidates', { depth: 6, candidates: [candidate('d4', 'd2d4', 10)] }),
        frame('candidates', { depth: 9, candidates: [candidate('e4', 'e2e4', 35)] }),
        frame('candidates', { depth: 12, candidates: FINAL.candidates }),
        frame('analysis', FINAL),
      ]),
    );

    const seen: AnalyzeCandidatesEvent[] = [];
    const result = await analyze('s1', { node_id: 3, depth: 12 }, {
      onCandidates: (event) => seen.push(event),
    });

    expect(seen.map((e) => e.depth)).toEqual([6, 9, 12]);
    expect(seen[0]!.candidates[0]!.san).toBe('d4');
    expect(result).toEqual(FINAL);
    // The verdict is on the result and was never on a partial — that is the
    // whole contract of this endpoint.
    expect(result.context?.played.classification).toBe('best');
    for (const partial of seen) {
      expect(Object.keys(partial).sort()).toEqual(['candidates', 'depth']);
    }
  });

  it('works for a caller that ignores the partials entirely', async () => {
    stub(
      sseResponse([
        frame('candidates', { depth: 6, candidates: FINAL.candidates }),
        frame('analysis', FINAL),
      ]),
    );
    // The old JSON behaviour, unchanged: one call, one `PositionAnalysis`.
    await expect(analyze('s1', { node_id: 3, depth: 12 })).resolves.toEqual(FINAL);
  });

  it('resolves with a single analysis event and no partials', async () => {
    // What a cached or terminal position sends: there was no search to narrate.
    stub(sseResponse([frame('analysis', FINAL)]));

    const seen: AnalyzeCandidatesEvent[] = [];
    const result = await analyze('s1', { node_id: 3, depth: 12 }, {
      onCandidates: (event) => seen.push(event),
    });
    expect(seen).toEqual([]);
    expect(result.depth).toBe(12);
  });

  /**
   * The case the streaming shape creates and the old JSON body could not: the
   * request fails *after* it has already produced output. The partials that
   * arrived were true when they arrived and stay delivered; the call still
   * rejects, because no verdict was ever reached.
   */
  it('rejects when the stream errors after partials have arrived', async () => {
    stub(
      sseResponse([
        frame('candidates', { depth: 6, candidates: FINAL.candidates }),
        frame('candidates', { depth: 8, candidates: FINAL.candidates }),
        frame('error', { error: 'engine process died' }),
      ]),
    );

    const seen: AnalyzeCandidatesEvent[] = [];
    const failure = await analyze('s1', { node_id: 3, depth: 12 }, {
      onCandidates: (event) => seen.push(event),
    }).catch((error: unknown) => error);

    expect(seen.map((e) => e.depth)).toEqual([6, 8]);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).message).toBe('engine process died');
    expect((failure as ApiError).isCancelled).toBe(false);
  });

  /**
   * Cancellation is the error that actually happens, and the client acts on it
   * by staying silent. It used to be a `409` status; mid-stream it can only be
   * an event, so `client.ts` turns it back into the same `ApiError` — otherwise
   * every superseded analysis would surface as a red error in the panel.
   */
  it('turns a cancelled error event back into the 409 the status used to be', async () => {
    stub(
      sseResponse([
        frame('candidates', { depth: 6, candidates: FINAL.candidates }),
        frame('error', { error: 'cancelled' }),
      ]),
    );

    const failure = await analyze('s1', { node_id: 3, depth: 12 }).catch(
      (error: unknown) => error,
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(409);
    expect((failure as ApiError).isCancelled).toBe(true);
  });

  it('rejects when the stream ends without a verdict', async () => {
    // A dropped connection: partials arrived, `analysis` never did. Resolving
    // with nothing would leave the panel loading forever.
    stub(sseResponse([frame('candidates', { depth: 6, candidates: FINAL.candidates })]));

    await expect(analyze('s1', { node_id: 3, depth: 12 })).rejects.toThrow(
      /ended without a result/,
    );
  });

  it('ignores a partial that arrives after the verdict', async () => {
    // A straggler from a stream that is already over would walk the board back
    // to a shallower ranking than the one just drawn.
    stub(
      sseResponse([
        frame('analysis', FINAL),
        frame('candidates', { depth: 6, candidates: [candidate('a3', 'a2a3', -50)] }),
      ]),
    );

    const seen: AnalyzeCandidatesEvent[] = [];
    const result = await analyze('s1', { node_id: 3, depth: 12 }, {
      onCandidates: (event) => seen.push(event),
    });
    expect(seen).toEqual([]);
    expect(result).toEqual(FINAL);
  });

  it('raises an HTTP failure that happened before the stream opened', async () => {
    // Unknown session, unknown node, no engine: decided before the response
    // began, so they are still ordinary statuses with the usual error body.
    stub(
      new Response(JSON.stringify({ error: 'node not found' }), {
        status: 404,
        headers: { 'Content-Type': 'application/json' },
      }),
    );

    const failure = await analyze('s1', { node_id: 99, depth: 12 }).catch(
      (error: unknown) => error,
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
    expect((failure as ApiError).message).toBe('node not found');
  });

  it('stops when the caller aborts mid-stream', async () => {
    const controller = new AbortController();
    stub(
      sseResponse([
        frame('candidates', { depth: 6, candidates: FINAL.candidates }),
        frame('analysis', FINAL),
      ]),
    );

    const seen: AnalyzeCandidatesEvent[] = [];
    const pending = analyze(
      's1',
      { node_id: 3, depth: 12 },
      {
        onCandidates: (event) => {
          seen.push(event);
          controller.abort();
        },
      },
      controller.signal,
    ).catch((error: unknown) => error);

    await pending;
    expect(seen).toHaveLength(1);
  });
});
