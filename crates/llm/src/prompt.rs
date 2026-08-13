//! Prompt construction. **The structure is shared by both providers**:
//!
//! ```text
//! [fixed]     role / prohibitions / output structure / chess-term glossary
//! [variable]  the AnalysisContext as JSON
//! ```
//!
//! The fixed part is authored per language. The glossary is only present for
//! languages other than English.

use crate::lang::Language;
use crate::{QaSession, Turn};
use kibitz_core::types::AnalysisContext;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

// ─── English ────────────────────────────────────────────

const EN_ROLE: &str = r#"You are the explanation writer for kibitz, a chess analysis tool.

Every fact you will ever state has already been computed for you — by Stockfish and by
deterministic analysis code — and handed to you as a single JSON object. You do not analyse
the position. You do not calculate. You put the facts you were given into words.

You have exactly three degrees of freedom:

1. Which of the given facts matter in this position, and which to leave out.
2. What order to say them in.
3. What words to say them in.

# Prohibitions

- Never mention a piece, a square, a file, a rank, or a line that does not appear in the
  JSON. If it is not in the JSON, it does not exist.
- Never judge for yourself whether the evaluation is good or bad. `played.classification`
  has already decided that — follow it. Do not soften a blunder, and do not talk a good move
  down.
- Never pad with generalities. Every sentence must rest on a specific value from the JSON:
  a SAN move, a square, a classification, a tactical motif, a feature change, an opening
  name. Advice that would fit any position at all does not belong here.
- Never assert the opponent's intent. `outlook.long_pv` and `counterfactual.pv` assume best
  play by both sides; your opponent will not necessarily play them. Attribute a line to the
  engine, never to a person's plan.
    - Wrong: "Your opponent is planning a kingside attack."
    - Right: "In the engine's line the d-file opens next, and ..."
- Never invent a number. Quote evaluations, win probabilities and counts exactly as given.

# Output structure

Write in this order, as flowing prose. No headings, no bullet lists, no JSON, no preamble
about what you are about to do.

1. **The conclusion.** What the move was, in the terms `played.classification` sets, in one
   or two sentences.
2. **How the opponent punishes it.** Walk `counterfactual.pv` and name the motifs listed in
   `counterfactual.motifs`. When `counterfactual.kind` is `alternative_collapse` the move
   was a good one, so this becomes what would have collapsed after the second-best move
   instead.
3. **The positional reasons.** The entries in `static_diff.changes` — and `outlook`, when it
   is present — that actually explain the verdict. Two or three at most; drop the rest.

Aim for 120-200 words. Address the player as "you". Write every move in exactly the SAN
given in the JSON."#;

const EN_QA: &str = r#"# Follow-up questions

The same rules apply. You may use only the facts in the JSON above and in this conversation,
and you must still not assert the opponent's intent. If a question cannot be answered from
what you have been given, say so plainly and say what would have to be analysed to answer
it. Never guess a line, an evaluation, or a square."#;

// ─── Japanese ───────────────────────────────────────────

const JA_ROLE: &str = r#"あなたはチェス解析ツール kibitz の解説文を書くライターです。

あなたが述べる事実は、すべて Stockfish と決定論的な解析コードがすでに計算し、1 つの JSON
としてあなたに渡したものです。あなたは局面を解析しません。読みも入れません。与えられた事実を
言葉にするだけです。

あなたに許された裁量は、次の 3 つだけです。

1. 与えられた事実のうち、この局面で重要なものを選ぶこと
2. どの順序で述べるかを決めること
3. どのような言葉で述べるかを決めること

# 禁止事項

- JSON に現れない駒・マス・筋・段・手順には、一切触れないでください。JSON にないものは存在
  しません。
- 評価の良し悪しを自分で判断しないでください。それは `played.classification` がすでに決めて
  います。その判定に従ってください。大悪手を和らげたり、好手を控えめに言い換えたりしては
  いけません。
- 一般論で字数を埋めないでください。すべての文が JSON 内の具体的な値（SAN の指し手、マス、
  classification、戦術モチーフ、特徴量の変化、オープニング名）に裏づけられている必要が
  あります。どの局面にも当てはまるような助言は書かないでください。
