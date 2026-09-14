//! Provider that shells out to `claude -p --output-format stream-json`.
//! Runs inside an existing subscription, so it costs nothing extra. **Default.**
//!
//! The CLI emits JSON Lines on stdout. Only two shapes carry explanation text:
//!
//! ```text
//! {"type":"stream_event","event":{"type":"content_block_delta",
//!   "delta":{"type":"text_delta","text":"..."}}}          ← incremental
//! {"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
//! ```
//!
//! We ask for `--include-partial-messages`, so the deltas arrive first and the
//! whole `assistant` message repeats them afterwards. The parser therefore takes
//! the deltas and drops the `assistant` echo — but falls back to the `assistant`
//! blocks if no delta was ever seen, which keeps it working against a CLI build
//! that ignores the flag.

use crate::{Language, LlmError, Provider, QaSession, StreamPiece, TextStream, prompt};
use futures::{SinkExt, StreamExt};
use kibitz_core::types::AnalysisContext;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub struct ClaudeCodeProvider {
    /// Defaults to `claude`, resolved through PATH.
    pub bin: String,
}

impl Default for ClaudeCodeProvider {
    fn default() -> Self {
        ClaudeCodeProvider {
            bin: std::env::var("KIBITZ_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
        }
    }
}

#[async_trait::async_trait]
impl Provider for ClaudeCodeProvider {
    async fn explain(
        &self,
        model: &str,
        lang: Language,
        ctx: &AnalysisContext,
    ) -> Result<TextStream, LlmError> {
        self.run(
            model,
            lang,
            prompt::system_prompt(lang),
            prompt::user_prompt(ctx, lang),
        )
        .await
    }

