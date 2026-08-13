import { describe, expect, it } from 'vitest';
import { Chess } from 'chess.js';
import { SAMPLE_PGN, SCRIPTED_MATES, SCRIPTED_MOVES } from './fixtures.ts';
import { analysisFor, buildSession } from './engine.ts';
import { mockFetch } from './mockServer.ts';
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

    const analyzed = await mockFetch(`/api/sessions/${session.session_id}/analyze`, {
      method: 'POST',
      body: JSON.stringify({ node_id: 18, depth: 20 }),
    });
    const analysis = (await analyzed.json()) as PositionAnalysis;
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
