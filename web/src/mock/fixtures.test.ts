import { describe, expect, it, vi } from 'vitest';
import { Chess } from 'chess.js';
import { SAMPLE_PGN, SCRIPTED_MATES, SCRIPTED_MOVES } from './fixtures.ts';
import { analysisFor, buildSession } from './engine.ts';
import { mockFetch } from './mockServer.ts';
import { analyze } from '../api/client.ts';
import { expandSanLine } from '../chess/rules.ts';
import { deepestOpening } from '../ui/opening.ts';
import type { PositionAnalysis, SessionResponse } from '../api/types.ts';
import { readSseStream, type SseEvent } from '../api/sse.ts';

const built = buildSession({ pgn: SAMPLE_PGN });
const nodeAt = (id: number) => built.tree.nodes.find((node) => node.id === id)!;

describe('sample game fixture', () => {
  it('builds one node per ply plus the root', () => {
    expect(built.tree.nodes).toHaveLength(34); // 33 plies, mate on 17. Rd8#
    expect(built.headers.White).toBe('Paul Morphy');
    expect(nodeAt(33).san).toBe('Rd8#');
  });

  it.each(SCRIPTED_MOVES.map((move) => [move.ply, move] as const))(
    'ply %i: every scripted SAN is legal in its position',
    (ply, scripted) => {
      const node = nodeAt(ply);
      const parent = nodeAt(node.parent!);

      for (const candidate of scripted.candidates) {
        expect(() => new Chess(parent.fen).move(candidate.san), candidate.san).not.toThrow();
        // ...and each candidate's PV must play out from the parent position.
        expect(expandSanLine(parent.fen, candidate.pv), candidate.san).toHaveLength(
          candidate.pv.length,
        );
      }

      const counterfactual = scripted.counterfactual;
      if (counterfactual) {
        const startFen = counterfactual.from === 'node' ? node.fen : parent.fen;
        expect(expandSanLine(startFen, counterfactual.pv)).toHaveLength(counterfactual.pv.length);
      }
    },
  );

  it('places scripted mate scores on positions that really are mating', () => {
    for (const id of Object.keys(SCRIPTED_MATES).map(Number)) {
      expect(nodeAt(id)).toBeDefined();
    }
  });

  it('names the opening for the first plies only, and stops partway', () => {
    expect(nodeAt(0).opening).toBeNull(); // the starting position has no name
    expect(nodeAt(4).opening).toMatchObject({ eco: 'C41', name: 'Philidor Defense' });
    expect(nodeAt(5).opening).toMatchObject({ eco: 'C41', matched_plies: 5 });
    // 3... Bg4 onward is past the end of the table — unnamed, not out of book.
    expect(nodeAt(6).opening).toBeNull();
    expect(nodeAt(33).opening).toBeNull();

    // ...and every one of those later positions still resolves to the Philidor.
    expect(deepestOpening(built.tree, 6)).toMatchObject({ name: 'Philidor Defense' });
    expect(deepestOpening(built.tree, 33)).toMatchObject({ name: 'Philidor Defense' });
  });

  it('marks the opening moves as book, and stops two plies past the last name', () => {
    // The server skips the engine for these, so they carry no candidates and
    // placeholder win probabilities. Plies 6 and 7 are the interesting ones:
    // book, but past the end of the name table.
    for (let ply = 1; ply <= 7; ply++) {
      const analysis = analysisFor(built.tree, ply)!;
      expect(analysis.context?.played.classification, `ply ${ply}`).toBe('book');
      expect(analysis.candidates, `ply ${ply}`).toHaveLength(0);
      expect(analysis.context?.counterfactual).toBeNull();
    }
    // 4... Bxf3 leaves theory and is analysed like anything else.
    const left = analysisFor(built.tree, 8)!;
    expect(left.context?.played.classification).not.toBe('book');
    expect(left.candidates.length).toBeGreaterThan(0);
  });

  it('synthesises legal candidates for unscripted positions', () => {
    for (const node of built.tree.nodes) {
      const analysis = analysisFor(built.tree, node.id);
      if (!analysis) continue;
      for (const candidate of analysis.candidates) {
        expect(() => new Chess(node.fen).move(candidate.san), candidate.san).not.toThrow();
      }
      const counterfactual = analysis.context?.counterfactual;
      if (counterfactual) {
        expect(expandSanLine(counterfactual.start_fen, counterfactual.pv).length).toBeGreaterThan(0);
      }
    }
    // `analysisFor` synthesises a full context per node — candidates, a
    // counterfactual line and a static diff — and doing that for every node of
    // the sample game costs seconds, not milliseconds. That is the price of
    // checking the whole tree rather than a sample of it, so the test gets a
    // real timeout instead of vitest's default five seconds.
  }, 60_000);
});

