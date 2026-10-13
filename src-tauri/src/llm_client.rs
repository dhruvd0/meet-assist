use crate::settings::{PostProcessApiFormat, PostProcessProvider};
use log::{debug, error, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct JsonSchema {
    name: String,
    strict: bool,
    schema: Value,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    json_schema: JsonSchema,
}

#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude: Option<bool>,
}

/// Request fields used to ask an endpoint to skip reasoning/thinking.
/// Providers disagree on the field name and accepted values, so at most one of
/// these is set per request (see `reasoning_disable_params`).
#[derive(Debug, Serialize, Clone, Default, PartialEq)]
struct ReasoningParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
}

impl ReasoningParams {
    fn is_empty(&self) -> bool {
        self.reasoning_effort.is_none() && self.reasoning.is_none() && self.thinking.is_none()
    }
}

/// Pick the reasoning-disable request fields an endpoint understands.
/// Unknown endpoints get the common OpenAI-style field; if they reject it,
/// the request is retried without it (see `send_chat_completion_with_schema`).
fn reasoning_disable_params(provider: &PostProcessProvider) -> ReasoningParams {
    let base_url = provider.base_url.to_lowercase();
    if base_url.contains("api.deepseek.com") {
        // DeepSeek rejects reasoning_effort "none" and uses its own field:
        // https://api-docs.deepseek.com/guides/thinking_mode
        ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
    } else if provider.id == "openrouter" {
        // OpenRouter nested object; exclude:true also keeps reasoning text out
        // of the response so it can't pollute structured-output JSON parsing
        ReasoningParams {
            reasoning: Some(ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
            ..Default::default()
        }
    } else {
        ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
    }
}

/// Endpoints (base_url|model) that rejected the reasoning-disable fields with a
/// 4xx. Remembered for the lifetime of the process so every dictation after the
/// first skips the doomed attempt and goes straight to a plain request.
fn reasoning_rejections() -> &'static Mutex<HashSet<String>> {
    static REJECTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REJECTED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn endpoint_key(provider: &PostProcessProvider, model: &str) -> String {
    format!("{}|{}", provider.base_url.trim_end_matches('/'), model)
}

fn is_known_rejected(key: &str) -> bool {
    reasoning_rejections()
        .lock()
        .map(|set| set.contains(key))
        .unwrap_or(false)
}

fn remember_rejection(key: String) {
    if let Ok(mut set) = reasoning_rejections().lock() {
        set.insert(key);
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(flatten)]
    reasoning: ReasoningParams,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
}

/// Output budget for Anthropic Messages requests, where `max_tokens` is
/// required. Ample for a cleaned-up dictation.
const ANTHROPIC_MAX_TOKENS: u32 = 4096;

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    /// Thinking and any other non-text blocks
    #[serde(other)]
    Other,
}

impl AnthropicMessagesResponse {
    fn into_text(self) -> Option<String> {
        let text: String = self
            .content
            .into_iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text),
                AnthropicContentBlock::Other => None,
            })
            .collect();
        (!text.is_empty()).then_some(text)
    }
}

/// Build headers for API requests based on provider type
fn build_headers(provider: &PostProcessProvider, api_key: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    // Common headers
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://github.com/cjpais/Handy"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Handy/1.0 (+https://github.com/cjpais/Handy)"),
    );
    headers.insert("X-Title", HeaderValue::from_static("Handy"));

    // Provider-specific auth headers
    if !api_key.is_empty() {
        if provider.api_format == PostProcessApiFormat::Anthropic {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key)
                    .map_err(|e| format!("Invalid API key header value: {}", e))?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))
                    .map_err(|e| format!("Invalid authorization header value: {}", e))?,
            );
        }
    }

    Ok(headers)
}

/// Create an HTTP client with provider-specific headers
fn create_client(provider: &PostProcessProvider, api_key: &str) -> Result<reqwest::Client, String> {
    let headers = build_headers(provider, api_key)?;
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| report_reqwest_error("Failed to build HTTP client", &e))
}

