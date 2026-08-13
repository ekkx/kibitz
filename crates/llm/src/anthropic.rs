//! Provider that calls the Messages API directly over `reqwest`.
//! **There is no official Anthropic Rust SDK**, hence raw HTTP.
//!
//! The response is an SSE stream; the only frames that carry explanation text
//! are `content_block_delta` events with a `text_delta`:
//!
//! ```text
//! event: content_block_delta
//! data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"..."}}
//! ```

use crate::{Language, LlmError, Provider, QaSession, StreamPiece, TextStream, Turn, prompt};
use futures::{SinkExt, StreamExt};
use kibitz_core::types::AnalysisContext;
use serde_json::{Value, json};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2023-06-01";

/// Generous enough for the 120-200 word explanation the prompt asks for, and for
/// a follow-up answer, without letting a runaway response cost real money.
const MAX_TOKENS: u32 = 4096;

pub struct AnthropicApiProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicApiProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        Ok(AnthropicApiProvider {
            api_key,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicApiProvider {
    async fn explain(
        &self,
        model: &str,
        lang: Language,
        ctx: &AnalysisContext,
    ) -> Result<TextStream, LlmError> {
        let messages = vec![json!({
            "role": "user",
            "content": prompt::user_prompt(ctx, lang),
        })];
        self.stream_messages(model, system_blocks(prompt::system_blocks(lang)), messages)
            .await
    }

    async fn ask(
        &self,
        model: &str,
        lang: Language,
        session: &mut QaSession,
        question: &str,
    ) -> Result<TextStream, LlmError> {
        // TODO(phase-3): hand the engine over as tools. `AnalysisTools` becomes
        // a `tools` array here and the loop below has to run the tool-use cycle
        // (stop_reason == "tool_use" -> execute -> send tool_result -> repeat).
        // Until then this is the plain conversational path.
        let mut messages = vec![json!({
            "role": "user",
            "content": prompt::qa_context_prompt(session.context.as_ref(), lang),
        })];
        for Turn { role, content } in &session.history {
            let role = if role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            messages.push(json!({ "role": role, "content": content }));
        }
        messages.push(json!({ "role": "user", "content": question }));

        // The fixed part gains the follow-up rules, so the cached prefix differs
        // from `explain`'s — that is expected, they are different tasks.
        let system = system_blocks(&[prompt::qa_system_prompt(lang)]);
        let stream = self.stream_messages(model, system, messages).await?;
        // The caller appends the assistant turn once it has consumed the stream.
        session.history.push(Turn {
            role: "user".into(),
            content: question.to_string(),
        });
        Ok(stream)
    }
}

impl AnthropicApiProvider {
    async fn stream_messages(
        &self,
        model: &str,
        system: Vec<Value>,
        messages: Vec<Value>,
    ) -> Result<TextStream, LlmError> {
        let body = json!({
            "model": model,
            "max_tokens": MAX_TOKENS,
            "stream": true,
            "system": system,
            "messages": messages,
        });

        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|err| LlmError::Http(err.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            return Err(LlmError::Http(format!("{status}: {}", detail.trim())));
        }

        let mut bytes = response.bytes_stream();
        let (mut tx, rx) = futures::channel::mpsc::channel::<Result<String, LlmError>>(64);

        tokio::spawn(async move {
            let mut parser = SseParser::default();
            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        let _ = tx.send(Err(LlmError::Http(err.to_string()))).await;
                        return;
                    }
                };
                if emit(&mut tx, parser.push(&chunk)).await {
                    return;
                }
            }
            emit(&mut tx, parser.finish()).await;
        });

        Ok(rx.boxed())
    }
}

