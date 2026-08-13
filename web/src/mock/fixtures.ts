import type { Classification, Motif, OpeningInfo, Score } from '../api/types.ts';
import { SAMPLE_PGN } from '../ui/samplePgn.ts';

/**
 * Fixture data for `VITE_MOCK=1`.
 *
 * The game is Morphy's Opera Game — short, famous, and it contains everything
 * the UI has to render: a quiet inaccuracy, an outright blunder, two piece
 * sacrifices that were each the only move that held, and a forced mate at the
 * end so the `{kind:"mate"}` branch of `Score` gets exercised.
 *
 * Only the interesting moves are scripted; every other position is synthesised
 * by `mock/engine.ts` from the actual legal moves, so all SAN in the mock is
 * real and the board can always play it.
 *
 * The PGN itself lives in `ui/samplePgn.ts`: the import screen offers it as the
 * sample game, and production code must not import anything from `mock/`.
 */

export { SAMPLE_PGN };

export interface ScriptedCandidate {
  san: string;
  score: Score;
  win_prob: number;
  pv: string[];
}

export interface ScriptedCounterfactual {
  kind: 'refutation' | 'alternative_collapse';
  /**
   * Which position the replay starts from. A refutation starts from the
   * position the played move created (the opponent now punishes it); an
   * alternative collapse starts from the position *before* it, because the
   * point is to play a different move instead.
   */
  from: 'node' | 'parent';
  pv: string[];
  motifs: Motif[];
}

export interface ScriptedMove {
  /** Ply number, which is also the mainline node id (root = 0). */
  ply: number;
  classification: Classification;
  win_prob_before: number;
  win_prob_after: number;
  accuracy: number;
  played_rank: number | null;
  /** MultiPV of the position *before* the move — `played_rank` indexes this. */
  candidates: ScriptedCandidate[];
  counterfactual: ScriptedCounterfactual | null;
  explanations: Record<string, string>;
}

const cp = (value: number): Score => ({ kind: 'cp', value });
const mate = (value: number): Score => ({ kind: 'mate', value });