/// Format a bounded error source chain.
///
/// `reqwest::Error`'s Display implementation intentionally gives only a short
/// summary. Nested causes contain the useful transport details, such as a
/// certificate validation failure, an HTTP/2 error, or a connection reset.
/// Callers must skip source types whose Display text can quote payload data.
fn error_source_chain(error: &(dyn StdError + 'static)) -> Vec<String> {
    let mut causes = Vec::new();
    let mut source = error.source();

    // Defensive cap in case a third-party error exposes a cyclic source chain.
    for _ in 0..16 {
        let Some(cause) = source else {
            break;
        };
        causes.push(cause.to_string());
        source = cause.source();
    }

    causes
}

fn reqwest_error_kinds(error: &reqwest::Error) -> String {
    let mut kinds = Vec::new();

    if error.is_builder() {
        kinds.push("builder");
    }
    if error.is_connect() {
        kinds.push("connect");
    }
    if error.is_request() {
        kinds.push("request");
    }
    if error.is_redirect() {
        kinds.push("redirect");
    }
    if error.is_timeout() {
        kinds.push("timeout");
    }
    if error.is_status() {
        kinds.push("status");
    }
    if error.is_body() {
        kinds.push("body");
    }
    if error.is_decode() {
        kinds.push("decode");
    }
    if error.is_upgrade() {
        kinds.push("upgrade");
    }

    if kinds.is_empty() {
        "unknown".to_string()
    } else {
        kinds.join(", ")
    }
}

fn sanitized_url(url: &reqwest::Url) -> String {
    let mut url = url.clone();

    // Custom endpoints should not contain credentials or query-string tokens,
    // but omit them from diagnostics in case one does.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    url.to_string()
}

fn sanitized_url_for_log(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|url| sanitized_url(&url))
        // Do not echo an invalid URL: the parse failure might have been caused
        // by sensitive data entered in the custom endpoint field.
        .unwrap_or_else(|_| "<invalid URL>".to_string())
}

fn report_reqwest_error(context: &str, error: &reqwest::Error) -> String {
    let kinds = reqwest_error_kinds(error);
    let url = error
        .url()
        .map(sanitized_url)
        .map(|url| format!(", url: {url}"))
        .unwrap_or_default();

    // serde_json's error text can quote values from a malformed response. That
    // response may contain transcription content, so retain the useful decode
    // classification but never put its nested source in logs or UI errors.
    let causes = if error.is_decode() {
        Vec::new()
    } else {
        error_source_chain(error)
    };
    let cause_details = if !causes.is_empty() {
        format!(": caused by: {}", causes.join(" -> "))
    } else if error.url().is_none() {
        // Reqwest's short Display text is safe when it cannot append a raw URL.
        format!(": {error}")
    } else {
        // The sanitized URL is already included above. Avoid formatting the
        // original error because its Display implementation includes the raw URL.
        String::new()
    };

    let details = format!("{context} (kind: {kinds}{url}){cause_details}");
    error!("{details}");
    details
}

/// Send a chat completion request to an OpenAI-compatible API
/// Returns Ok(Some(content)) on success, Ok(None) if response has no content,
/// or Err on actual errors (HTTP, parsing, etc.)
pub async fn send_chat_completion(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    disable_reasoning: bool,
) -> Result<Option<String>, String> {
    if provider.api_format == PostProcessApiFormat::Anthropic {
        return send_anthropic_message(provider, api_key, model, prompt).await;
    }

    send_chat_completion_with_schema(
        provider,
        api_key,
        model,
        prompt,
        None,
        None,
        disable_reasoning,
    )
    .await
}