/// Wrap the fixed prompt blocks as Messages API system blocks, with
/// `cache_control: {"type": "ephemeral"}` on the last one so the whole fixed
/// prefix is cached.
fn system_blocks(blocks: &[&str]) -> Vec<Value> {
    let last = blocks.len().saturating_sub(1);
    blocks
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if index == last {
                json!({
                    "type": "text",
                    "text": text,
                    "cache_control": {"type": "ephemeral"},
                })
            } else {
                json!({"type": "text", "text": text})
            }
        })
        .collect()
}

async fn emit(
    tx: &mut futures::channel::mpsc::Sender<Result<String, LlmError>>,
    pieces: Vec<StreamPiece>,
) -> bool {
    for piece in pieces {
        let item = match piece {
            StreamPiece::Text(text) => Ok(text),
            StreamPiece::Failure(message) => Err(LlmError::Http(message)),
        };
        let is_error = item.is_err();
        if tx.send(item).await.is_err() || is_error {
            return true;
        }
    }
    false
}

/// Incremental SSE decoder.
///
/// Works line by line rather than on `\n\n`-separated frames: the `data:` line
/// is self-describing, so the `event:` line adds nothing, and a line-oriented
/// buffer handles a chunk boundary anywhere — including inside a multi-byte
/// UTF-8 sequence — without special cases.
#[derive(Default)]
pub(crate) struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<StreamPiece> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(newline) = self.buf.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=newline).collect();
            line.pop(); // the newline itself
            handle_line(&line, &mut out);
        }
        out
    }

    /// Flush a trailing line that never got its newline.
    pub(crate) fn finish(&mut self) -> Vec<StreamPiece> {
        let rest = std::mem::take(&mut self.buf);
        let mut out = Vec::new();
        if !rest.is_empty() {
            handle_line(&rest, &mut out);
        }
        out
    }
}