    async fn ask(
        &self,
        model: &str,
        lang: Language,
        session: &mut QaSession,
        question: &str,
    ) -> Result<TextStream, LlmError> {
        // TODO(phase-3): hand the engine over as tools. `AnalysisTools` is
        // exposed as an MCP server and passed here with
        // `--mcp-config <path-or-json> --strict-mcp-config`, plus
        // `--allowed-tools` naming the four analysis tools. Until then this is
        // the plain conversational path and the model has to say so when the
        // JSON does not contain the answer.
        let user = prompt::qa_transcript_prompt(session, question, lang);
        let stream = self
            .run(model, lang, prompt::qa_system_prompt(lang), user)
            .await?;
        // The caller appends the assistant turn once it has consumed the stream.
        session.history.push(crate::Turn {
            role: "user".into(),
            content: question.to_string(),
        });
        Ok(stream)
    }
}

impl ClaudeCodeProvider {
    /// Spawn the CLI, feed `user` on stdin, and stream the decoded text back.
    async fn run(
        &self,
        model: &str,
        lang: Language,
        system: &str,
        user: String,
    ) -> Result<TextStream, LlmError> {
        let mut child = Command::new(&self.bin)
            .arg("-p")
            .arg("--output-format")
            .arg("stream-json")
            // `-p` with `stream-json` is rejected without this.
            .arg("--verbose")
            .arg("--include-partial-messages")
            .arg("--model")
            .arg(model)
            .arg("--system-prompt")
            .arg(system)
            // Without this the CLI starts every MCP server the user has
            // configured globally and ships their tool definitions with the
            // request. Measured on a developer machine with five servers
            // configured: 60-90 extra tool definitions and startup up to the
            // `system/init` event of 1.9s instead of 0.9s. This provider never
            // calls a tool — the whole answer is in the prompt.
            // The user's own `language` setting otherwise decides the output
            // language, and it beats the system prompt: with
            // `"language": "Japanese"` in `~/.claude/settings.json`, `--lang en`
            // produced Japanese for every explanation. `--settings` is merged
            // over the user's, so naming the language here — and only the
            // language — puts `Language` back in charge without disturbing
            // anything else they have configured.
            .arg("--settings")
            .arg(format!(
                r#"{{"language":"{}"}}"#,
                lang.cli_settings_name()
            ))
            .arg("--strict-mcp-config")
            .arg("--mcp-config")
            .arg(r#"{"mcpServers":{}}"#)
            // Extended thinking is on by default, and it dominates the wall
            // clock: ~90s of reasoning before the first token, against ~2s of
            // process startup and ~5s of actual answer. Measured end to end on
            // a real `AnalysisContext` with the Japanese prompts: 97.6s with
            // thinking, 8.5s without, for the same 400-character answer.
            //
            // Nothing in this task needs it. Every fact has already been
            // computed by the engine and handed over in the JSON; the model is
            // only putting it into words. Both variables are set because the
            // CLI has honoured them at different versions.
            .env("MAX_THINKING_TOKENS", "0")
            .env("CLAUDE_CODE_DISABLE_THINKING", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    LlmError::Process(format!(
                        "`{}` not found on PATH. Install Claude Code, or point KIBITZ_CLAUDE_BIN \
                         at the binary, or set KIBITZ_LLM=anthropic to use the API instead.",
                        self.bin
                    ))
                } else {
                    LlmError::Process(format!("could not start `{}`: {err}", self.bin))
                }
            })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| LlmError::Process("child stdin was not piped".into()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| LlmError::Process("child stdout was not piped".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| LlmError::Process("child stderr was not piped".into()))?;

        // Write the prompt from its own task: a prompt larger than the pipe
        // buffer would otherwise deadlock against a child that has not started
        // draining stdin yet.
        tokio::spawn(async move {
            if let Err(err) = stdin.write_all(user.as_bytes()).await {
                tracing::warn!(%err, "failed to write prompt to claude stdin");
            }
            let _ = stdin.shutdown().await;
        });

        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let bin = self.bin.clone();
        let (mut tx, rx) = futures::channel::mpsc::channel::<Result<String, LlmError>>(64);

        tokio::spawn(async move {
            let mut parser = StreamJsonParser::default();
            let mut chunk = vec![0u8; 8192];
            let mut failed = false;

            loop {
                let read = match stdout.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(err) => {
                        let _ = tx.send(Err(LlmError::Io(err))).await;
                        failed = true;
                        break;
                    }
                };
                if emit(&mut tx, parser.push(&chunk[..read])).await {
                    failed = true;
                    break;
                }
            }

            if !failed && emit(&mut tx, parser.finish()).await {
                failed = true;
            }

            let status = child.wait().await;
            let stderr = stderr_task.await.unwrap_or_default();

            match status {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    if !failed {
                        let detail = stderr.trim();
                        let detail = if detail.is_empty() {
                            "no stderr"
                        } else {
                            detail
                        };
                        let _ = tx
                            .send(Err(LlmError::Process(format!(
                                "`{bin}` exited with {status}: {detail}"
                            ))))
                            .await;
                    }
                }
                Err(err) => {
                    if !failed {
                        let _ = tx
                            .send(Err(LlmError::Process(format!(
                                "could not wait for `{bin}`: {err}"
                            ))))
                            .await;
                    }
                }
            }
        });

        Ok(rx.boxed())
    }
}

/// Forward decoded pieces onto the channel. Returns `true` if the stream should
/// stop (a failure was reported, or the receiver went away).
async fn emit(
    tx: &mut futures::channel::mpsc::Sender<Result<String, LlmError>>,
    pieces: Vec<StreamPiece>,
) -> bool {
    for piece in pieces {
        let item = match piece {
            StreamPiece::Text(text) => Ok(text),
            StreamPiece::Failure(message) => Err(LlmError::Process(message)),
        };
        let is_error = item.is_err();
        if tx.send(item).await.is_err() || is_error {
            return true;
        }
    }
    false
}

/// Incremental JSON Lines decoder.
///
/// Fed arbitrary byte chunks — a chunk boundary may land anywhere, including in
/// the middle of a multi-byte UTF-8 sequence — and yields the text pieces of
/// every complete line seen so far.
#[derive(Default)]
pub(crate) struct StreamJsonParser {
    buf: Vec<u8>,
    saw_delta: bool,
}

