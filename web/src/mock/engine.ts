/**
 * A toy "engine" for mock mode.
 *
 * It is not trying to play well — it exists so that every position in mock
 * mode has *legal, plausible* candidate moves and a monotone-ish evaluation,
 * without hand-writing analysis for 33 plies. Hand-written analysis for the
 * moves that matter lives in `fixtures.ts` and overrides everything here.
 */
import { Chess, type Move } from 'chess.js';
import type {
  AnalysisContext,
  Candidate,
  Classification,
  Counterfactual,
  GameTree,
  Node,
  PlayedMove,
  PositionAnalysis,
  Score,
} from '../api/types.ts';
import { SCRIPTED_MATES, SCRIPTED_MOVES, openingForPath, type ScriptedMove } from './fixtures.ts';

const PIECE_VALUE: Record<string, number> = { p: 100, n: 320, b: 330, r: 500, q: 900, k: 0 };

export function winProbFromCp(cp: number): number {
  return 1 / (1 + Math.exp(-0.00368208 * cp));
}

export function scoreToWinProb(score: Score): number {
  if (score.kind === 'mate') return score.value > 0 ? 1 : 0;
  return winProbFromCp(score.value);
}

/** Lichess's accuracy formula (DESIGN §8.1). */
export function accuracyOf(before: number, after: number): number {
  const value = 103.1668 * Math.exp(-0.04354 * ((before - after) * 100)) - 3.1669;
  return Math.max(0, Math.min(100, Math.round(value)));
}

/** Static evaluation in centipawns, from the side to move's point of view. */
function evaluate(chess: Chess): number {
  let white = 0;
  let black = 0;
  for (const row of chess.board()) {
    for (const square of row) {
      if (!square) continue;
      const value = PIECE_VALUE[square.type] ?? 0;
      if (square.color === 'w') white += value;
      else black += value;
    }
  }
  const material = white - black; // white's point of view
  // Mobility as a difference, not "moves the side to move has" — otherwise
  // every position looks bad for whoever just moved and the bar never settles.
  const whitePov = material + 2 * (mobilityOf(chess, 'w') - mobilityOf(chess, 'b'));
  return chess.turn() === 'w' ? whitePov : -whitePov;
}

function mobilityOf(chess: Chess, color: 'w' | 'b'): number {
  if (chess.turn() === color) return chess.moves().length;
  try {
    const flipped = new Chess(chess.fen());
    flipped.setTurn(color);
    return flipped.moves().length;
  } catch {
    return 0;
  }
}

const CENTRE = new Set(['d4', 'e4', 'd5', 'e5']);

/** Deterministic tie-breaker so the same position always ranks the same way. */
function jitter(key: string): number {
  let h = 2166136261;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 17) - 8;
}

function moveHeuristic(chess: Chess, move: Move): number {
  let score = jitter(chess.fen() + move.san);
  if (move.captured) score += 120 + (PIECE_VALUE[move.captured] ?? 0) / 10;
  if (move.san.includes('#')) score += 5000;
  if (move.san.includes('+')) score += 45;
  if (move.promotion) score += 300;
  if (move.san.startsWith('O-O')) score += 60;
  if (CENTRE.has(move.to)) score += 25;
  if ((move.piece === 'n' || move.piece === 'b') && /[18]$/.test(move.from)) score += 30;
  if (move.piece === 'q' && chess.moveNumber() < 8) score -= 40;
  return score;
}

function rankedMoves(chess: Chess): Move[] {
  return chess
    .moves({ verbose: true })
    .map((move) => ({ move, key: moveHeuristic(chess, move) }))
    .sort((a, b) => b.key - a.key)
    .map((entry) => entry.move);
}

/**
 * Static evaluation is useless in the middle of an exchange — it reads "a pawn
 * up" right before the recapture, so half the game would come out as blunders.
 * A small quiescence search (stand-pat plus captures, negamax) settles the
 * exchanges and is what makes the mock's numbers behave like an engine's.
 */
function quiescentEval(fen: string, depth = 4): number {
  let chess: Chess;
  try {
    chess = new Chess(fen);
  } catch {
    return 0;
  }
  return qsearch(chess, depth);
}

function qsearch(chess: Chess, depth: number): number {
  if (chess.isCheckmate()) return -30000;
  if (chess.isGameOver()) return 0;
  const standPat = evaluate(chess);
  if (depth === 0) return standPat;

  const captures = chess
    .moves({ verbose: true })
    .filter((move) => move.captured)
    .sort(
      (a, b) =>
        (PIECE_VALUE[b.captured!] ?? 0) -
        (PIECE_VALUE[b.piece] ?? 0) / 10 -
        ((PIECE_VALUE[a.captured!] ?? 0) - (PIECE_VALUE[a.piece] ?? 0) / 10),
    )
    .slice(0, 6);

  let best = standPat;
  for (const capture of captures) {
    chess.move(capture.san);
    const score = -qsearch(chess, depth - 1);
    chess.undo();
    if (score > best) best = score;
  }
  return best;
}

