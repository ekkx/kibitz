import type { UiMessageKey } from './en.ts';

/**
 * The Japanese UI catalogue.
 *
 * Terminology follows the Japanese glossary the explanation prompt already
 * uses (`crates/llm/src/prompt.rs`), because the interface and the explanation
 * are read together: a panel that says 「大悪手」 above text that says something
 * else for the same verdict is worse than no translation at all. Piece names
 * are katakana (キング, クイーン, ルーク, ビショップ, ナイト, ポーン), the
 * classifications are 定跡 / 好手 / 最善手 / 優良手 / 妥当な手 / 不正確 / 悪手 /
 * 大悪手 / 詰み逃し, and the tactical motifs keep their katakana names with
 * フォーク（両取り）and ピン（釘付け）as the prompt writes them.
 *
 * Style: です・ます調 throughout, matching the explanations. Squares, SAN, ECO
 * codes, FEN and PGN stay as they are — they are notation, not English.
 *
 * Typed as a complete record rather than a partial one: adding a string to the
 * English catalogue should fail the build here, not silently ship English text
 * inside a Japanese interface.
 */
export const ja: Record<UiMessageKey, string> = {
  'app.name': 'kibitz',
  'app.tagline': '理由まで説明するチェス解析',

  'health.checking': 'エンジンを確認しています…',
  'health.engine': 'エンジン',
  'health.ok': '{engine} 準備完了',
  'health.noEngine':
    '解析エンジンが起動しませんでした。Stockfish が見つからないか起動できないため、解析を実行できません。Stockfish をインストールしてから kibitz サーバーを再起動してください。',
  'health.unreachable':
    '127.0.0.1:7777 の kibitz サーバーに接続できません。`cargo run` で起動するか、VITE_MOCK=1 でフロントエンドを起動してフィクスチャデータを使ってください。',
  'health.engineMissing': 'エンジンがありません',
  'health.offline': 'サーバー停止中',
  'health.retry': '再試行',
  'health.mockBadge': 'モックデータ',

  'import.title': '棋譜を読み込む',
  'import.subtitle': 'PGN を貼り付けるか、好きな局面から始めて自由に分岐できます。',
  'import.tab.pgn': 'PGN',
  'import.tab.position': '局面',
  'import.pgnPlaceholder': '[Event "…"]\n1. e4 e5 2. Nf3 …',
  'import.fenLabel': 'FEN',
  'import.fenPlaceholder': '空欄のままなら初期局面から始めます',
  'import.submit': '開く',
  'import.loading': '読み込み中…',
  'import.sample': 'サンプル棋譜を使う',
  'import.emptyPgn': '先に PGN を貼り付けてください。',

  'import.feature.sweep.title': 'すべての手を分類します',
  'import.feature.sweep.body':
    '棋譜全体を解析して、不正確・悪手・大悪手に棋譜上で印をつけ、その手までひと息で移動できます。',
  'import.feature.explain.title': '「なぜ」まで説明します',
  'import.feature.explain.body':
    'どの局面でもエンジンの候補手に加えて、その局面のために書かれた解説が読めます。咎め方は盤上で再生します。',
  'import.feature.branch.title': '自分の手を試せます',
  'import.feature.branch.body':
    '対局中のどの局面からでも指して分岐でき、解析はその変化にもついていきます。',

  'board.flip': '盤を反転',
  'board.first': '最初の手へ',
  'board.prev': '前の手へ',
  'board.next': '次の手へ',
  'board.last': '最後の手へ',
  'board.startingPosition': '初期局面',

  'promotion.choose': '成る駒を選ぶ',
  'promotion.cancel': 'プロモーションをやめる',
  'piece.queen': 'クイーン',
  'piece.rook': 'ルーク',
  'piece.bishop': 'ビショップ',
  'piece.knight': 'ナイト',

  'tree.title': '棋譜',
  'tree.empty': 'まだ手がありません。盤上で指してみてください。',
  'tree.variation': '変化',
  // 「悪手」は分類名（mistake）に割り当て済みなので、不正確・悪手・大悪手・
  // 詰み逃しをまとめて指すここでは使えません。盤上で赤く描かれる「損をした手」
  // という説明そのものを名前にしています。
  'tree.mistakePrev': '前の損をした手へ',
  'tree.mistakeNext': '次の損をした手へ',
  'tree.mistakeNone': 'この方向にはもうありません',
  'tree.mistakeUnknown': 'そこまで解析していません。「棋譜全体を解析」を実行してください',

  'opening.label': 'オープニング',
  'opening.eco': 'ECO {eco}',

  'eval.title': '評価',
  'eval.winProb': '勝率',
  'eval.mateIn': 'M{n}',
  'eval.depth': '深さ {n}',
  'eval.forWhite': '白',
  'eval.forBlack': '黒',

  'analysis.title': '解析',
  'analysis.candidates': 'この局面の候補手',
  'analysis.playedMove': '指した手',
  'analysis.rank': '候補手 {n} 位',
  'analysis.notInTop': 'エンジンの候補手には入っていません',
  'analysis.accuracyLabel': '正確度',
  'analysis.analyzing': '解析中…',
  'analysis.idle': '手を選ぶか盤上で指すと、その局面を解析します。',
  'analysis.cancelled': '新しい解析に置き換えられました。',
  'analysis.failed': '解析に失敗しました: {message}',
  'analysis.analyzeNow': 'この局面を解析する',
  'analysis.motifs': '読み筋の中の戦術',

  'counterfactual.refutation': 'どう咎められるか',
  'counterfactual.alternative_collapse': '別の手を選んでいたら',
  'counterfactual.replay': '読み筋を再生',
  'counterfactual.stop': '停止',
  'counterfactual.playing': '盤上で再生中',
  'counterfactual.returned': '盤面を対局の局面に戻しました。',

  'explain.title': '解説',
  'explain.streaming': '生成中…',
  'explain.idle': 'この手の解説はありません。',
  'explain.notAnalyzed': '先にこの局面を解析してください。',
  'explain.failed': '解説を取得できませんでした: {message}',
  'explain.cached': 'キャッシュ',
  'explain.regenerate': '解説を作り直す',
  'explain.request': 'この手を解説する',

  'sweep.run': '棋譜全体を解析',
  'sweep.running': '解析中 {done} / {total}',
  'sweep.cancel': '停止',
  'sweep.done': '{total} 局面を解析しました',
  'sweep.failed': '全体解析に失敗しました: {message}',
  'sweep.accuracyFor': '{color}の正確度',
  // 英語は「3 大悪手」の語順、日本語は「大悪手 3」。数字は分類の色がつくので
  // `tParts` で描きます。
  'sweep.count': '{label} {n}',
  'sweep.summaryTitle': '対局の要約',

  'ask.title': '追加で質問する',
  'ask.placeholder': 'Bxf7+ の後はどうなりますか？',
  'ask.submit': '質問する',
  'ask.busy': '考えています…',

  'lang.label': '言語',
  'lang.hint': '画面表示と、生成される解説の言語です。',

  'settings.title': '設定',
  'settings.open': '設定',
  'settings.close': '閉じる',

  'settings.arrows.label': '盤上の矢印',
  'settings.arrows.hint':
    'エンジンの上位候補手を、評価が高いものほど太く描きます。互角の手は同じ太さで描きます。',
  'settings.arrows.none': 'なし',

  'settings.sound.label': '盤の効果音',
  'settings.sound.on': 'オン',
  'settings.sound.off': 'オフ',
  'settings.sound.hint':
    '手を指すたびに音が鳴ります。駒を取る手、キャスリング、チェック、チェックメイトはそれぞれ別の音です。',

  'settings.depth.label': '解析の深さ',
  'settings.depth.10': '高速',
  'settings.depth.12': '標準',
  'settings.depth.14': '深い',
  'settings.depth.16': '最も深い',
  'settings.depth.option': '{label}・深さ {n}',
  'settings.depth.hint':
    '深く読むほど見落としは減りますが、時間は大きく増えます。2 手深くするごとに、棋譜全体の解析時間はおよそ 3 倍になります。設定は次の解析から反映されます。この対局を解析し直すには「棋譜全体を解析」を使ってください。',

  'theme.toggle': 'ライト / ダークを切り替え',

  'session.newGame': '新しい対局',
  'session.white': '白番',
  'session.black': '黒番',
  'session.result': '結果',

  'class.book': '定跡',
  'class.great': '好手',
  'class.best': '最善手',
  'class.excellent': '優良手',
  'class.good': '妥当な手',
  'class.inaccuracy': '不正確',
  'class.mistake': '悪手',
  'class.blunder': '大悪手',
  'class.miss': '詰み逃し',

  'motif.fork': 'フォーク',
  'motif.pin': 'ピン',
  'motif.skewer': 'スキュア',
  'motif.discovered_attack': 'ディスカバードアタック',
  'motif.hanging': '浮き駒',
  'motif.back_rank': 'バックランク',
};