impl StreamJsonParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<StreamPiece> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(newline) = self.buf.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=newline).collect();
            line.pop(); // the newline itself
            self.handle_line(&line, &mut out);
        }
        out
    }

    /// Flush a trailing line that never got its newline.
    pub(crate) fn finish(&mut self) -> Vec<StreamPiece> {
        let rest = std::mem::take(&mut self.buf);
        let mut out = Vec::new();
        if !rest.is_empty() {
            self.handle_line(&rest, &mut out);
        }
        out
    }

    fn handle_line(&mut self, line: &[u8], out: &mut Vec<StreamPiece>) {
        let line = String::from_utf8_lossy(line);
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            tracing::debug!(line, "ignoring unparseable stream-json line");
            return;
        };

        // Subagent output carries a parent tool-use id; it is not the answer.
        if value
            .get("parent_tool_use_id")
            .and_then(Value::as_str)
            .is_some()
        {
            return;
        }

        match value.get("type").and_then(Value::as_str) {
            Some("stream_event") => {
                let Some(event) = value.get("event") else {
                    return;
                };
                if event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
                    return;
                }
                let Some(delta) = event.get("delta") else {
                    return;
                };
                if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
                    return;
                }
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    self.saw_delta = true;
                    if !text.is_empty() {
                        out.push(StreamPiece::Text(text.to_string()));
                    }
                }
            }
            // Complete message. Only used when partial messages are unavailable,
            // otherwise it duplicates the deltas we already emitted.
            Some("assistant") if !self.saw_delta => {
                let blocks = value
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_array);
                for block in blocks.into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) != Some("text") {
                        continue;
                    }
                    if let Some(text) = block.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        out.push(StreamPiece::Text(text.to_string()));
                    }
                }
            }
            Some("result") if value.get("is_error").and_then(Value::as_bool) == Some(true) => {
                let detail = value
                    .get("result")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("subtype").and_then(Value::as_str))
                    .unwrap_or("unknown error");
                out.push(StreamPiece::Failure(format!(
                    "claude reported an error: {detail}"
                )));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from
    /// `claude -p --output-format stream-json --verbose --include-partial-messages`.
    const SAMPLE: &str = concat!(
        r#"{"type":"system","subtype":"init","session_id":"7702112f","model":"claude-haiku-4-5"}"#,
        "\n",
        r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"},"session_id":"7702112f"}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_01","role":"assistant","content":[]}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"The user wants"}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Nxe4 loses a knight — "}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"エンジンの読み筋では Qa4+ がフォークになります。"}},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"assistant","message":{"id":"msg_01","role":"assistant","content":[{"type":"text","text":"Nxe4 loses a knight — エンジンの読み筋では Qa4+ がフォークになります。"}]},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"content_block_stop","index":1},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"type":"stream_event","event":{"type":"message_stop"},"session_id":"7702112f","parent_tool_use_id":null}"#,
        "\n",
        r#"{"is_error":false,"num_turns":1,"stop_reason":"end_turn","session_id":"7702112f","subtype":"success","result":"Nxe4 loses a knight — エンジンの読み筋では Qa4+ がフォークになります。","type":"result","duration_ms":3928}"#,
        "\n",
    );

    const EXPECTED: &str = "Nxe4 loses a knight — エンジンの読み筋では Qa4+ がフォークになります。";

    fn collect(pieces: Vec<StreamPiece>, into: &mut String, failures: &mut Vec<String>) {
        for piece in pieces {
            match piece {
                StreamPiece::Text(text) => into.push_str(&text),
                StreamPiece::Failure(message) => failures.push(message),
            }
        }
    }

    fn run_in_chunks(input: &str, size: usize) -> (String, Vec<String>) {
        let bytes = input.as_bytes();
        let mut parser = StreamJsonParser::default();
        let mut text = String::new();
        let mut failures = Vec::new();
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

    #[test]
    fn thinking_deltas_are_not_emitted() {
        let (text, _) = run_in_chunks(SAMPLE, SAMPLE.len());
        assert!(!text.contains("The user wants"));
    }

    /// The bug this kind of parser always has: a chunk boundary landing in the
    /// middle of an event — including in the middle of a multi-byte character.
    #[test]
    fn survives_chunks_split_mid_event() {
        for size in [1, 2, 3, 7, 16, 63, 64, 100, 511, 1024] {
            let (text, failures) = run_in_chunks(SAMPLE, size);
            assert_eq!(text, EXPECTED, "chunk size {size}");
            assert!(failures.is_empty(), "chunk size {size}");
        }
    }

    #[test]
    fn a_line_without_a_trailing_newline_is_flushed_by_finish() {
        let trimmed = SAMPLE.trim_end_matches('\n');
        let (text, failures) = run_in_chunks(trimmed, 13);
        assert_eq!(text, EXPECTED);
        assert!(failures.is_empty());
    }

    #[test]
    fn falls_back_to_assistant_messages_without_partial_deltas() {
        let input = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Nxe4 is a blunder."}]},"parent_tool_use_id":null}"#,
            "\n",
            r#"{"type":"result","is_error":false,"subtype":"success"}"#,
            "\n",
        );
        let (text, failures) = run_in_chunks(input, 5);
        assert_eq!(text, "Nxe4 is a blunder.");
        assert!(failures.is_empty());
    }

    #[test]
    fn subagent_output_is_ignored() {
        let input = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"from a subagent"}]},"parent_tool_use_id":"toolu_01"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"the answer"}]},"parent_tool_use_id":null}"#,
            "\n",
        );
        let (text, _) = run_in_chunks(input, 9);
        assert_eq!(text, "the answer");
    }

    #[test]
    fn an_error_result_becomes_a_failure() {
        let input = concat!(
            r#"{"type":"result","is_error":true,"subtype":"error_during_execution","result":"model overloaded"}"#,
            "\n",
        );
        let (text, failures) = run_in_chunks(input, 4);
        assert!(text.is_empty());
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("model overloaded"));
    }

    #[test]
    fn unparseable_lines_are_skipped() {
        let input = concat!(
            "not json at all\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"ok"}}}"#,
            "\n",
            "{ still not json\n",
        );
        let (text, failures) = run_in_chunks(input, 6);
        assert_eq!(text, "ok");
        assert!(failures.is_empty());
    }

    #[tokio::test]
    async fn a_missing_binary_is_an_error_not_a_panic() {
        let provider = ClaudeCodeProvider {
            bin: "kibitz-no-such-binary-hopefully".into(),
        };
        let ctx = crate::test_support::fixture_context();
        let Err(err) = provider
            .explain("claude-haiku-4-5", Language::En, &ctx)
            .await
        else {
            panic!("a missing binary must be reported as an error");
        };
        assert!(matches!(err, LlmError::Process(_)), "{err:?}");
        assert!(err.to_string().contains("not found on PATH"), "{err}");
    }

    #[tokio::test]
    async fn a_non_zero_exit_becomes_a_stream_error() {
        // `false` exits 1 without writing anything to stdout.
        let provider = ClaudeCodeProvider {
            bin: "false".into(),
        };
        let ctx = crate::test_support::fixture_context();
        let Ok(mut stream) = provider
            .explain("claude-haiku-4-5", Language::En, &ctx)
            .await
        else {
            panic!("spawning `false` should succeed");
        };
        let first = stream.next().await.expect("one item");
        let Err(err) = first else {
            panic!("a non-zero exit must surface as an error");
        };
        assert!(matches!(err, LlmError::Process(_)), "{err:?}");
    }

    /// End-to-end against the real `claude` binary. Not run by default: it
    /// costs a live model call and needs Claude Code installed and logged in.
    ///
    /// Run it manually with:
    ///     cargo test -p kibitz-llm -- --ignored claude_code_end_to_end
    #[tokio::test]
    #[ignore = "spawns the real `claude` binary and makes a live model call"]
    async fn claude_code_end_to_end() {
        let provider = ClaudeCodeProvider::default();
        let ctx = crate::test_support::fixture_context();
        let Ok(mut stream) = provider
            .explain("claude-haiku-4-5", Language::En, &ctx)
            .await
        else {
            panic!("could not spawn the claude binary");
        };
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            text.push_str(&chunk.expect("no stream error"));
        }
        assert!(!text.trim().is_empty(), "the model said nothing");
    }
}