export const SCRIPTED_MOVES: ScriptedMove[] = [
  {
    // 3... Bg4 — natural-looking pin, but it hands over the bishop pair.
    ply: 6,
    classification: 'inaccuracy',
    win_prob_before: 0.48,
    win_prob_after: 0.37,
    accuracy: 71,
    played_rank: 4,
    candidates: [
      { san: 'exd4', score: cp(18), win_prob: 0.516, pv: ['exd4', 'Qxd4', 'Nc6', 'Bb5', 'Bd7'] },
      { san: 'Nf6', score: cp(24), win_prob: 0.522, pv: ['Nf6', 'dxe5', 'Nxe4', 'Bd3', 'd5'] },
      { san: 'Nd7', score: cp(41), win_prob: 0.537, pv: ['Nd7', 'Bc4', 'c6', 'O-O', 'Be7'] },
      { san: 'Bg4', score: cp(96), win_prob: 0.587, pv: ['Bg4', 'dxe5', 'Bxf3', 'Qxf3', 'dxe5'] },
    ],
    counterfactual: {
      kind: 'refutation',
      from: 'node',
      pv: ['dxe5', 'Bxf3', 'Qxf3', 'dxe5', 'Bc4'],
      motifs: [{ kind: 'hanging', square: 'g4', role: 'bishop', see: -30 }],
    },
    explanations: {
      en: 'Pinning the knight is the move everyone plays here, and it is already a small concession. The centre is still unresolved, so White simply takes on e5 first: dxe5 attacks the d6 pawn and forces Bxf3, because Black cannot afford to lose a second tempo. After Qxf3 dxe5 White has the two bishops and the more comfortable queen, while Black has traded the piece that was doing the pinning. exd4 or the quiet Nf6 keeps the position balanced. It is a small loss, not a mistake — but it is the first step in the direction the whole game then takes.',
      ja: 'ナイトをピンで縛る自然な手だが、すでにわずかな譲歩になっている。中央がまだ決着していないため、白は先に dxe5 と取れる。これは d6 のポーンに当たり、黒は二手損を避けるために Bxf3 を強いられる。Qxf3 dxe5 のあと、白は二枚のビショップと働きの良いクイーンを得て、黒はピンをかけていた駒を手放してしまった。exd4 か落ち着いた Nf6 なら互角のままだった。ミスというほどではないが、この一局全体の流れがここから始まっている。',
    },
  },
  {
    // 9... b5 — the losing move. The refutation replay is the whole point.
    ply: 18,
    classification: 'blunder',
    win_prob_before: 0.31,
    win_prob_after: 0.06,
    accuracy: 22,
    played_rank: null,
    candidates: [
      { san: 'Qc7', score: cp(148), win_prob: 0.647, pv: ['Qc7', 'O-O-O', 'Nbd7', 'Rhe1', 'Be7'] },
      { san: 'Nbd7', score: cp(171), win_prob: 0.66, pv: ['Nbd7', 'O-O-O', 'Rd8', 'Rhe1', 'h6'] },
      { san: 'b6', score: cp(210), win_prob: 0.687, pv: ['b6', 'O-O-O', 'Nbd7', 'Rhe1', 'Qe6'] },
    ],
    counterfactual: {
      kind: 'refutation',
      from: 'node',
      pv: ['Nxb5', 'cxb5', 'Bxb5+', 'Nbd7', 'O-O-O', 'Rd8', 'Rxd7', 'Rxd7', 'Rd1'],
      motifs: [
        { kind: 'hanging', square: 'b5', role: 'pawn', see: -100 },
        { kind: 'pin', attacker: 'd1', pinned: 'd7', behind: 'd8' },
        { kind: 'discovered_attack', moved_from: 'd1', revealed: 'b5', targets: ['d7'] },
      ],
    },
    explanations: {
      en: 'b5 loses on the spot. The pawn is not defended by anything that matters — the knight on c3 can take it immediately, and after cxb5 the bishop recaptures with check. That check is the whole problem: Bxb5+ hits a king still sitting on e8 with no way out, so Nbd7 is forced as a block, and now every White piece is pointing at d7. Castling long brings the last rook into the attack for free, and after Rxd7 Rxd7 Rd1 the knight is pinned by a rook, attacked by a bishop, and defended only by pieces that cannot move. Black is not even down material yet and the position is already lost. Qc7 or Nbd7 first, leaving the b-pawn where it stands, keeps the game going.',
      ja: 'b5 は一手で敗着になる。この歩は実質的に守られておらず、c3 のナイトがすぐに取れる。cxb5 のあとビショップが王手つきで取り返すのが問題で、Bxb5+ は e8 に残ったままの王に当たる。逃げ場がないので Nbd7 の合駒が強制され、白の全ての駒が d7 を睨む形になる。ロングキャスリングで最後のルークもタダで攻めに参加し、Rxd7 Rxd7 Rd1 と進めばナイトはルークにピンされ、ビショップに当てられ、動けない駒だけに守られている。まだ駒損すらしていないのに、局面はすでに負けている。b 歩を突かずに Qc7 か Nbd7 なら勝負は続いていた。',
    },
  },
  {
    // 10. Nxb5 — a knight for a pawn, and correct.
    ply: 19,
    classification: 'great',
    win_prob_before: 0.94,
    win_prob_after: 0.95,
    accuracy: 100,
    played_rank: 0,
    candidates: [
      {
        san: 'Nxb5',
        score: cp(612),
        win_prob: 0.902,
        pv: ['Nxb5', 'cxb5', 'Bxb5+', 'Nbd7', 'O-O-O', 'Rd8'],
      },
      { san: 'Bxf6', score: cp(121), win_prob: 0.61, pv: ['Bxf6', 'gxf6', 'Bd3', 'Nd7', 'a4', 'b4'] },
      { san: 'Bd3', score: cp(78), win_prob: 0.571, pv: ['Bd3', 'Nbd7', 'a4', 'b4', 'Nd5', 'Nxd5'] },
    ],
    counterfactual: {
      kind: 'alternative_collapse',
      from: 'parent',
      pv: ['Bxf6', 'gxf6', 'Bd3', 'Nd7', 'a4', 'b4'],
      motifs: [{ kind: 'hanging', square: 'b5', role: 'pawn', see: 0 }],
    },
    explanations: {
      en: 'Nxb5 hands over a knight for a pawn and it is by some distance the best move on the board. What it buys is the b5 square for the bishop, and with it a check the black king cannot answer by moving. Every follow-up is forced: cxb5 Bxb5+ Nbd7 and the knight is nailed to d7, where it is the only thing holding the position together. The quiet alternative shows what is at stake — after Bxf6 gxf6 Bd3 Nd7, Black is ugly but breathing, the extra pawn on b5 is real, and there is no attack left to speak of. The sacrifice is not a gamble on complications; it is the only way to keep the king in the centre.',
      ja: 'Nxb5 はナイトを歩一枚で捨てる手だが、盤上で圧倒的に最善である。得られるのは b5 のマス、そしてそこから放つ、黒王が動いて受けられない王手だ。以降は全て強制で、cxb5 Bxb5+ Nbd7 とナイトが d7 に釘付けになり、その駒だけが局面を支えることになる。静かな代案を見れば何が懸かっていたかが分かる。Bxf6 gxf6 Bd3 Nd7 なら黒は形は悪いが呼吸ができ、b5 の一歩得は本物で、攻めはもう残らない。この犠牲は複雑化に賭けた手ではなく、王を中央に留めておくための唯一の方法である。',
    },
  },
  {
    // 16. Qb8+ — the queen sacrifice that deflects the d7 knight.
    ply: 31,
    classification: 'great',
    win_prob_before: 0.99,
    win_prob_after: 1.0,
    accuracy: 100,
    played_rank: 0,
    candidates: [
      { san: 'Qb8+', score: mate(2), win_prob: 1.0, pv: ['Qb8+', 'Nxb8', 'Rd8#'] },
      { san: 'Qb5', score: cp(486), win_prob: 0.859, pv: ['Qb5', 'Qc6', 'Qxc6', 'Be7', 'Qxd7+'] },
      { san: 'Qa4', score: cp(455), win_prob: 0.848, pv: ['Qa4', 'Qc6', 'Qxc6', 'Be7', 'Qxd7+'] },
    ],
    counterfactual: {
      kind: 'alternative_collapse',
      from: 'parent',
      pv: ['Qb5', 'Qc6', 'Qxc6', 'Be7', 'Qxd7+'],
      motifs: [{ kind: 'back_rank', attacker: 'd1', king: 'e8' }],
    },
    explanations: {
      en: 'The queen is offered to the one piece that cannot refuse it. The knight on d7 is the only defender of d8, so Qb8+ forces Nxb8 and the square falls: Rd8 is mate, delivered by the rook that has been aimed down the file since move twelve. Every other move throws the point away. Qb5 wins material and the game eventually, but after Qc6 Qxc6 Be7 Qxd7+ the queens have come off and Black is merely a piece down, still on the board. Giving up the strongest piece to remove the last defender is not flashy here — it is simply the shortest line.',
      ja: 'クイーンを、それを断れない唯一の駒に差し出す。d7 のナイトは d8 の唯一の守り手なので、Qb8+ は Nxb8 を強制し、そのマスは陥落する。あとは Rd8 でメイト、12 手目からその筋に狙いを定めていたルークが決める。他の手はすべて要点を逃す。Qb5 も駒を得ていずれ勝てるが、Qc6 Qxc6 Be7 Qxd7+ とクイーンが交換され、黒は一駒損しただけでまだ盤上に残る。最強の駒を捨てて最後の守り駒を排除するのは派手さではなく、単に最短の道筋だからである。',
    },
  },
];

