import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Key } from 'chessground/types';
import { analyze, createSession, getSession, play } from '../api/client.ts';
import { runExclusive } from '../api/engineQueue.ts';
import { ApiError } from '../api/transport.ts';
import type { Candidate, GameTree, Node, PositionAnalysis } from '../api/types.ts';
import { playMove as playLocally, type PromotionPiece } from '../chess/rules.ts';
import { clearResumePoint, readResumePoint, writeResumePoint } from './resumeStorage.ts';

export type AnalysisStatus = 'idle' | 'loading' | 'ready' | 'error';

/**
 * The shortest gap between two partial results reaching the screen.
 *
 * `/analyze` sends one event per completed depth, and the engine does not
 * complete them at an even rate: measured over the 73 positions of the two test
 * games at depth 12 / MultiPV 5, the depths from the streaming floor arrive at
 * roughly 6, 9, 22, 47, 94, 163 and 289 ms. The first three land inside 22 ms of
 * each other — three different pictures inside one and a half display frames,
 * which is a flicker rather than progress — while the last few are 60 to 130 ms
 * apart, which reads as the answer sharpening.
 *
 * And the pictures really are different. The board's arrows are clustered by win
 * probability (`ui/arrows.ts`), so a small score change can reshuffle both the
 * order and the stroke weights: the drawn top-3 changes on 60–92% of depth
 * transitions, all the way to depth 12. Decomposed, that is mostly *which* moves
 * are in the top three (29–62% of transitions) rather than their weights
 * (5–16%), so damping the weight ramp would be treating the wrong thing — the
 * ranks below the best move are genuinely unsettled at every depth, and no
 * amount of smoothing makes them settle.
 *
 * What can be fixed is the cadence. A plain leading-edge throttle — draw the
 * first partial immediately, then ignore any that arrives less than this long
 * after the last one drawn — collapses the opening burst into one frame and
 * leaves the well-spaced later updates alone. Measured effect at a 100 ms
 * window: 7.0 partials drawn per search falls to 2.5, and visible changes to the
 * arrows fall from 4.4 to 1.5, with **no cost to the time-to-first-arrow**,
 * which is what the whole feature is for.
 *
 * There is deliberately no trailing timer to flush a partial the throttle held
 * back. Nothing is lost by dropping one: the final `analysis` event is never
 * throttled and always redraws, so the worst case is that the arrows sit at
 * depth 10 for the last stretch instead of stepping through 11 — and a frame
 * the eye never resolves is not worth a timer that can fire after the node has
 * changed.
 */
const PARTIAL_MIN_INTERVAL_MS = 100;

/** The engine's ranking mid-search, and the node it belongs to. */
interface PartialAnalysis {
  nodeId: number;
  depth: number;
  candidates: Candidate[];
}

export interface SessionState {
  status: 'empty' | 'opening' | 'ready';
  openError: string | null;
  sessionId: string | null;
  tree: GameTree | null;
  headers: Record<string, string>;
  currentId: number;
  currentNode: Node | null;
  analyses: Map<number, PositionAnalysis>;
  analysis: PositionAnalysis | null;
  analysisStatus: AnalysisStatus;
  analysisError: string | null;
  open: (input: { pgn: string } | { fen?: string }) => Promise<void>;
  reset: () => void;
  goto: (nodeId: number) => void;
  step: (direction: 'first' | 'prev' | 'next' | 'last') => void;
  playMove: (from: Key, to: Key, promotion?: PromotionPiece) => Promise<void>;
  playSan: (san: string) => Promise<void>;
  analyzeCurrent: () => void;
  recordAnalysis: (nodeId: number, analysis: PositionAnalysis) => void;
}

export interface SessionInput {
  /**
   * Search depth for single-node analysis, from the user's setting. Changing it
   * governs the *next* analysis: results already on the tree are kept, since
   * re-searching everything the moment a select changes is not what anyone
   * means by adjusting a setting.
   */
  depth: number;
}