fn handle_line(line: &[u8], out: &mut Vec<StreamPiece>) {
    let line = String::from_utf8_lossy(line);
    // `\r\n` line endings, and the `event:` / comment lines we do not need.
    let line = line.trim_end_matches('\r');
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        tracing::debug!(payload, "ignoring unparseable SSE data line");
        return;
    };

    match value.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            let Some(delta) = value.get("delta") else {
                return;
            };
            if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
                return;
            }
            if let Some(text) = delta.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                out.push(StreamPiece::Text(text.to_string()));
            }
        }
        Some("error") => {
            let detail = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            out.push(StreamPiece::Failure(format!("anthropic api: {detail}")));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire format of a streamed Messages API response.
    const SAMPLE: &str = concat!(
        "event: message_start\n",
        r#"data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","content":[],"model":"claude-haiku-4-5","usage":{"input_tokens":812,"output_tokens":1}}}"#,
        "\n\n",
        "event: content_block_start\n",
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        "\n\n",
        "event: ping\n",
        r#"data: {"type":"ping"}"#,
        "\n\n",
        "event: content_block_delta\n",
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Nxe4 loses a knight — "}}"#,
        "\n\n",
        "event: content_block_delta\n",
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"エンジンの読み筋では Qa4+ がフォークになります。"}}"#,
        "\n\n",
        "event: content_block_stop\n",
        r#"data: {"type":"content_block_stop","index":0}"#,
        "\n\n",
        "event: message_delta\n",
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":57}}"#,
        "\n\n",
        "event: message_stop\n",
        r#"data: {"type":"message_stop"}"#,
        "\n\n",
    );

    const EXPECTED: &str = "Nxe4 loses a knight — エンジンの読み筋では Qa4+ がフォークになります。";

    fn run_in_chunks(input: &str, size: usize) -> (String, Vec<String>) {
        let bytes = input.as_bytes();
        let mut parser = SseParser::default();
        let mut text = String::new();
        let mut failures = Vec::new();
        let collect = |pieces: Vec<StreamPiece>, text: &mut String, failures: &mut Vec<String>| {
            for piece in pieces {
                match piece {
                    StreamPiece::Text(chunk) => text.push_str(&chunk),
                    StreamPiece::Failure(message) => failures.push(message),
                }
            }
        };
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + size).min(bytes.len());
            collect(parser.push(&bytes[offset..end]), &mut text, &mut failures);
            offset = end;
        }
        collect(parser.finish(), &mut text, &mut failures);
        (text, failures)
    }

    #[test]
    fn parses_the_whole_payload_in_one_chunk() {
        let (text, failures) = run_in_chunks(SAMPLE, SAMPLE.len());
        assert_eq!(text, EXPECTED);
        assert!(failures.is_empty());
    }

    /// The bug this kind of parser always has: a chunk boundary landing in the
    /// middle of an event — including in the middle of a multi-byte character.
    #[test]
    fn survives_chunks_split_mid_event() {
        for size in [1, 2, 3, 5, 17, 64, 128, 333, 1024] {
            let (text, failures) = run_in_chunks(SAMPLE, size);
            assert_eq!(text, EXPECTED, "chunk size {size}");
            assert!(failures.is_empty(), "chunk size {size}");
        }
    }

    #[test]
    fn handles_crlf_line_endings_and_a_missing_final_newline() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        let trimmed = crlf.trim_end_matches(['\r', '\n']);
        let (text, failures) = run_in_chunks(trimmed, 11);
        assert_eq!(text, EXPECTED);
        assert!(failures.is_empty());
    }

    #[test]
    fn a_mid_stream_error_event_becomes_a_failure() {
        let input = concat!(
            "event: content_block_delta\n",
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}"#,
            "\n\n",
            "event: error\n",
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            "\n\n",
        );
        let (text, failures) = run_in_chunks(input, 7);
        assert_eq!(text, "partial");
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("Overloaded"));
    }

    #[test]
    fn thinking_deltas_and_done_markers_are_ignored() {
        let input = concat!(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#,
            "\n\n",
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}"#,
            "\n\n",
            r#"data: {"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"real"}}"#,
            "\n\n",
            "data: [DONE]\n\n",
        );
        let (text, failures) = run_in_chunks(input, 5);
        assert_eq!(text, "real");
        assert!(failures.is_empty());
    }

    #[test]
    fn cache_control_goes_on_the_last_system_block_only() {
        let blocks = system_blocks(prompt::system_blocks(Language::Ja));
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(
            blocks[1].get("cache_control"),
            Some(&json!({"type": "ephemeral"}))
        );
        assert!(
            blocks[1]
                .get("text")
                .and_then(Value::as_str)
                .unwrap()
                .contains("用語対応表")
        );

        let en = system_blocks(prompt::system_blocks(Language::En));
        assert_eq!(en.len(), 1);
        assert_eq!(
            en[0].get("cache_control"),
            Some(&json!({"type": "ephemeral"}))
        );
    }

    #[test]
    fn a_missing_api_key_is_reported() {
        // Only meaningful when the variable is genuinely unset in this process.
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            return;
        }
        assert!(matches!(
            AnthropicApiProvider::from_env(),
            Err(LlmError::MissingApiKey)
        ));
    }

    /// End-to-end against the real Messages API. Not run by default: it costs
    /// money and needs a key.
    ///
    /// Run it manually with:
    ///     ANTHROPIC_API_KEY=sk-ant-... cargo test -p kibitz-llm -- --ignored anthropic_end_to_end
    #[tokio::test]
    #[ignore = "calls the live Anthropic Messages API and costs money"]
    async fn anthropic_end_to_end() {
        let provider = AnthropicApiProvider::from_env().expect("ANTHROPIC_API_KEY must be set");
        let ctx = crate::test_support::fixture_context();
        let Ok(mut stream) = provider
            .explain("claude-haiku-4-5", Language::En, &ctx)
            .await
        else {
            panic!("the Messages API rejected the request");
        };
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            text.push_str(&chunk.expect("no stream error"));
        }
        assert!(!text.trim().is_empty(), "the model said nothing");
    }
}