/// Send a chat completion request with structured output support.
/// When json_schema is provided, uses structured outputs mode.
/// system_prompt is used as the system message when provided.
///
/// When disable_reasoning is set, the request carries the reasoning-disable
/// fields the endpoint is expected to understand. Not every OpenAI-compatible
/// endpoint accepts them (DeepSeek, Gemini's compat layer, and some OpenRouter
/// upstreams reject with 400), so a 400/422 answer to such a request triggers
/// one retry without the fields, and the rejection is remembered per
/// (base_url, model) so later requests skip the failing attempt entirely.
pub async fn send_chat_completion_with_schema(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    user_content: String,
    system_prompt: Option<String>,
    json_schema: Option<Value>,
    disable_reasoning: bool,
) -> Result<Option<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/chat/completions", base_url);

    debug!(
        "Sending chat completion request to: {}",
        sanitized_url_for_log(&url)
    );

    let client = create_client(provider, &api_key)?;

    // Build messages vector
    let mut messages = Vec::new();

    // Add system prompt if provided
    if let Some(system) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system,
        });
    }

    // Add user message
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_content,
    });

    // Build response_format if schema is provided
    let response_format = json_schema.map(|schema| ResponseFormat {
        format_type: "json_schema".to_string(),
        json_schema: JsonSchema {
            name: "transcription_output".to_string(),
            strict: true,
            schema,
        },
    });

    let key = endpoint_key(provider, model);
    let reasoning = if disable_reasoning && !is_known_rejected(&key) {
        reasoning_disable_params(provider)
    } else {
        ReasoningParams::default()
    };

    let mut request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages,
        stream: false,
        response_format,
        reasoning,
    };

    let mut response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;
    let mut status = response.status();
    debug!(
        "Chat completion response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );

    // A 400/422 on a request carrying reasoning-disable fields is almost always
    // the endpoint rejecting those fields — retry once without them.
    if !status.is_success()
        && matches!(status.as_u16(), 400 | 422)
        && !request_body.reasoning.is_empty()
    {
        let error_text = response.text().await.unwrap_or_else(|e| {
            report_reqwest_error("Failed to read reasoning rejection response", &e)
        });
        info!(
            "Endpoint rejected request with reasoning disabled (status {}): {}. Retrying without reasoning fields",
            status, error_text
        );

        request_body.reasoning = ReasoningParams::default();
        response = client
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| report_reqwest_error("HTTP retry failed", &e))?;
        status = response.status();
        debug!(
            "Chat completion retry response received with status {} over {:?} from {}",
            status,
            response.version(),
            sanitized_url(response.url())
        );

        if status.is_success() {
            info!(
                "Retry without reasoning fields succeeded; '{}' (model '{}') will skip them from now on",
                sanitized_url_for_log(base_url), model
            );
            remember_rejection(key);
        }
    }

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(format!(
            "API request failed with status {}: {}",
            status, error_text
        ));
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    let Some(choice) = completion.choices.into_iter().next() else {
        return Ok(None);
    };

    // A response cut off at the provider's output limit is incomplete; pasting
    // it would silently drop the end of the dictation (or, for structured
    // output, paste unparseable JSON).
    if choice.finish_reason.as_deref() == Some("length") {
        return Err("Response was truncated at the provider's output token limit".to_string());
    }

    Ok(choice.message.content)
}

/// Send a single-prompt request to an Anthropic Messages API (`/messages`).
/// Used by every provider with the Anthropic format (the built-in Anthropic
/// provider and a custom provider set to it). None of them use structured
/// output, and thinking is off unless requested, so no reasoning fields are
/// needed.
async fn send_anthropic_message(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
) -> Result<Option<String>, String> {
    let url = format!("{}/messages", provider.base_url.trim_end_matches('/'));

    debug!(
        "Sending Anthropic Messages request to: {}",
        sanitized_url_for_log(&url)
    );

    let request_body = AnthropicMessagesRequest {
        model: model.to_string(),
        max_tokens: ANTHROPIC_MAX_TOKENS,
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }],
    };

    let response = create_client(provider, &api_key)?
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;
    let status = response.status();
    debug!(
        "Anthropic Messages response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );

    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read API error response", &e));
        return Err(format!(
            "API request failed with status {}: {}",
            status, error_text
        ));
    }

    let message: AnthropicMessagesResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse API response", &e))?;

    // A response cut off at the token budget is incomplete; pasting it would
    // silently drop the end of the dictation.
    if message.stop_reason.as_deref() == Some("max_tokens") {
        return Err(format!(
            "Response was truncated at the {} token limit",
            ANTHROPIC_MAX_TOKENS
        ));
    }

    Ok(message.into_text())
}