- 相手の意図を断定しないでください。`outlook.long_pv` と `counterfactual.pv` は双方が最善を
  尽くした場合の手順であり、相手が実際にそう指すとは限りません。手順は必ずエンジンの読み筋
  として述べ、誰かの計画としては述べないでください。
    - 誤:「相手はキングサイドへの攻めを狙っています」
    - 正:「エンジンの読み筋では、次に d ファイルが開いて……」
- 数値を創作しないでください。評価値・勝率・個数は、与えられたとおりに引用してください。

# 出力の構成

次の順序で、地の文として書いてください。見出し・箇条書き・JSON・前置きは使わないでください。

1. **結論。** `played.classification` の判定に沿って、その手が何だったのかを 1〜2 文で。
2. **相手にどう咎められるか。** `counterfactual.pv` を順に追い、`counterfactual.motifs` の
   戦術モチーフを名前で挙げてください。`counterfactual.kind` が `alternative_collapse` の
   場合、その手は好手なので、代わりに次善手を選んでいたら何が崩れていたかを書きます。
3. **位置的な理由。** `static_diff.changes` と（あれば）`outlook` のうち、その判定を実際に
   説明している項目だけを挙げます。多くても 2〜3 点にとどめ、残りは捨ててください。

分量は 250〜400 字程度。文体は「です・ます調」で統一してください（「〜だ」「〜である」は使い
ません）。読み手のことは「あなた」と呼びます。指し手は JSON に与えられた SAN の表記のまま書いて
ください。"#;

const JA_GLOSSARY: &str = r#"# 用語対応表

JSON 内の英語のキーや値は、本文では次の日本語を使ってください。ここにない英語表記をそのまま
本文に混ぜないでください。

## 駒（`role` / `common_piece` の値）

- `king` — キング（「王」「玉」とは書きません）
- `queen` — クイーン（「女王」「クイン」とは書きません）
- `rook` — ルーク
- `bishop` — ビショップ
- `knight` — ナイト（「桂馬」とは書きません）
- `pawn` — ポーン
- `white` / `black` — 白番／黒番、白の駒／黒の駒

## 盤上の位置

- マス（`e4` などの座標）は英数字のまま書きます
- ファイル（縦の列、`a`〜`h`） — 「d ファイル」「d 筋」
- ランク（横の列、1〜8） — 「第 1 ランク」「1 段目」

## 戦術モチーフ（`Motif` の `kind`）

- `fork` — フォーク（両取り）
- `pin` — ピン（釘付け）
- `skewer` — スキュア（串刺し）
- `discovered_attack` — ディスカバードアタック（開き攻撃）
- `hanging` — 只（ただ）の駒、取り返しのきかない浮き駒
- `back_rank` — バックランク（一段目の詰み筋）

## 特徴量（`Features` と `FeatureChange.kind`）

- `material` — 駒得・駒損（マテリアル）
- `center_control` — 中央の支配
- `king_safety` — キングの安全度
- `attackers` — キング周辺に利いている敵の駒の数
- `missing_shield_pawns` — キング前の守りのポーンの欠け
- `pawn_structure` — ポーン構造
- `isolated` — 孤立ポーン
- `doubled` — 重なりポーン（ダブルポーン）
- `passed` — パスポーン（通しポーン）
- `open_files` / `fully_open` — オープンファイル（開いた筋）
- 半開の筋（`fully_open` が false） — セミオープンファイル
- `mobility` — 駒の可動性（合法手の数）
- `see` — SEE（駒の取り合いの損得計算）

## 評価（`Classification`）

- `book` — 定跡
- `great` — 好手（!）
- `best` — 最善手
- `excellent` — 優良手
- `good` — 妥当な手
- `inaccuracy` — 不正確（?!）
- `mistake` — 悪手（?）
- `blunder` — 大悪手（??）
- `miss` — 詰み逃し

## その他

- `pv` / `long_pv` — 読み筋
- `counterfactual` — もし別の手を指していたらどうなっていたか
- `refutation` — 咎め方
- `alternative_collapse` — 次善手を選んでいた場合の崩壊
- `win_prob` / `win_prob_before` / `win_prob_after` — 勝率
- `delta` — 勝率の増減
- `accuracy` — 正確度
- `eco` / `opening` — ECO コード／オープニング名
- `side_to_move` — 手番
- `fen` — FEN（局面の表記）"#;