function principalVariation(fen: string, first: Move, plies: number): string[] {
  const chess = new Chess(fen);
  chess.move(first.san);
  const pv = [first.san];
  for (let i = 1; i < plies; i++) {
    const next = rankedMoves(chess)[0];
    if (!next) break;
    chess.move(next.san);
    pv.push(next.san);
  }
  return pv;
}

/** MultiPV for a position: the top `count` legal moves with scores and lines. */
export function candidatesFor(fen: string, count = 3): Candidate[] {
  let chess: Chess;
  try {
    chess = new Chess(fen);
  } catch {
    return [];
  }
  const moves = rankedMoves(chess).slice(0, count);
  return moves.map((move) => {
    const after = new Chess(fen);
    after.move(move.san);
    let score: Score;
    if (after.isCheckmate()) score = { kind: 'mate', value: 1 };
    // Negate: the evaluation of `after` is from the opponent's point of view.
    else score = { kind: 'cp', value: -quiescentEval(after.fen()) };
    return {
      san: move.san,
      uci: `${move.from}${move.to}${move.promotion ?? ''}`,
      score,
      win_prob: scoreToWinProb(score),
      pv: principalVariation(fen, move, 6),
    };
  });
}

/** DESIGN §8.3 thresholds, minus Book / Great, which are scripted. */
function classify(delta: number, isBest: boolean): Classification {
  if (delta <= -0.3) return 'blunder';
  if (delta <= -0.2) return 'mistake';
  if (delta <= -0.1) return 'inaccuracy';
  if (isBest) return 'best';
  if (delta > -0.02) return 'excellent';
  return 'good';
}

export function findScripted(nodeId: number): ScriptedMove | undefined {
  return SCRIPTED_MOVES.find((entry) => entry.ply === nodeId);
}

function nodeById(tree: GameTree, id: number): Node | undefined {
  return tree.nodes.find((node) => node.id === id);
}

function staticDiff(parentFen: string, fen: string) {
  const before = new Chess(parentFen);
  const after = new Chess(fen);
  const materialOf = (chess: Chess) => {
    let value = 0;
    for (const row of chess.board())
      for (const square of row)
        if (square)
          value += (square.color === 'w' ? 1 : -1) * ((PIECE_VALUE[square.type] ?? 0) / 100);
    return value;
  };
  return {
    material: materialOf(after) - materialOf(before),
    hanging_pieces: [] as string[],
    mobility: { white: before.moves().length, black: after.moves().length },
  };
}

/** Build the `PositionAnalysis` a mock `/analyze` or sweep returns for a node. */
export function analysisFor(
  tree: GameTree,
  nodeId: number,
  explanations: Record<string, string> = {},
): PositionAnalysis | null {
  const node = nodeById(tree, nodeId);
  if (!node) return null;

  const candidates = candidatesWithScriptedMate(nodeId, node.fen);

  const parent = node.parent === null ? null : nodeById(tree, node.parent);
  let context: AnalysisContext | null = null;

  if (parent && node.san && node.uci) {
    const scripted = findScripted(nodeId);
    const parentCandidates = scripted
      ? scripted.candidates.map((candidate) => ({
          san: candidate.san,
          uci: sanToUci(parent.fen, candidate.san) ?? '',
          score: candidate.score,
          win_prob: candidate.win_prob,
          pv: candidate.pv,
        }))
      : candidatesWithScriptedMate(parent.id, parent.fen);

    const winProbBefore = scripted
      ? scripted.win_prob_before
      : (parentCandidates[0]?.win_prob ?? 0.5);
    // No candidates means the game ended on this move: mate is a win for the
    // player who just moved, anything else is a draw.
    const stmWinProb =
      candidates[0]?.win_prob ?? (new Chess(node.fen).isCheckmate() ? 0 : 0.5);
    const winProbAfter = scripted ? scripted.win_prob_after : 1 - stmWinProb;
    const delta = winProbAfter - winProbBefore;
    const playedRank = scripted
      ? scripted.played_rank
      : (() => {
          const index = parentCandidates.findIndex((candidate) => candidate.san === node.san);
          return index === -1 ? null : index;
        })();

    const played: PlayedMove = {
      san: node.san,
      uci: node.uci,
      win_prob_before: round(winProbBefore),
      win_prob_after: round(winProbAfter),
      delta: round(delta),
      classification: scripted ? scripted.classification : classify(delta, playedRank === 0),
      accuracy: scripted ? scripted.accuracy : accuracyOf(winProbBefore, winProbAfter),
    };

    context = {
      candidates: parentCandidates,
      played_rank: playedRank,
      played,
      counterfactual: counterfactualFor(node, parent, scripted),
      static_diff: staticDiff(parent.fen, node.fen),
    };
  }

  return { fen: node.fen, depth: 18, candidates, context, explanations };
}

/** Candidates for a position, with a hand-written mate score where we have one. */
function candidatesWithScriptedMate(nodeId: number, fen: string): Candidate[] {
  const candidates = candidatesFor(fen);
  const mateScore = SCRIPTED_MATES[nodeId];
  const first = candidates[0];
  if (!mateScore || !first) return candidates;
  return [
    { ...first, score: mateScore, win_prob: scoreToWinProb(mateScore) },
    ...candidates.slice(1),
  ];
}