/// Fetch available models from an OpenAI-compatible API
/// Returns a list of model IDs
pub async fn fetch_models(
    provider: &PostProcessProvider,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = provider.base_url.trim_end_matches('/');
    let url = format!("{}/models", base_url);

    debug!("Fetching models from: {}", sanitized_url_for_log(&url));

    let client = create_client(provider, &api_key)?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| report_reqwest_error("Failed to fetch models", &e))?;

    let status = response.status();
    debug!(
        "Model list response received with status {} over {:?} from {}",
        status,
        response.version(),
        sanitized_url(response.url())
    );
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|e| report_reqwest_error("Failed to read model list error", &e));
        return Err(format!(
            "Model list request failed ({}): {}",
            status, error_text
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse model list response", &e))?;

    let mut models = Vec::new();

    // Handle OpenAI format: { data: [ { id: "..." }, ... ] }
    if let Some(data) = parsed.get("data").and_then(|d| d.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|i| i.as_str()) {
                models.push(id.to_string());
            } else if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                models.push(name.to_string());
            }
        }
    }
    // Handle array format: [ "model1", "model2", ... ]
    else if let Some(array) = parsed.as_array() {
        for entry in array {
            if let Some(model) = entry.as_str() {
                models.push(model.to_string());
            }
        }
    }

    Ok(models)
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug)]
pub enum ToolLoopError {
    /// The endpoint rejected the `tools` field; caller should retry without tools.
    ToolsRejected(String),
    Other(String),
}

impl std::fmt::Display for ToolLoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolLoopError::ToolsRejected(s) => write!(f, "tools rejected: {s}"),
            ToolLoopError::Other(s) => write!(f, "{s}"),
        }
    }
}

pub fn is_tools_rejection(status: u16, body: &str) -> bool {
    (status == 400 || status == 422) && body.to_ascii_lowercase().contains("tool")
}

pub fn openai_tool_request(
    model: &str,
    messages: &[Value],
    tools: &[ToolDef],
    final_schema: Option<&Value>,
) -> Value {
    let mut req = serde_json::json!({ "model": model, "messages": messages, "stream": false });
    if !tools.is_empty() {
        req["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
                    })
                })
                .collect(),
        );
    }
    if let Some(schema) = final_schema {
        req["response_format"] = serde_json::json!({
            "type": "json_schema",
            "json_schema": { "name": "observer_verdict", "strict": true, "schema": schema }
        });
    }
    req
}

pub fn parse_openai_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>) {
    let msg = &resp["choices"][0]["message"];
    let text = msg["content"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let calls = msg["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c["id"].as_str()?.to_string();
                    let name = c["function"]["name"].as_str()?.to_string();
                    let raw = c["function"]["arguments"].as_str().unwrap_or("{}");
                    let arguments =
                        serde_json::from_str(raw).unwrap_or(Value::Object(Default::default()));
                    Some(ToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (text, calls)
}

pub fn openai_tool_result_message(call: &ToolCall, result: &str) -> Value {
    serde_json::json!({ "role": "tool", "tool_call_id": call.id, "content": result })
}

pub fn openai_assistant_tool_call_message(calls: &[ToolCall], text: Option<&str>) -> Value {
    serde_json::json!({
        "role": "assistant",
        "content": text,
        "tool_calls": calls.iter().map(|c| serde_json::json!({
            "id": c.id, "type": "function",
            "function": { "name": c.name, "arguments": c.arguments.to_string() }
        })).collect::<Vec<_>>()
    })
}

pub fn anthropic_tool_request(
    model: &str,
    system: &str,
    messages: &[Value],
    tools: &[ToolDef],
) -> Value {
    let mut req = serde_json::json!({
        "model": model, "max_tokens": ANTHROPIC_MAX_TOKENS, "system": system, "messages": messages
    });
    if !tools.is_empty() {
        req["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| serde_json::json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
                .collect(),
        );
    }
    req
}

pub fn parse_anthropic_tool_calls(resp: &Value) -> (Option<String>, Vec<ToolCall>) {
    let mut text = String::new();
    let mut calls = Vec::new();
    if let Some(blocks) = resp["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or("")),
                Some("tool_use") => calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                }),
                _ => {}
            }
        }
    }
    (if text.is_empty() { None } else { Some(text) }, calls)
}

