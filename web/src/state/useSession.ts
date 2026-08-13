import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Key } from 'chessground/types';
import { analyze, createSession, getSession, play } from '../api/client.ts';
import { runExclusive } from '../api/engineQueue.ts';
import { ApiError } from '../api/transport.ts';
import type { GameTree, Node, PositionAnalysis } from '../api/types.ts';
import { playMove as playLocally, type PromotionPiece } from '../chess/rules.ts';
import { clearResumePoint, readResumePoint, writeResumePoint } from './resumeStorage.ts';

export type AnalysisStatus = 'idle' | 'loading' | 'ready' | 'error';

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

  /** The single-node analysis that is running or queued, and for which node. */
  const inFlight = useRef<{ nodeId: number; controller: AbortController } | null>(null);

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
   */
  const runAnalysis = useCallback(
    (nodeId: number, options: { force?: boolean } = {}) => {
      if (!sessionId) return;
      inFlight.current?.controller.abort();
      const controller = new AbortController();
      inFlight.current = { nodeId, controller };
      setAnalysisStatus('loading');
      setAnalysisError(null);

      runExclusive(async () => {
        // By the time the queue gets here the sweep may have covered this node,
        // or the user may have moved on. Either way, spend no engine time.
        if (controller.signal.aborted) return null;
        if (!options.force && analysesRef.current.has(nodeId)) return null;
        return analyze(sessionId, { node_id: nodeId, depth }, controller.signal);
      }, controller.signal)
        .then((analysis) => {
          if (controller.signal.aborted) return;
          if (analysis) recordAnalysis(nodeId, analysis);
          setAnalysisStatus('ready');
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return;
          // A 409 "cancelled" means a newer analyze superseded this one; the
          // newer request owns the UI state, so stay quiet.
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

  return {
    status,
    openError,
    sessionId,
    tree,
    headers,
    currentId,
    currentNode,
    analyses,
    analysis: analyses.get(currentId) ?? null,
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