export function useSession({ depth }: SessionInput): SessionState {
  // Resuming is decided before the first paint, so a reload does not flash the
  // import screen on its way back to the game.
  const [status, setStatus] = useState<SessionState['status']>(() =>
    readResumePoint() ? 'opening' : 'empty',
  );
  const [openError, setOpenError] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [tree, setTree] = useState<GameTree | null>(null);
  const [headers, setHeaders] = useState<Record<string, string>>({});
  const [currentId, setCurrentId] = useState(0);
  const [received, setReceived] = useState<Map<number, PositionAnalysis>>(new Map());
  const [analysisStatus, setAnalysisStatus] = useState<AnalysisStatus>('idle');
  const [analysisError, setAnalysisError] = useState<string | null>(null);

  /**
   * The ranking of the search that is running right now, if any.
   *
   * Deliberately **not** merged into `received`. `analyses` is the map of
   * finished analyses: the move list annotates from it, the mistake navigation
   * searches it, and the effect below treats a hit in it as "this node is done"
   * and cancels the request. A partial in there would abort its own search a
   * fifth of the way through and leave a depth-6 result on the tree forever.
   *
   * So it lives here, alone, and is overlaid onto the selected node's analysis
   * only when there is no finished one — see `analysis` in the returned state.
   */
  const [partial, setPartial] = useState<PartialAnalysis | null>(null);

  /** The single-node analysis that is running or queued, and for which node. */
  const inFlight = useRef<{ nodeId: number; controller: AbortController } | null>(null);
  /** When the last partial was allowed through, for `PARTIAL_MIN_INTERVAL_MS`. */
  const lastPartialAt = useRef(0);

  const nodesById = useMemo(() => {
    const map = new Map<number, Node>();
    for (const node of tree?.nodes ?? []) map.set(node.id, node);
    return map;
  }, [tree]);

  const currentNode = nodesById.get(currentId) ?? null;

  /**
   * Analyses come from two places and both are authoritative.
   *
   * The server keeps every analysis it has ever produced on the tree
   * (`tree.nodes[*].analysis`), and hands the whole tree back from
   * `GET /sessions/{id}` and from every `POST /play`. So the tree is the
   * baseline: re-opening an analysed session, or branching mid-game, shows the
   * annotations the server already has rather than a bare move list.
   *
   * On top of that sits what this client has received since — the sweep's SSE
   * `node` events and single-node `analyze` responses — which arrive before the
   * tree is next refetched. The deeper of the two wins, so a fresh result is
   * never replaced by a staler one and a `/play` response cannot wipe out
   * analysis the client already had.
   */
  const analyses = useMemo(() => {
    const merged = new Map<number, PositionAnalysis>();
    for (const node of tree?.nodes ?? []) {
      if (node.analysis) merged.set(node.id, node.analysis);
    }
    for (const [nodeId, analysis] of received) {
      const existing = merged.get(nodeId);
      if (!existing || analysis.depth >= existing.depth) merged.set(nodeId, analysis);
    }
    return merged;
  }, [tree, received]);

  // Read inside queued work, which may start long after the render that queued it.
  const analysesRef = useRef(analyses);
  analysesRef.current = analyses;

  const recordAnalysis = useCallback((nodeId: number, analysis: PositionAnalysis) => {
    setReceived((previous) => {
      const next = new Map(previous);
      next.set(nodeId, analysis);
      return next;
    });
  }, []);

  const open = useCallback(async (input: { pgn: string } | { fen?: string }) => {
    setStatus('opening');
    setOpenError(null);
    inFlight.current?.controller.abort();
    inFlight.current = null;
    try {
      const response = await createSession(input);
      setSessionId(response.session_id);
      setTree(response.tree);
      setHeaders(response.headers ?? {});
      setReceived(new Map());
      setPartial(null);
      setCurrentId(response.tree.root);
      setStatus('ready');
      writeResumePoint({ sessionId: response.session_id, nodeId: response.tree.root });
    } catch (error) {
      setOpenError(error instanceof Error ? error.message : String(error));
      setStatus('empty');
      clearResumePoint();
    }
  }, []);

  /**
   * Reload, same game. The server keeps the session and every analysis on its
   * tree, so `GET /api/sessions/{id}` brings the annotated move list straight
   * back — nothing is re-analysed, and the sweep is not re-run (`useSweep`
   * sees an already-analysed mainline).
   *
   * A session the server no longer has is not an error worth showing: the
   * pointer is simply dropped and the import screen appears as usual.
   */
  useEffect(() => {
    const resume = readResumePoint();
    if (!resume) return;
    let cancelled = false;

    getSession(resume.sessionId)
      .then((response) => {
        if (cancelled) return;
        const known = response.tree.nodes.some((node) => node.id === resume.nodeId);
        setSessionId(response.session_id);
        setTree(response.tree);
        setHeaders(response.headers ?? {});
        setCurrentId(known ? resume.nodeId : response.tree.root);
        setStatus('ready');
      })
      .catch(() => {
        if (cancelled) return;
        clearResumePoint();
        setStatus('empty');
      });

    return () => {
      cancelled = true;
    };
  }, []);

  // Keep the resume point pointing at the node on screen.
  useEffect(() => {
    if (status !== 'ready' || !sessionId) return;
    writeResumePoint({ sessionId, nodeId: currentId });
  }, [status, sessionId, currentId]);

  const reset = useCallback(() => {
    inFlight.current?.controller.abort();
    inFlight.current = null;
    clearResumePoint();
    setStatus('empty');
    setSessionId(null);
    setTree(null);
    setHeaders({});
    setReceived(new Map());
    setPartial(null);
    setCurrentId(0);
    setAnalysisStatus('idle');
    setAnalysisError(null);
  }, []);

  const goto = useCallback((nodeId: number) => setCurrentId(nodeId), []);

  const step = useCallback(
    (direction: 'first' | 'prev' | 'next' | 'last') => {
      if (!tree) return;
      setCurrentId((current) => {
        const node = nodesById.get(current);
        switch (direction) {
          case 'first':
            return tree.root;
          case 'prev':
            return node?.parent ?? current;
          case 'next':
            return node?.children[0] ?? current;
          case 'last': {
            let walker = node;
            while (walker?.children[0] !== undefined) {
              const next = nodesById.get(walker.children[0]);
              if (!next) break;
              walker = next;
            }
            return walker?.id ?? current;
          }
        }
      });
    },
    [tree, nodesById],
  );

  /**
   * A move played on the board: `POST /play` then `POST /analyze` (API.md flow
   * step 4). The move is validated locally first so an illegal drag never
   * leaves the browser and the board can snap back immediately.
   */
  const playMove = useCallback(
    async (from: Key, to: Key, promotion?: PromotionPiece) => {
      if (!sessionId || !currentNode) return;
      // The promotion piece only reaches the server inside the UCI: `local.uci`
      // carries the suffix (`b7b8n`), which is exactly what `/play` takes.
      const local = playLocally(currentNode.fen, from, to, promotion);
      if (!local) return;
      try {
        const response = await play(sessionId, { node_id: currentNode.id, uci: local.uci });
        setTree(response.tree);
        setCurrentId(response.node_id);
      } catch (error) {
        setAnalysisError(error instanceof Error ? error.message : String(error));
        setAnalysisStatus('error');
      }
    },
    [sessionId, currentNode],
  );

  /** Same as `playMove`, but from a SAN the engine suggested. */
  const playSan = useCallback(
    async (san: string) => {
      if (!sessionId || !currentNode) return;
      try {
        const response = await play(sessionId, { node_id: currentNode.id, san });
        setTree(response.tree);
        setCurrentId(response.node_id);
      } catch (error) {
        setAnalysisError(error instanceof Error ? error.message : String(error));
        setAnalysisStatus('error');
      }
    },
    [sessionId, currentNode],
  );

  /**
   * Analyse one node. The request goes through the engine queue, so it waits
   * for a running whole-game sweep instead of cancelling it (see
   * `api/engineQueue.ts`) — that is what lets the user click around the board
   * while the sweep runs.
   *
   * The response is a stream. Each `candidates` event updates the arrows, the
   * candidate list and the eval bar; the classification, the accuracy and the
   * counterfactual arrive only with the final result, because they are a
   * judgement rather than a measurement and one that changes its mind on screen
   * is worse than one that takes 300 ms (API.md, `POST .../analyze`).
   */
  const runAnalysis = useCallback(
    (nodeId: number, options: { force?: boolean } = {}) => {
      if (!sessionId) return;
      inFlight.current?.controller.abort();
      const controller = new AbortController();
      inFlight.current = { nodeId, controller };
      setAnalysisStatus('loading');
      setAnalysisError(null);
      // Whatever the previous search had drawn belonged to the previous search.
      setPartial(null);
      lastPartialAt.current = 0;

      runExclusive(async () => {
        // By the time the queue gets here the sweep may have covered this node,
        // or the user may have moved on. Either way, spend no engine time.
        if (controller.signal.aborted) return null;
        if (!options.force && analysesRef.current.has(nodeId)) return null;
        return analyze(
          sessionId,
          { node_id: nodeId, depth },
          {
            onCandidates: (event) => {
              // The stream outlives the request only in the sense that events
              // can still be in flight when the user moves on; a partial for a
              // node nobody is looking at must not be drawn.
              if (controller.signal.aborted) return;
              const now = Date.now();
              if (now - lastPartialAt.current < PARTIAL_MIN_INTERVAL_MS) return;
              lastPartialAt.current = now;
              setPartial({ nodeId, depth: event.depth, candidates: event.candidates });
            },
          },
          controller.signal,
        );
      }, controller.signal)
        .then((analysis) => {
          if (controller.signal.aborted) return;
          if (analysis) recordAnalysis(nodeId, analysis);
          // The finished analysis supersedes the partial for this node, and the
          // overlay below would hide it if the partial stayed.
          setPartial((current) => (current?.nodeId === nodeId ? null : current));
          setAnalysisStatus('ready');
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return;
          setPartial((current) => (current?.nodeId === nodeId ? null : current));
          // "cancelled" means a newer analyze superseded this one; the newer
          // request owns the UI state, so stay quiet. It reaches us as a 409
          // whether the server refused before the stream opened or stopped
          // midway through it — `client.ts` normalises the two shapes.
          if (error instanceof ApiError && error.isCancelled) return;
          if (error instanceof DOMException && error.name === 'AbortError') return;
          setAnalysisError(error instanceof Error ? error.message : String(error));
          setAnalysisStatus('error');
        });
    },
    [sessionId, depth, recordAnalysis],
  );

  /**
   * Analyse whatever node is selected, unless we already have it — from the
   * tree, from the sweep, or from an earlier request. `analyses` is a dependency
   * on purpose: when the sweep reaches the node the user is sitting on, that is
   * the result, and the pending single-node request for it can be dropped.
   */
  useEffect(() => {
    if (!sessionId || !currentNode) return;
    if (analyses.has(currentNode.id)) {
      setAnalysisStatus('ready');
      if (inFlight.current?.nodeId === currentNode.id) {
        inFlight.current.controller.abort();
        inFlight.current = null;
      }
      return;
    }
    // Already queued for this node: waiting is not the same as doing nothing.
    if (inFlight.current?.nodeId === currentNode.id) return;
    runAnalysis(currentNode.id);
  }, [sessionId, currentNode, analyses, runAnalysis]);

  const analyzeCurrent = useCallback(() => {
    if (currentNode) runAnalysis(currentNode.id, { force: true });
  }, [currentNode, runAnalysis]);

  /**
   * What the panel and the board describe: the finished analysis for the
   * selected node, or — while its search is still running — the ranking so far,
   * dressed as a `PositionAnalysis` with **no context**.
   *
   * `context: null` is the whole of the design rule, expressed in one field. It
   * is the shape the server already sends for the root node, so every consumer
   * handles it: the classification badge, the accuracy figure, the delta, the
   * rank line, the counterfactual block and the "explain this move" button are
   * all gated on `context.played` and simply do not render. What does render is
   * everything a ranking can honestly support — the candidate list, the ranked
   * arrows and the eval bar — plus the depth in the panel header, which is read
   * off this object and so is always the depth of the numbers beside it.
   *
   * Keying on `nodeId` is what stops a stale verdict, or a stale ranking, from
   * appearing under a node it does not belong to: a partial for another node is
   * not shown at all, and the selected node falls back to `null` until its own
   * first partial lands.
   */
  const partialAnalysis = useMemo<PositionAnalysis | null>(() => {
    if (!partial || partial.nodeId !== currentId || !currentNode) return null;
    return {
      fen: currentNode.fen,
      depth: partial.depth,
      candidates: partial.candidates,
      context: null,
      explanations: {},
    };
  }, [partial, currentId, currentNode]);

  return {
    status,
    openError,
    sessionId,
    tree,
    headers,
    currentId,
    currentNode,
    analyses,
    analysis: analyses.get(currentId) ?? partialAnalysis,
    analysisStatus,
    analysisError,
    open,
    reset,
    goto,
    step,
    playMove,
    playSan,
    analyzeCurrent,
    recordAnalysis,
  };
}