pub fn anthropic_assistant_tool_use_message(calls: &[ToolCall], text: Option<&str>) -> Value {
    let mut content: Vec<Value> = Vec::new();
    if let Some(t) = text {
        content.push(serde_json::json!({ "type": "text", "text": t }));
    }
    for c in calls {
        content.push(serde_json::json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
    }
    serde_json::json!({ "role": "assistant", "content": content })
}

pub fn anthropic_tool_result_message(results: &[(ToolCall, String)]) -> Value {
    serde_json::json!({
        "role": "user",
        "content": results.iter().map(|(c, r)| serde_json::json!({
            "type": "tool_result", "tool_use_id": c.id, "content": r
        })).collect::<Vec<_>>()
    })
}

/// Truncate `s` to at most `max` bytes without splitting a UTF-8 character.
fn truncate_at_char_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
}

/// Run a tool-calling conversation until the model answers without tool
/// calls, or `max_tool_calls` is reached (then one final no-tools request
/// demanding the answer). Returns the final assistant text.
pub async fn send_with_tools(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    system_prompt: String,
    user_content: String,
    tools: &[ToolDef],
    final_schema: Value,
    max_tool_calls: usize,
    handler: &(dyn Fn(&ToolCall) -> String + Sync),
) -> Result<String, ToolLoopError> {
    let client = create_client(provider, &api_key).map_err(ToolLoopError::Other)?;
    let base = provider.base_url.trim_end_matches('/');
    let anthropic = provider.api_format == PostProcessApiFormat::Anthropic;
    let url = if anthropic {
        format!("{base}/messages")
    } else {
        format!("{base}/chat/completions")
    };

    let mut messages: Vec<Value> = Vec::new();
    if !anthropic {
        messages.push(serde_json::json!({ "role": "system", "content": system_prompt }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": user_content }));

    let mut calls_made = 0usize;
    let mut first_request = true;
    loop {
        let tools_now: &[ToolDef] = if calls_made >= max_tool_calls {
            &[]
        } else {
            tools
        };
        if calls_made >= max_tool_calls {
            messages.push(serde_json::json!({ "role": "user", "content": "Tool budget exhausted. Answer now with the final JSON only." }));
        }
        let body = if anthropic {
            anthropic_tool_request(model, &system_prompt, &messages, tools_now)
        } else {
            openai_tool_request(
                model,
                &messages,
                tools_now,
                if tools_now.is_empty() {
                    Some(&final_schema)
                } else {
                    None
                },
            )
        };
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolLoopError::Other(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| ToolLoopError::Other(e.to_string()))?;
        if !(200..300).contains(&status) {
            if first_request && !tools_now.is_empty() && is_tools_rejection(status, &text) {
                return Err(ToolLoopError::ToolsRejected(text));
            }
            return Err(ToolLoopError::Other(format!("HTTP {status}: {text}")));
        }
        first_request = false;
        let json: Value = serde_json::from_str(&text)
            .map_err(|e| ToolLoopError::Other(format!("bad json: {e}")))?;
        let (answer, calls) = if anthropic {
            parse_anthropic_tool_calls(&json)
        } else {
            parse_openai_tool_calls(&json)
        };
        if calls.is_empty() {
            return answer.ok_or_else(|| ToolLoopError::Other("empty answer".into()));
        }
        let mut results = Vec::new();
        for c in &calls {
            if calls_made >= max_tool_calls {
                results.push((c.clone(), "Tool budget exhausted; answer now.".to_string()));
                continue;
            }
            calls_made += 1;
            let mut r = handler(c);
            if r.len() > 4096 {
                truncate_at_char_boundary(&mut r, 4096);
                r.push_str("\n[truncated]");
            }
            results.push((c.clone(), r));
        }
        if anthropic {
            messages.push(anthropic_assistant_tool_use_message(
                &calls,
                answer.as_deref(),
            ));
            messages.push(anthropic_tool_result_message(&results));
        } else {
            messages.push(openai_assistant_tool_call_message(
                &calls,
                answer.as_deref(),
            ));
            for (c, r) in &results {
                messages.push(openai_tool_result_message(c, r));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Debug)]
    struct TestError {
        message: &'static str,
        source: Option<Box<TestError>>,
    }

    impl fmt::Display for TestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.message)
        }
    }

    impl StdError for TestError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source
                .as_deref()
                .map(|source| source as &(dyn StdError + 'static))
        }
    }

    fn provider(id: &str, base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            id: id.to_string(),
            label: id.to_string(),
            base_url: base_url.to_string(),
            allow_base_url_edit: true,
            models_endpoint: None,
            supports_structured_output: false,
            api_format: PostProcessApiFormat::OpenAi,
        }
    }

    fn anthropic_format_provider(base_url: &str) -> PostProcessProvider {
        PostProcessProvider {
            api_format: PostProcessApiFormat::Anthropic,
            ..provider("custom", base_url)
        }
    }

    fn request_json(reasoning: ReasoningParams) -> Value {
        let request = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            stream: false,
            response_format: None,
            reasoning,
        };
        serde_json::to_value(&request).unwrap()
    }

    async fn serve_one_response(status: &str, body: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        format!("http://{address}")
    }

    #[test]
    fn error_source_chain_includes_all_nested_causes() {
        let error = TestError {
            message: "request failed",
            source: Some(Box::new(TestError {
                message: "TLS handshake failed",
                source: Some(Box::new(TestError {
                    message: "unknown certificate authority",
                    source: None,
                })),
            })),
        };

        assert_eq!(
            error_source_chain(&error),
            vec!["TLS handshake failed", "unknown certificate authority"]
        );
    }

    #[test]
    fn log_url_sanitization_removes_credentials_and_tokens() {
        let url = "https://user:password@example.com/v1/models?api_key=secret#private";
        assert_eq!(sanitized_url_for_log(url), "https://example.com/v1/models");
    }

    #[test]
    fn invalid_log_urls_are_not_echoed() {
        assert_eq!(
            sanitized_url_for_log("not a URL containing secret"),
            "<invalid URL>"
        );
    }

    #[tokio::test]
    async fn decode_error_does_not_echo_response_values() {
        let base_url =
            serve_one_response("200 OK", r#"{"choices":"PRIVATE TRANSCRIPTION CONTENT"}"#).await;
        let error = reqwest::get(base_url)
            .await
            .unwrap()
            .json::<ChatCompletionResponse>()
            .await
            .unwrap_err();

        let details = report_reqwest_error("Failed to parse API response", &error);
        assert!(details.contains("kind: decode"));
        assert!(!details.contains("PRIVATE TRANSCRIPTION CONTENT"));
    }

    #[tokio::test]
    async fn raw_error_url_is_not_reintroduced_without_a_source() {
        let base_url = serve_one_response("400 Bad Request", "bad request").await;
        let error = reqwest::get(format!(
            "{base_url}/private?api_key=SECRET_QUERY_TOKEN#private"
        ))
        .await
        .unwrap()
        .error_for_status()
        .unwrap_err();

        let details = report_reqwest_error("Request failed", &error);
        assert!(details.contains(&format!("url: {base_url}/private")));
        assert!(!details.contains("SECRET_QUERY_TOKEN"));
        assert!(!details.contains("#private"));
    }

    #[test]
    fn requests_explicitly_disable_streaming() {
        let json = request_json(ReasoningParams::default());
        assert_eq!(json["stream"], false);
    }

    #[test]
    fn default_reasoning_params_serialize_to_no_fields() {
        let json = request_json(ReasoningParams::default());
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn custom_provider_uses_top_level_reasoning_effort() {
        let params = reasoning_disable_params(&provider("custom", "http://localhost:11434/v1"));
        let json = request_json(params);
        assert_eq!(json["reasoning_effort"], "none");
        assert!(json.get("reasoning").is_none());
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn openrouter_uses_nested_reasoning_object() {
        let params =
            reasoning_disable_params(&provider("openrouter", "https://openrouter.ai/api/v1"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert_eq!(json["reasoning"]["effort"], "none");
        assert_eq!(json["reasoning"]["exclude"], true);
        assert!(json.get("thinking").is_none());
    }

    #[test]
    fn deepseek_base_url_uses_thinking_disabled() {
        let params = reasoning_disable_params(&provider("custom", "https://api.deepseek.com"));
        let json = request_json(params);
        assert!(json.get("reasoning_effort").is_none());
        assert!(json.get("reasoning").is_none());
        assert_eq!(json["thinking"]["type"], "disabled");
    }

    #[test]
    fn reasoning_params_is_empty_tracks_all_fields() {
        assert!(ReasoningParams::default().is_empty());
        assert!(!ReasoningParams {
            reasoning_effort: Some("none".to_string()),
            ..Default::default()
        }
        .is_empty());
        assert!(!ReasoningParams {
            thinking: Some(serde_json::json!({ "type": "disabled" })),
            ..Default::default()
        }
        .is_empty());
    }

    #[test]
    fn rejection_memo_is_keyed_by_base_url_and_model() {
        let deepseek = provider("custom", "https://api.deepseek.com/");
        let key = endpoint_key(&deepseek, "deepseek-chat");
        assert_eq!(key, "https://api.deepseek.com|deepseek-chat");
        assert!(!is_known_rejected(&key));
        remember_rejection(key.clone());
        assert!(is_known_rejected(&key));
        // A different model on the same endpoint is tracked separately
        assert!(!is_known_rejected(&endpoint_key(&deepseek, "other-model")));
    }

    #[test]
    fn anthropic_format_uses_api_key_headers() {
        let headers = build_headers(
            &anthropic_format_provider("https://example.com/anthropic/v1"),
            "key",
        )
        .unwrap();
        assert_eq!(headers["x-api-key"], "key");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert!(headers.get(AUTHORIZATION).is_none());

        let headers =
            build_headers(&provider("custom", "http://localhost:11434/v1"), "key").unwrap();
        assert_eq!(headers[AUTHORIZATION], "Bearer key");
        assert!(headers.get("x-api-key").is_none());
    }

    #[test]
    fn anthropic_request_carries_required_max_tokens() {
        let request = AnthropicMessagesRequest {
            model: "claude-haiku".to_string(),
            max_tokens: ANTHROPIC_MAX_TOKENS,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["max_tokens"], ANTHROPIC_MAX_TOKENS);
        assert_eq!(json["messages"][0]["role"], "user");
        assert!(json.get("stream").is_none());
        assert!(json.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_response_keeps_only_text_blocks() {
        let response: AnthropicMessagesResponse = serde_json::from_str(
            r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Hello"},{"type":"text","text":" world"}]}"#,
        )
        .unwrap();
        assert_eq!(response.into_text().as_deref(), Some("Hello world"));

        let empty: AnthropicMessagesResponse = serde_json::from_str(r#"{"content":[]}"#).unwrap();
        assert_eq!(empty.into_text(), None);
    }

    #[tokio::test]
    async fn anthropic_format_parses_messages_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","role":"assistant","content":[{"type":"text","text":"Cleaned text"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            true,
        )
        .await;
        assert_eq!(result, Ok(Some("Cleaned text".to_string())));
    }

    #[tokio::test]
    async fn chat_completion_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"choices":[{"message":{"content":"Cut off"},"finish_reason":"length"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &provider("custom", &base_url),
            "key".to_string(),
            "test-model",
            "raw text".to_string(),
            false,
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }

    #[tokio::test]
    async fn anthropic_format_rejects_truncated_response() {
        let base_url = serve_one_response(
            "200 OK",
            r#"{"type":"message","stop_reason":"max_tokens","content":[{"type":"text","text":"Cut off"}]}"#,
        )
        .await;
        let result = send_chat_completion(
            &anthropic_format_provider(&base_url),
            "key".to_string(),
            "claude-haiku",
            "raw text".to_string(),
            true,
        )
        .await;
        assert!(result.unwrap_err().contains("truncated"));
    }
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use serde_json::json;

    fn tool() -> ToolDef {
        ToolDef {
            name: "search_context".into(),
            description: "Search project sources".into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        }
    }

    #[test]
    fn openai_request_carries_tools_and_schema() {
        let msgs = vec![json!({"role":"user","content":"hi"})];
        let schema = json!({"type":"object"});
        let req = openai_tool_request("m", &msgs, &[tool()], Some(&schema));
        assert_eq!(req["tools"][0]["type"], "function");
        assert_eq!(req["tools"][0]["function"]["name"], "search_context");
        assert_eq!(
            req["tools"][0]["function"]["parameters"]["required"][0],
            "query"
        );
        assert_eq!(req["response_format"]["type"], "json_schema");
        assert_eq!(req["stream"], false);
        let req2 = openai_tool_request("m", &msgs, &[], None);
        assert!(req2.get("tools").is_none());
        assert!(req2.get("response_format").is_none());
    }

    #[test]
    fn openai_response_tool_calls_parsed() {
        let resp = json!({"choices":[{"message":{"role":"assistant","content":null,
            "tool_calls":[{"id":"c1","type":"function","function":{"name":"search_context","arguments":"{\"query\":\"retry\"}"}}]},
            "finish_reason":"tool_calls"}]});
        let (text, calls) = parse_openai_tool_calls(&resp);
        assert!(text.is_none());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].arguments["query"], "retry");
        let resp2 = json!({"choices":[{"message":{"role":"assistant","content":"{\"intervene\":false}"},"finish_reason":"stop"}]});
        let (text, calls) = parse_openai_tool_calls(&resp2);
        assert_eq!(text.as_deref(), Some("{\"intervene\":false}"));
        assert!(calls.is_empty());
    }

    #[test]
    fn anthropic_request_and_response_shapes() {
        let msgs = vec![json!({"role":"user","content":"hi"})];
        let req = anthropic_tool_request("m", "sys", &msgs, &[tool()]);
        assert_eq!(req["system"], "sys");
        assert_eq!(req["tools"][0]["name"], "search_context");
        assert!(req["tools"][0]["input_schema"].is_object());
        assert_eq!(req["max_tokens"], 4096);
        let resp = json!({"content":[{"type":"text","text":"thinking"},{"type":"tool_use","id":"t1","name":"search_context","input":{"query":"x"}}],"stop_reason":"tool_use"});
        let (text, calls) = parse_anthropic_tool_calls(&resp);
        assert_eq!(text.as_deref(), Some("thinking"));
        assert_eq!(calls[0].id, "t1");
        assert_eq!(calls[0].arguments["query"], "x");
    }

    #[test]
    fn tool_result_messages_have_provider_shape() {
        let call = ToolCall {
            id: "c1".into(),
            name: "search_context".into(),
            arguments: json!({}),
        };
        let o = openai_tool_result_message(&call, "RESULT");
        assert_eq!(o["role"], "tool");
        assert_eq!(o["tool_call_id"], "c1");
        assert_eq!(o["content"], "RESULT");
        let a = anthropic_tool_result_message(&[(call.clone(), "RESULT".to_string())]);
        assert_eq!(a["role"], "user");
        assert_eq!(a["content"][0]["type"], "tool_result");
        assert_eq!(a["content"][0]["tool_use_id"], "c1");
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let mut s = "é".repeat(5000);
        truncate_at_char_boundary(&mut s, 4096);
        assert!(s.len() <= 4096);
        assert_eq!(s.chars().count(), 2048);
    }

    #[test]
    fn tools_rejected_detection() {
        assert!(is_tools_rejection(
            400,
            "Unrecognized request argument supplied: tools"
        ));
        assert!(is_tools_rejection(422, "tools is not supported"));
        assert!(!is_tools_rejection(400, "invalid api key"));
        assert!(!is_tools_rejection(500, "tools"));
    }
}
