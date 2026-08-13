//! Explanation generation.
//!
//! **The LLM's only job is putting structured data into words.** It makes no
//! judgements: `Classification` decides good or bad, `Counterfactual` decides why
//! a move fails, `center_control` decides whether the centre was taken.

use futures::stream::BoxStream;
use kibitz_core::types::AnalysisContext;

pub mod anthropic;
pub mod claude_code;
pub mod lang;
pub mod prompt;

#[cfg(test)]
mod test_support;

pub use lang::{Language, SUPPORTED_LANGUAGES};

pub type TextStream = BoxStream<'static, Result<String, LlmError>>;

/// One decoded piece of a provider's output stream. Both the `claude -p`
/// stream-json parser and the Anthropic SSE parser produce these, so the two
/// providers can share the same shape of test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StreamPiece {
    /// Incremental text to append to the explanation.
    Text(String),
    /// The provider reported a failure mid-stream.
    Failure(String),
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// `lang` is the language the explanation is written in, chosen by the user
    /// in the UI. It is a request parameter, never a build-time constant.
    async fn explain(
        &self,
        model: &str,
        lang: Language,
        ctx: &AnalysisContext,
    ) -> Result<TextStream, LlmError>;

    /// Follow-up questions. The model is run as an agent with the engine exposed
    /// as tools.
    async fn ask(
        &self,
        model: &str,
        lang: Language,
        session: &mut QaSession,
        question: &str,
    ) -> Result<TextStream, LlmError>;
}

/// Context for a follow-up conversation: the original `AnalysisContext` plus history.
#[derive(Debug, Clone, Default)]
pub struct QaSession {
    pub context: Option<AnalysisContext>,
    pub history: Vec<Turn>,
}

#[derive(Debug, Clone)]
pub struct Turn {
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
}

/// Different tasks need different capabilities, so the model is configured per task.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// Narration — restating structured data.
    pub narrate: String,
    /// Reasoning — strategic outlook and follow-up questions, where judgement enters.
    pub reason: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        ModelConfig {
            narrate: std::env::var("KIBITZ_MODEL_NARRATE")
                .unwrap_or_else(|_| "claude-haiku-4-5".into()),
            reason: std::env::var("KIBITZ_MODEL_REASON").unwrap_or_else(|_| "claude-opus-5".into()),
        }
    }
}

/// Selected by `KIBITZ_LLM=claude-code | anthropic`. Defaults to `claude-code`,
/// which runs inside an existing subscription and costs nothing extra.
pub fn from_env() -> Result<Box<dyn Provider>, LlmError> {
    let selected = std::env::var("KIBITZ_LLM").unwrap_or_default();
    provider_by_name(selected.trim())
}

/// The body of [`from_env`], split out so it is testable without touching the
/// process environment.
fn provider_by_name(name: &str) -> Result<Box<dyn Provider>, LlmError> {
    match name {
        "" | "claude-code" => Ok(Box::new(claude_code::ClaudeCodeProvider::default())),
        "anthropic" => Ok(Box::new(anthropic::AnthropicApiProvider::from_env()?)),
        other => Err(LlmError::UnknownProvider(other.to_string())),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("provider process failed: {0}")]
    Process(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("missing ANTHROPIC_API_KEY")]
    MissingApiKey,
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_selection_defaults_to_claude_code() {
        assert!(provider_by_name("").is_ok());
        assert!(provider_by_name("claude-code").is_ok());
    }

    #[test]
    fn unknown_provider_names_are_rejected() {
        match provider_by_name("openai") {
            Ok(_) => panic!("`openai` is not a provider kibitz knows about"),
            Err(err) => assert!(matches!(err, LlmError::UnknownProvider(name) if name == "openai")),
        }
    }

    #[test]
    fn anthropic_selection_needs_a_key() {
        // Without a key the selection fails cleanly rather than panicking.
        match provider_by_name("anthropic") {
            Ok(_) => assert!(std::env::var("ANTHROPIC_API_KEY").is_ok()),
            Err(err) => assert!(matches!(err, LlmError::MissingApiKey), "{err:?}"),
        }
    }

    #[test]
    fn model_config_defaults_match_the_design_doc() {
        // Only meaningful when the overrides are unset.
        if std::env::var("KIBITZ_MODEL_NARRATE").is_ok()
            || std::env::var("KIBITZ_MODEL_REASON").is_ok()
        {
            return;
        }
        let config = ModelConfig::default();
        assert_eq!(config.narrate, "claude-haiku-4-5");
        assert_eq!(config.reason, "claude-opus-5");
    }
}