const JA_QA: &str = r#"# 追加の質問について

以上のルールはそのまま適用されます。使ってよいのは、上の JSON とこの会話に現れた事実だけ
です。相手の意図を断定してはいけません。与えられた情報だけでは答えられない質問には、その旨を
はっきり述べ、答えるには何を解析する必要があるかを書いてください。手順・評価値・マスを推測で
埋めてはいけません。"#;

static EN_BLOCKS: [&str; 1] = [EN_ROLE];
static JA_BLOCKS: [&str; 2] = [JA_ROLE, JA_GLOSSARY];

static JA_SYSTEM: LazyLock<String> = LazyLock::new(|| JA_BLOCKS.join("\n\n"));
static EN_QA_SYSTEM: LazyLock<String> = LazyLock::new(|| format!("{EN_ROLE}\n\n{EN_QA}"));
static JA_QA_SYSTEM: LazyLock<String> = LazyLock::new(|| format!("{}\n\n{JA_QA}", *JA_SYSTEM));

/// The fixed part. In `AnthropicApiProvider` this goes in the system block, with
/// `cache_control: {"type": "ephemeral"}` on the final block.
pub fn system_prompt(lang: Language) -> &'static str {
    match lang {
        Language::En => EN_ROLE,
        Language::Ja => JA_SYSTEM.as_str(),
    }
}

/// The fixed part, split the way `AnthropicApiProvider` wants it: one block per
/// section, so `cache_control` can be attached to the last one. Concatenating
/// the blocks with a blank line yields exactly [`system_prompt`].
pub fn system_blocks(lang: Language) -> &'static [&'static str] {
    match lang {
        Language::En => &EN_BLOCKS,
        Language::Ja => &JA_BLOCKS,
    }
}

/// The fixed part for follow-up mode: the explanation prompt plus the extra
/// rules that apply once the user starts asking questions.
///
/// TODO(phase-3): when the engine is exposed as tools (`AnalysisTools`, §12.4),
/// this block also has to tell the model to reach for those tools instead of
/// answering "that cannot be determined".
pub fn qa_system_prompt(lang: Language) -> &'static str {
    match lang {
        Language::En => EN_QA_SYSTEM.as_str(),
        Language::Ja => JA_QA_SYSTEM.as_str(),
    }
}

/// The variable part: the `AnalysisContext` as JSON plus whatever framing it needs.
pub fn user_prompt(ctx: &AnalysisContext, lang: Language) -> String {
    let json = pretty_context(ctx);
    match lang {
        Language::En => format!(
            "Here is the analysis of a single move, as JSON. Explain it, following the rules above.\n\n```json\n{json}\n```"
        ),
        Language::Ja => format!(
            "次の JSON は 1 手ぶんの解析結果です。上のルールに従って解説してください。\n\n```json\n{json}\n```"
        ),
    }
}

/// The opening user turn of a follow-up conversation: the same JSON, framed as
/// the shared reference material for the questions that follow.
pub fn qa_context_prompt(ctx: Option<&AnalysisContext>, lang: Language) -> String {
    match (ctx, lang) {
        (Some(ctx), Language::En) => format!(
            "This is the position under discussion, as JSON. Every answer you give must rest on it.\n\n```json\n{}\n```",
            pretty_context(ctx)
        ),
        (Some(ctx), Language::Ja) => format!(
            "以下が話題になっている局面の JSON です。あなたの回答はすべてこの内容に基づいていなければなりません。\n\n```json\n{}\n```",
            pretty_context(ctx)
        ),
        (None, Language::En) => {
            "No position analysis is attached to this conversation.".to_string()
        }
        (None, Language::Ja) => "この会話には局面の解析結果が添付されていません。".to_string(),
    }
}

/// The whole follow-up conversation flattened into one prompt. Needed by
/// `ClaudeCodeProvider`, which gets a single prompt on stdin rather than a
/// structured message list.
pub fn qa_transcript_prompt(session: &QaSession, question: &str, lang: Language) -> String {
    let mut out = qa_context_prompt(session.context.as_ref(), lang);
    let (user_label, assistant_label, question_label) = match lang {
        Language::En => ("User", "You", "New question from the user"),
        Language::Ja => ("ユーザー", "あなた", "ユーザーからの新しい質問"),
    };

    if !session.history.is_empty() {
        out.push_str("\n\n---\n");
        for Turn { role, content } in &session.history {
            let label = if role == "assistant" {
                assistant_label
            } else {
                user_label
            };
            out.push_str(&format!("\n{label}: {content}\n"));
        }
    }

    out.push_str(&format!("\n\n---\n\n{question_label}: {question}\n"));
    out
}

