/**
 * The wire types from `docs/API.md`. This file is the frontend half of that
 * contract — it must not drift from it. Shapes that API.md defers to the Rust
 * source (`AnalysisContext`, `StaticDiff`, `StrategicOutlook`) are typed
 * defensively: everything the UI does not strictly need is optional, so an
 * extra or missing field in the server's serde output cannot break rendering.
 */

export type Score =
  | { kind: 'cp'; value: number }
  | { kind: 'mate'; value: number }; // positive: the side to move is mating

export type Classification =
  | 'book'
  | 'great'
  | 'best'
  | 'excellent'
  | 'good'
  | 'inaccuracy'
  | 'mistake'
  | 'blunder'
  | 'miss';

export interface Candidate {
  san: string;
  uci: string;
  score: Score;
  win_prob: number; // 0..1, from the side to move's point of view
  pv: string[]; // SAN
}

export interface PlayedMove {
  san: string;
  uci: string;
  win_prob_before: number;
  win_prob_after: number;
  delta: number; // negative means the move made things worse
  classification: Classification;
  accuracy: number; // 0..100
}

export type CounterfactualKind = 'refutation' | 'alternative_collapse';

export interface Counterfactual {
  kind: CounterfactualKind;
  start_fen: string; // position the replay starts from
  pv: string[]; // the line to animate on the board (SAN)
  motifs: Motif[];
}

export type Motif =
  | { kind: 'fork'; from: string; attacker: string; targets: string[] }
  | { kind: 'pin'; attacker: string; pinned: string; behind: string }
  | { kind: 'skewer'; attacker: string; front: string; behind: string }
  | { kind: 'discovered_attack'; moved_from: string; revealed: string; targets: string[] }
  | { kind: 'hanging'; square: string; role: string; see: number }
  | { kind: 'back_rank'; attacker: string; king: string };

/** DESIGN.md §8.4 — only the parts the UI surfaces are typed. */
export interface StaticDiff {
  material?: number;
  hanging_pieces?: string[];
  center_control?: { white: number; black: number };
  mobility?: { white: number; black: number };
  [key: string]: unknown;
}

/** DESIGN.md §8.6. */
export interface StrategicOutlook {
  long_pv?: string[];
  terminal_fen?: string;
  [key: string]: unknown;
}

/** DESIGN.md §8. `position` is left opaque — the UI uses `PositionAnalysis.fen`. */
export interface AnalysisContext {
  position?: unknown;
  candidates?: Candidate[];
  played_rank?: number | null;
  played: PlayedMove;
  counterfactual: Counterfactual | null;
  static_diff?: StaticDiff;
  outlook?: StrategicOutlook | null;
}

export interface PositionAnalysis {
  fen: string;
  depth: number;
  candidates: Candidate[];
  context: AnalysisContext | null; // null at the root
  explanations: Record<string, string>; // keyed by language code
}

/**
 * The named opening a position belongs to, from an ECO table embedded in the
 * server binary. Offline and always present, so there is no request, no loading
 * state and no failure mode for the UI to handle.
 *
 * It is a *name for the position*, and nothing more. It says nothing about
 * whether the move is still theory — that judgement needs the game counts of an
 * opening explorer, which an ECO table does not carry — so it must never be
 * rendered as "book" / "out of book". A line simply stops having a name at some
 * depth, which is not the same thing as leaving theory.
 */
export interface OpeningInfo {
  eco: string; // "C65"
  name: string; // "Ruy Lopez: Berlin Defense"
  matched_plies: number; // plies from the root to this node
}

export interface Node {
  id: number;
  parent: number | null;
  children: number[]; // [0] is the mainline
  san: string | null; // null at the root
  uci: string | null;
  fen: string;
  analysis: PositionAnalysis | null;
  /** Null once the game leaves every named line the ECO table knows. */
  opening: OpeningInfo | null;
}

export interface GameTree {
  nodes: Node[];
  root: number;
}

export interface SessionResponse {
  session_id: string;
  tree: GameTree;
  headers: Record<string, string>;
}

export interface PlayResponse {
  node_id: number;
  created: boolean;
  tree: GameTree;
}

export interface HealthResponse {
  ok: boolean;
  engine: string | null;
  stockfish_path?: string | null;
}

export interface LanguageOption {
  code: string;
  name: string;
}

export interface LanguagesResponse {
  languages: LanguageOption[];
  default: string;
}

/* ---- SSE event payloads ---- */

export interface SweepProgressEvent {
  node_id: number;
  done: number;
  total: number;
}

export interface SweepDoneEvent {
  total: number;
}

export interface ExplainDeltaEvent {
  text: string;
}

export interface ExplainDoneEvent {
  text: string;
  lang: string;
  model: string;
  cached: boolean;
}

export interface ToolEvent {
  name: string;
  input: unknown;
}

export interface ErrorEvent {
  error: string;
}