function counterfactualFor(
  node: Node,
  parent: Node,
  scripted: ScriptedMove | undefined,
): Counterfactual | null {
  if (scripted) {
    if (!scripted.counterfactual) return null;
    const { kind, from, pv, motifs } = scripted.counterfactual;
    return { kind, start_fen: from === 'node' ? node.fen : parent.fen, pv, motifs };
  }
  // Generic: the opponent's best continuation from the position just created.
  const best = candidatesFor(node.fen, 1)[0];
  if (!best) return null;
  return { kind: 'refutation', start_fen: node.fen, pv: best.pv, motifs: [] };
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}

export function sanToUci(fen: string, san: string): string | null {
  try {
    const chess = new Chess(fen);
    const move = chess.move(san);
    return `${move.from}${move.to}${move.promotion ?? ''}`;
  } catch {
    return null;
  }
}

/* ---------- tree construction ---------- */

export interface BuiltSession {
  tree: GameTree;
  headers: Record<string, string>;
}

const START_FEN = new Chess().fen();

/** The SAN moves that lead from the root down to `nodeId`, root move first. */
function sanPathTo(tree: GameTree, nodeId: number): string[] {
  const path: string[] = [];
  let walker = nodeById(tree, nodeId);
  while (walker && walker.san !== null) {
    path.push(walker.san);
    walker = walker.parent === null ? undefined : nodeById(tree, walker.parent);
  }
  return path.reverse();
}

/**
 * The mock's ECO lookup. Its table is keyed by moves from the standard start,
 * so a session opened from an arbitrary FEN is simply unnamed throughout —
 * which is a state the UI has to handle anyway.
 */
function openingFor(tree: GameTree, nodeId: number): ReturnType<typeof openingForPath> {
  if (nodeById(tree, tree.root)?.fen !== START_FEN) return null;
  return openingForPath(sanPathTo(tree, nodeId));
}

export function buildSession(input: { pgn?: string; fen?: string }): BuiltSession {
  if (input.pgn && input.pgn.trim()) {
    const chess = new Chess();
    chess.loadPgn(input.pgn);
    const headers = chess.getHeaders() as Record<string, string>;
    const history = chess.history({ verbose: true });
    const start = headers.FEN ?? START_FEN;
    const nodes: Node[] = [
      {
        id: 0,
        parent: null,
        children: [],
        san: null,
        uci: null,
        fen: start,
        analysis: null,
        opening: null,
      },
    ];
    const sanPath: string[] = [];
    for (const move of history) {
      const id = nodes.length;
      const parent = nodes[id - 1]!;
      parent.children.push(id);
      sanPath.push(move.san);
      nodes.push({
        id,
        parent: parent.id,
        children: [],
        san: move.san,
        uci: `${move.from}${move.to}${move.promotion ?? ''}`,
        fen: move.after,
        analysis: null,
        opening: start === START_FEN ? openingForPath(sanPath) : null,
      });
    }
    return { tree: { nodes, root: 0 }, headers };
  }

  const fen = input.fen?.trim() ? input.fen.trim() : START_FEN;
  new Chess(fen); // throws on an invalid FEN, which the caller turns into a 400
  return {
    tree: {
      nodes: [
        {
          id: 0,
          parent: null,
          children: [],
          san: null,
          uci: null,
          fen,
          analysis: null,
          opening: null,
        },
      ],
      root: 0,
    },
    headers: {},
  };
}

/** `POST /play` semantics, including auto-merge of an existing child. */
export function playOnTree(
  tree: GameTree,
  nodeId: number,
  move: { uci?: string; san?: string },
): { node_id: number; created: boolean } | null {
  const node = nodeById(tree, nodeId);
  if (!node) return null;
  const chess = new Chess(node.fen);
  let result;
  try {
    result = move.uci
      ? chess.move({
          from: move.uci.slice(0, 2),
          to: move.uci.slice(2, 4),
          promotion: move.uci[4] ?? 'q',
        })
      : chess.move(move.san!);
  } catch {
    return null;
  }
  if (!result) return null;

  const uci = `${result.from}${result.to}${result.promotion ?? ''}`;
  const existing = node.children
    .map((id) => nodeById(tree, id))
    .find((child) => child?.uci === uci);
  if (existing) return { node_id: existing.id, created: false };

  const id = tree.nodes.length;
  tree.nodes.push({
    id,
    parent: node.id,
    children: [],
    san: result.san,
    uci,
    fen: chess.fen(),
    analysis: null,
    opening: null,
  });
  node.children.push(id);
  tree.nodes[id]!.opening = openingFor(tree, id);
  return { node_id: id, created: true };
}

export function mainlineIds(tree: GameTree): number[] {
  const ids: number[] = [];
  let current = nodeById(tree, tree.root);
  while (current) {
    ids.push(current.id);
    const nextId = current.children[0];
    if (nextId === undefined) break;
    current = nodeById(tree, nextId);
  }
  return ids;
}
