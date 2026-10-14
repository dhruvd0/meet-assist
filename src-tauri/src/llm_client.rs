use crate::settings::{PostProcessApiFormat, PostProcessProvider};
use log::{debug, error, info};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, REFERER, USER_AGENT};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::sync::{Mutex, OnceLock};

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

/// Output budget for Anthropic Messages requests, where `max_tokens` is
/// required.
const ANTHROPIC_MAX_TOKENS: u32 = 4096;

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

/// Narrow heuristic: the error body names the `tools` field. Kept as a
/// diagnostic for logs; the loop itself falls back on a broader rule
/// (`should_fallback_without_tools`), because providers word this many ways.
pub fn is_tools_rejection(status: u16, body: &str) -> bool {
    (status == 400 || status == 422) && body.to_ascii_lowercase().contains("tool")
}

/// Whether a non-2xx answer to the first tooled request should trigger the
/// no-tools fallback. Only client errors that could plausibly be the endpoint
/// choking on `tools`/`tool_choice` qualify. Auth (401/403), missing route
/// (404), quota or rate limits (429) and server errors (5xx) would fail the
/// same way without tools, and must surface as the real error.
pub fn should_fallback_without_tools(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 401 | 403 | 404 | 429)
}

/// Build an OpenAI-format tool request. `tool_choice_none` keeps the tool
/// definitions (so earlier tool messages stay valid) but forbids new calls.
pub fn openai_tool_request(
    model: &str,
    messages: &[Value],
    tools: &[ToolDef],
    final_schema: Option<&Value>,
    tool_choice_none: bool,
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
        if tool_choice_none {
            req["tool_choice"] = Value::String("none".into());
        }
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

/// Build an Anthropic Messages tool request. `tool_choice_none` keeps the
/// tool definitions (required once `tool_use` blocks are in the history) but
/// forbids new calls.
pub fn anthropic_tool_request(
    model: &str,
    system: &str,
    messages: &[Value],
    tools: &[ToolDef],
    tool_choice_none: bool,
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
        if tool_choice_none {
            req["tool_choice"] = serde_json::json!({ "type": "none" });
        }
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

/// Merge the reasoning-disable fields into an OpenAI-format JSON body.
fn merge_reasoning(body: &mut Value, reasoning: &ReasoningParams) {
    if let (Some(obj), Ok(Value::Object(fields))) =
        (body.as_object_mut(), serde_json::to_value(reasoning))
    {
        for (k, v) in fields {
            obj.insert(k, v);
        }
    }
}

/// Run a tool-calling conversation until the model answers without tool
/// calls, or `max_tool_calls` is reached (then one final request with
/// `tool_choice: none` demanding the answer). Returns the final assistant
/// text. Builds a fresh client; long-lived callers should hold one and use
/// `send_with_tools_with_client`.
#[allow(clippy::too_many_arguments)] // mirrors the request shape; a params struct adds no clarity
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
    send_with_tools_with_client(
        &client,
        provider,
        model,
        system_prompt,
        user_content,
        tools,
        final_schema,
        max_tool_calls,
        handler,
    )
    .await
}

/// Build the HTTP client `send_with_tools_with_client` expects for a provider.
pub fn create_tool_client(
    provider: &PostProcessProvider,
    api_key: &str,
) -> Result<reqwest::Client, String> {
    create_client(provider, api_key)
}

/// `send_with_tools` over a caller-owned client (built by `create_tool_client`
/// with the provider's auth headers), so connections are reused across calls.
#[allow(clippy::too_many_arguments)] // mirrors the request shape; a params struct adds no clarity
pub async fn send_with_tools_with_client(
    client: &reqwest::Client,
    provider: &PostProcessProvider,
    model: &str,
    system_prompt: String,
    user_content: String,
    tools: &[ToolDef],
    final_schema: Value,
    max_tool_calls: usize,
    handler: &(dyn Fn(&ToolCall) -> String + Sync),
) -> Result<String, ToolLoopError> {
    let base = provider.base_url.trim_end_matches('/');
    let anthropic = provider.api_format == PostProcessApiFormat::Anthropic;
    let url = if anthropic {
        format!("{base}/messages")
    } else {
        format!("{base}/chat/completions")
    };
    let key = endpoint_key(provider, model);
    // Anthropic-format endpoints never get reasoning fields (as in
    // `send_anthropic_message`); OpenAI-format ones get the same fields as
    // dictation post-processing unless this endpoint already rejected them.
    let mut reasoning = if anthropic || is_known_rejected(&key) {
        ReasoningParams::default()
    } else {
        reasoning_disable_params(provider)
    };

    let mut messages: Vec<Value> = Vec::new();
    if !anthropic {
        messages.push(serde_json::json!({ "role": "system", "content": system_prompt }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": user_content }));

    let mut calls_made = 0usize;
    let mut first_request = true;
    loop {
        let final_turn = calls_made >= max_tool_calls;
        if final_turn && !tools.is_empty() {
            messages.push(serde_json::json!({ "role": "user", "content": "Tool budget exhausted. Answer now with the final JSON only." }));
        }
        let tool_choice_none = final_turn && !tools.is_empty();
        let schema = if provider.supports_structured_output && (final_turn || tools.is_empty()) {
            Some(&final_schema)
        } else {
            None
        };
        let build = |reasoning: &ReasoningParams| {
            if anthropic {
                anthropic_tool_request(model, &system_prompt, &messages, tools, tool_choice_none)
            } else {
                let mut body =
                    openai_tool_request(model, &messages, tools, schema, tool_choice_none);
                merge_reasoning(&mut body, reasoning);
                body
            }
        };
        let (mut status, mut text) = post_json(client, &url, &build(&reasoning)).await?;
        // A 400/422 on a request carrying reasoning-disable fields is most
        // likely the endpoint rejecting them: retry once without and remember.
        if matches!(status, 400 | 422) && !reasoning.is_empty() {
            info!(
                "Tool loop: endpoint rejected reasoning fields (status {status}); retrying without them"
            );
            reasoning = ReasoningParams::default();
            (status, text) = post_json(client, &url, &build(&reasoning)).await?;
            if (200..300).contains(&status) {
                remember_rejection(key.clone());
            }
        }
        if !(200..300).contains(&status) {
            if first_request && !tools.is_empty() && should_fallback_without_tools(status) {
                if !is_tools_rejection(status, &text) {
                    debug!("Tool loop: HTTP {status} on first tooled request; treating as tools rejection");
                }
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
        if calls.is_empty() || final_turn {
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

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Result<(u16, String), ToolLoopError> {
    let resp =
        client.post(url).json(body).send().await.map_err(|e| {
            ToolLoopError::Other(report_reqwest_error("Tool loop request failed", &e))
        })?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| ToolLoopError::Other(e.to_string()))?;
    Ok((status, text))
}

/// Strip markdown fences or surrounding prose from a model's JSON answer.
pub fn strip_json_fences(text: &str) -> &str {
    let t = text.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    // Fall back to the outermost braces if the model wrapped the JSON in prose.
    match (t.find('{'), t.rfind('}')) {
        (Some(s), Some(e)) if e > s => &t[s..=e],
        _ => t,
    }
}

/// Single-shot JSON completion that works across provider formats. The
/// schema is passed through `send_with_tools` (which gates it on provider
/// support); fences/prose are stripped from the answer before parsing.
#[allow(dead_code)] // consumed by the summary/digest lanes
pub async fn complete_json(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    system_prompt: String,
    user_content: String,
    schema: Value,
) -> Result<Value, String> {
    let handler = |_c: &ToolCall| String::new();
    let text = send_with_tools(
        provider,
        api_key,
        model,
        system_prompt,
        user_content,
        &[],
        schema,
        0,
        &handler,
    )
    .await
    .map_err(|e| format!("{e}"))?;
    serde_json::from_str::<Value>(strip_json_fences(&text)).map_err(|e| format!("json parse: {e}"))
}

/// MIME type for a supported image file extension.
#[allow(dead_code)]
pub fn mime_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

/// True when an HTTP error looks like the model refusing image input.
pub fn is_vision_rejection(status: u16, body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    (status == 400 || status == 422)
        && (b.contains("image")
            || b.contains("vision")
            || b.contains("multimodal")
            || b.contains("image_url")
            || b.contains("media_type"))
}

pub fn openai_image_message(prompt: &str, mime: &str, b64: &str) -> Value {
    serde_json::json!({ "role": "user", "content": [
        { "type": "text", "text": prompt },
        { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{b64}") } }
    ]})
}

pub fn anthropic_image_message(prompt: &str, mime: &str, b64: &str) -> Value {
    serde_json::json!({ "role": "user", "content": [
        { "type": "image", "source": { "type": "base64", "media_type": mime, "data": b64 } },
        { "type": "text", "text": prompt }
    ]})
}

/// Ask the model to describe an image. Returns `Err("vision_unsupported")`
/// when the provider/model rejects image input so callers can fall back.
#[allow(dead_code)] // consumed by the attachment indexing lane
pub async fn describe_image(
    provider: &PostProcessProvider,
    api_key: String,
    model: &str,
    prompt: String,
    mime: &str,
    bytes: &[u8],
) -> Result<String, String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let client = create_client(provider, &api_key)?;
    let base = provider.base_url.trim_end_matches('/');
    let anthropic = provider.api_format == PostProcessApiFormat::Anthropic;
    let (url, body) = if anthropic {
        (
            format!("{base}/messages"),
            serde_json::json!({
                "model": model, "max_tokens": 1024,
                "messages": [anthropic_image_message(&prompt, mime, &b64)]
            }),
        )
    } else {
        (
            format!("{base}/chat/completions"),
            serde_json::json!({
                "model": model, "stream": false,
                "messages": [openai_image_message(&prompt, mime, &b64)]
            }),
        )
    };
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        if is_vision_rejection(status, &text) {
            return Err("vision_unsupported".into());
        }
        return Err(format!("HTTP {status}: {text}"));
    }
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("bad json: {e}"))?;
    let (answer, _) = if anthropic {
        parse_anthropic_tool_calls(&json)
    } else {
        parse_openai_tool_calls(&json)
    };
    answer
        .map(|s| s.trim().to_string())
        .ok_or_else(|| "empty answer".into())
}