describe('mock server', () => {
  it('runs a session end to end: create, analyze, explain', async () => {
    const created = await mockFetch('/api/sessions', {
      method: 'POST',
      body: JSON.stringify({ pgn: SAMPLE_PGN }),
    });
    const session = (await created.json()) as SessionResponse;
    expect(session.tree.nodes).toHaveLength(34);

    // Explaining before analysis is 409, per API.md.
    const tooEarly = await mockFetch(`/api/sessions/${session.session_id}/explain/18?lang=en`);
    expect(tooEarly.status).toBe(409);
    expect(await tooEarly.json()).toEqual({ error: 'not analyzed' });

    // `/analyze` is SSE: rankings while the search runs, then the verdict.
    const analyzeEvents: SseEvent[] = [];
    await readSseStream(
      await mockFetch(`/api/sessions/${session.session_id}/analyze`, {
        method: 'POST',
        body: JSON.stringify({ node_id: 18, depth: 20 }),
      }),
      (event) => analyzeEvents.push(event),
    );

    const partials = analyzeEvents.filter((event) => event.event === 'candidates');
    expect(partials.length).toBeGreaterThan(1); // it really is streamed
    expect(analyzeEvents.at(-1)!.event).toBe('analysis');

    // The rule this endpoint exists to enforce: a partial is a ranking and
    // nothing else, and its depth only ever climbs.
    let previousDepth = 0;
    for (const partial of partials) {
      const payload = JSON.parse(partial.data) as Record<string, unknown>;
      expect(Object.keys(payload).sort()).toEqual(['candidates', 'depth']);
      expect(payload.depth as number).toBeGreaterThan(previousDepth);
      previousDepth = payload.depth as number;
      expect((payload.candidates as unknown[]).length).toBeGreaterThan(0);
    }
    // Nothing below the server's streaming floor is ever sent.
    expect(JSON.parse(partials[0]!.data).depth).toBeGreaterThanOrEqual(6);

    const analysis = JSON.parse(analyzeEvents.at(-1)!.data) as PositionAnalysis;
    expect(analysis.context?.played.classification).toBe('blunder');
    expect(analysis.context?.counterfactual?.pv[0]).toBe('Nxb5');

    const events: SseEvent[] = [];
    const stream = await mockFetch(`/api/sessions/${session.session_id}/explain/18?lang=ja`);
    await readSseStream(stream, (event) => events.push(event));
    const deltas = events.filter((event) => event.event === 'delta');
    expect(deltas.length).toBeGreaterThan(5); // it really is streamed in pieces
    const done = events.at(-1)!;
    expect(done.event).toBe('done');
    const payload = JSON.parse(done.data) as { text: string; cached: boolean };
    expect(payload.cached).toBe(false);
    expect(deltas.map((d) => (JSON.parse(d.data) as { text: string }).text).join('')).toBe(
      payload.text,
    );

    // Second request for the same language is served from cache in one event.
    const again: SseEvent[] = [];
    await readSseStream(
      await mockFetch(`/api/sessions/${session.session_id}/explain/18?lang=ja`),
      (event) => again.push(event),
    );
    expect(again).toHaveLength(1);
    expect((JSON.parse(again[0]!.data) as { cached: boolean }).cached).toBe(true);

    const unsupported = await mockFetch(`/api/sessions/${session.session_id}/explain/18?lang=zz`);
    expect(unsupported.status).toBe(400);
  });

  /**
   * The mock's `/analyze` driven by the app's own client rather than by a raw
   * reader — the combination `VITE_MOCK=1` actually runs.
   *
   * `client.ts` reaches the mock through `apiFetch`, which is `fetch` unless the
   * build set `VITE_MOCK`, so stubbing the global is what puts the two halves
   * together here: the mock's deliberately awkward chunking, the real SSE
   * parser, and the real partial/final dispatch.
   */
  it('serves the app client a streamed analysis', async () => {
    const created = await mockFetch('/api/sessions', {
      method: 'POST',
      body: JSON.stringify({ pgn: SAMPLE_PGN }),
    });
    const session = (await created.json()) as SessionResponse;

    vi.stubGlobal('fetch', ((input: RequestInfo | URL, init?: RequestInit) =>
      mockFetch(input, init)) as typeof fetch);
    try {
      const seen: number[] = [];
      const analysis = await analyze(
        session.session_id,
        { node_id: 18, depth: 12 },
        { onCandidates: (event) => seen.push(event.depth) },
      );
      expect(seen.length).toBeGreaterThan(1);
      expect([...seen].sort((a, b) => a - b)).toEqual(seen); // monotone
      expect(seen[0]).toBeGreaterThanOrEqual(6);
      expect(analysis.context?.played.classification).toBe('blunder');

      // An unknown node is a status, not an event — the client must reject
      // rather than wait for a stream that will never open.
      await expect(
        analyze(session.session_id, { node_id: 999, depth: 12 }),
      ).rejects.toThrow('node not found');
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('rejects an illegal move and auto-merges a repeated one', async () => {
    const created = await mockFetch('/api/sessions', {
      method: 'POST',
      body: JSON.stringify({}),
    });
    const session = (await created.json()) as SessionResponse;
    const id = session.session_id;

    const illegal = await mockFetch(`/api/sessions/${id}/play`, {
      method: 'POST',
      body: JSON.stringify({ node_id: 0, uci: 'e2e5' }),
    });
    expect(illegal.status).toBe(400);
    expect(await illegal.json()).toEqual({ error: 'illegal move' });

    const first = await mockFetch(`/api/sessions/${id}/play`, {
      method: 'POST',
      body: JSON.stringify({ node_id: 0, uci: 'e2e4' }),
    });
    const firstBody = (await first.json()) as {
      node_id: number;
      created: boolean;
      tree: SessionResponse['tree'];
    };
    expect(firstBody.created).toBe(true);
    // A move played on the board is named the same way a PGN move is.
    expect(deepestOpening(firstBody.tree, firstBody.node_id)).toMatchObject({ eco: 'B00' });

    const second = await mockFetch(`/api/sessions/${id}/play`, {
      method: 'POST',
      body: JSON.stringify({ node_id: 0, san: 'e4' }),
    });
    const secondBody = (await second.json()) as { node_id: number; created: boolean };
    expect(secondBody).toMatchObject({ node_id: firstBody.node_id, created: false });
  });

  it('streams the game sweep with progress, nodes and a done event', async () => {
    const created = await mockFetch('/api/sessions', {
      method: 'POST',
      body: JSON.stringify({ pgn: SAMPLE_PGN }),
    });
    const session = (await created.json()) as SessionResponse;
    const events: SseEvent[] = [];
    await readSseStream(
      await mockFetch(`/api/sessions/${session.session_id}/analyze-game`, {
        method: 'POST',
        body: JSON.stringify({ depth: 18 }),
      }),
      (event) => events.push(event),
    );
    expect(events.filter((e) => e.event === 'progress')).toHaveLength(33);
    expect(events.filter((e) => e.event === 'node')).toHaveLength(33);
    expect(events.at(-1)?.event).toBe('done');
  }, 20000);
});