/// Cache key for a generated explanation. Stable hash over the `AnalysisContext`,
/// the model **and the language** — all three change the output.
pub fn context_hash(ctx: &AnalysisContext, model: &str, lang: Language) -> String {
    let payload = serde_json::json!({
        // Bump when the prompt changes in a way that invalidates cached output.
        "prompt_version": PROMPT_VERSION,
        "lang": lang.code(),
        "model": model,
        "context": to_value(ctx),
    });
    stable_digest(&payload)
}

/// Part of the cache key: a prompt change makes every cached explanation stale.
const PROMPT_VERSION: u32 = 1;

fn to_value(ctx: &AnalysisContext) -> Value {
    serde_json::to_value(ctx).unwrap_or(Value::Null)
}

fn pretty_context(ctx: &AnalysisContext) -> String {
    serde_json::to_string_pretty(&canonical(&to_value(ctx))).unwrap_or_else(|_| "{}".to_string())
}

/// SHA-256 over the canonical serialization, hex-encoded.
pub(crate) fn stable_digest(value: &Value) -> String {
    let canonical = canonical(value);
    // `to_string` on a canonicalized value is deterministic: object keys are
    // emitted in the order they were inserted, and `canonical` inserts sorted.
    let serialized = serde_json::to_string(&canonical).unwrap_or_default();
    let digest = Sha256::digest(serialized.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Recursively sort object keys.
///
/// `serde_json::Map` is a `BTreeMap` by default, but any crate in the graph can
/// turn on the `preserve_order` feature and make it an `IndexMap` — at which
/// point the iteration order of a `HashMap` upstream (such as
/// `PositionAnalysis.explanations`) would leak into the hash. Sorting here makes
/// the digest independent of that.
pub(crate) fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut sorted = Map::with_capacity(keys.len());
            for key in keys {
                sorted.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixture_context;
    use std::collections::HashMap;

    #[test]
    fn system_prompt_is_distinct_and_non_empty_per_language() {
        let en = system_prompt(Language::En);
        let ja = system_prompt(Language::Ja);
        assert!(!en.trim().is_empty());
        assert!(!ja.trim().is_empty());
        assert_ne!(en, ja);
    }

    #[test]
    fn system_prompt_states_the_prohibitions() {
        for lang in [Language::En, Language::Ja] {
            let prompt = system_prompt(lang);
            // Every prohibition names the JSON field it is anchored to.
            assert!(prompt.contains("played.classification"), "{lang:?}");
            assert!(prompt.contains("counterfactual.pv"), "{lang:?}");
            assert!(prompt.contains("static_diff.changes"), "{lang:?}");
            assert!(prompt.contains("long_pv"), "{lang:?}");
        }
    }

    #[test]
    fn glossary_is_present_for_ja_and_absent_for_en() {
        let ja = system_prompt(Language::Ja);
        assert!(ja.contains("用語対応表"));
        assert!(ja.contains("フォーク"));
        assert!(ja.contains("ディスカバードアタック"));
        assert!(ja.contains("バックランク"));
        assert!(ja.contains("パスポーン"));
        // Piece names: the model reaches for 女王 / 桂馬 without them.
        assert!(ja.contains("クイーン"));
        assert!(ja.contains("ナイト"));
        assert!(ja.contains("ビショップ"));

        let en = system_prompt(Language::En);
        assert!(!en.contains("Glossary"));
        // Every glossary-only key would otherwise show up here.
        assert!(!en.contains("discovered_attack"));

        assert!(Language::Ja.needs_glossary());
        assert!(!Language::En.needs_glossary());
    }

    #[test]
    fn system_blocks_join_back_into_system_prompt() {
        for lang in [Language::En, Language::Ja] {
            assert_eq!(system_blocks(lang).join("\n\n"), system_prompt(lang));
        }
        assert_eq!(system_blocks(Language::En).len(), 1);
        assert_eq!(system_blocks(Language::Ja).len(), 2);
    }

    #[test]
    fn user_prompt_carries_the_context_json() {
        let ctx = fixture_context();
        for lang in [Language::En, Language::Ja] {
            let prompt = user_prompt(&ctx, lang);
            assert!(prompt.contains("```json"));
            assert!(prompt.contains(&ctx.played.san));
            assert!(prompt.contains(&ctx.position.fen));
        }
        assert_ne!(
            user_prompt(&ctx, Language::En),
            user_prompt(&ctx, Language::Ja)
        );
    }

    #[test]
    fn context_hash_is_stable_across_calls() {
        let ctx = fixture_context();
        let a = context_hash(&ctx, "claude-haiku-4-5", Language::En);
        let b = context_hash(&ctx, "claude-haiku-4-5", Language::En);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn context_hash_changes_with_model_and_language() {
        let ctx = fixture_context();
        let base = context_hash(&ctx, "claude-haiku-4-5", Language::En);
        assert_ne!(base, context_hash(&ctx, "claude-opus-5", Language::En));
        assert_ne!(base, context_hash(&ctx, "claude-haiku-4-5", Language::Ja));
        assert_ne!(
            context_hash(&ctx, "claude-opus-5", Language::En),
            context_hash(&ctx, "claude-opus-5", Language::Ja)
        );
    }

    #[test]
    fn context_hash_changes_with_the_context() {
        let ctx = fixture_context();
        let mut other = fixture_context();
        other.played.san = "Nf3".into();
        assert_ne!(
            context_hash(&ctx, "m", Language::En),
            context_hash(&other, "m", Language::En)
        );
    }

    /// `PositionAnalysis.explanations` is a `HashMap`, and two maps with the
    /// same contents can iterate in different orders. The digest must not care.
    #[test]
    fn stable_digest_ignores_map_iteration_order() {
        let keys = [
            "en", "ja", "de", "fr", "es", "it", "pt", "nl", "sv", "pl", "ru", "zh", "ko", "tr",
        ];

        let mut forward: HashMap<String, String> = HashMap::new();
        for key in keys {
            forward.insert(key.to_string(), format!("explanation for {key}"));
        }

        let mut backward: HashMap<String, String> = HashMap::new();
        for key in keys.iter().rev() {
            backward.insert(key.to_string(), format!("explanation for {key}"));
        }

        // Sanity: the two maps really do hold the same thing.
        assert_eq!(forward, backward);

        let a = stable_digest(&serde_json::to_value(&forward).unwrap());
        let b = stable_digest(&serde_json::to_value(&backward).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn canonical_sorts_nested_object_keys() {
        let value = serde_json::json!({
            "z": {"b": 1, "a": {"y": 2, "x": 3}},
            "a": [{"n": 1, "m": 2}],
        });
        let text = serde_json::to_string(&canonical(&value)).unwrap();
        assert_eq!(
            text,
            r#"{"a":[{"m":2,"n":1}],"z":{"a":{"x":3,"y":2},"b":1}}"#
        );
    }

    #[test]
    fn qa_transcript_includes_history_and_question() {
        let mut session = QaSession {
            context: Some(fixture_context()),
            history: Vec::new(),
        };
        session.history.push(Turn {
            role: "user".into(),
            content: "Why not Nf3?".into(),
        });
        session.history.push(Turn {
            role: "assistant".into(),
            content: "Because the engine's line continues ...".into(),
        });

        let prompt = qa_transcript_prompt(&session, "What about Bxf7+?", Language::En);
        assert!(prompt.contains("Why not Nf3?"));
        assert!(prompt.contains("Because the engine's line continues ..."));
        assert!(prompt.contains("What about Bxf7+?"));
        assert!(prompt.contains("```json"));

        let ja = qa_transcript_prompt(&session, "Bxf7+ はどうですか？", Language::Ja);
        assert!(ja.contains("ユーザーからの新しい質問"));
    }

    #[test]
    fn qa_system_prompt_extends_the_explanation_prompt() {
        for lang in [Language::En, Language::Ja] {
            let base = system_prompt(lang);
            let qa = qa_system_prompt(lang);
            assert!(qa.starts_with(base));
            assert!(qa.len() > base.len());
        }
    }
}