/** Positions where the engine already sees a forced mate (node id → score). */
export const SCRIPTED_MATES: Record<number, Score> = {
  30: mate(2), // White to move: Qb8+ Nxb8 Rd8#
  31: mate(-1), // Black to move, mated next
  32: mate(1), // White to move: Rd8#
};

/**
 * A miniature stand-in for the ECO table the server embeds, keyed by the SAN
 * path from the starting position.
 *
 * It covers the sample game and the first moves a user is likely to try on the
 * board, and — deliberately — it runs out partway through the sample game: the
 * last entry is 3. d4, so from 3... Bg4 onward no node matches and the caption
 * has to keep showing the Philidor by walking back up the path. That is the
 * case most likely to be rendered wrong, so mock mode always exercises it.
 *
 * Note that neither the table nor its end says anything about theory. It stops
 * naming positions; the players have not "left book".
 */
const OPENING_TABLE: Record<string, { eco: string; name: string }> = {
  e4: { eco: 'B00', name: 'King’s Pawn Game' },
  d4: { eco: 'A40', name: 'Queen’s Pawn Game' },
  c4: { eco: 'A10', name: 'English Opening' },
  Nf3: { eco: 'A04', name: 'Zukertort Opening' },
  'd4 d5': { eco: 'D00', name: 'Queen’s Pawn Game' },
  'd4 Nf6': { eco: 'A45', name: 'Indian Defense' },
  'e4 c5': { eco: 'B20', name: 'Sicilian Defense' },
  'e4 e6': { eco: 'C00', name: 'French Defense' },
  'e4 c6': { eco: 'B10', name: 'Caro-Kann Defense' },
  'e4 e5': { eco: 'C20', name: 'King’s Pawn Game' },
  'e4 e5 Bc4': { eco: 'C23', name: 'Bishop’s Opening' },
  'e4 e5 Nf3': { eco: 'C40', name: 'King’s Knight Opening' },
  'e4 e5 Nf3 Nc6': { eco: 'C44', name: 'King’s Knight Opening: Normal Variation' },
  'e4 e5 Nf3 Nc6 Bb5': { eco: 'C60', name: 'Ruy Lopez' },
  'e4 e5 Nf3 Nc6 Bb5 Nf6': { eco: 'C65', name: 'Ruy Lopez: Berlin Defense' },
  'e4 e5 Nf3 d6': { eco: 'C41', name: 'Philidor Defense' },
  'e4 e5 Nf3 d6 d4': { eco: 'C41', name: 'Philidor Defense' },
};

/**
 * The opening for a position, looked up by the SAN moves that reach it. Null
 * for anything the table does not name, including the starting position.
 */
export function openingForPath(sanPath: readonly string[]): OpeningInfo | null {
  const entry = OPENING_TABLE[sanPath.join(' ')];
  if (!entry) return null;
  return { eco: entry.eco, name: entry.name, matched_plies: sanPath.length };
}

export const MOCK_LANGUAGES = {
  languages: [
    { code: 'en', name: 'English' },
    { code: 'ja', name: '日本語' },
  ],
  default: 'en',
};

export const MOCK_HEALTH = {
  ok: true,
  engine: 'Stockfish 18 (mock)',
  stockfish_path: '/opt/homebrew/bin/stockfish',
};

/** Fallback explanation for positions that are not hand-scripted. */
export function genericExplanation(
  san: string,
  classification: Classification,
  lang: string,
): string {
  if (lang === 'ja') {
    return `${san} は engine の評価では ${classification} に分類される。局面の要求は変わらず、駒の働きと王の安全がそのまま次の数手を決める。ここでは大きな変化はなく、互いに計画を進める局面が続く。（これはモックデータの説明文である。）`;
  }
  return `${san} is classified ${classification}. Nothing changes about what the position demands: piece activity and king safety still decide the next few moves, and both sides simply carry on with their plans. (This text is mock fixture data.)`;
}
