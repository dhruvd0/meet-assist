//! MCP-shaped tools the observer can call. One implementation, reusable by a
//! future MCP server. Every handler returns compact JSON text and never panics.

use crate::llm_client::{ToolCall, ToolDef};
use crate::meeting::store::ProjectStore;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ToolContext {
    pub store: Arc<ProjectStore>,
    pub project_id: Option<i64>,
    pub meeting_id: i64,
}

pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_sources".into(),
            description: "List the project's sources with their short digests. Call this first when you need to know what material exists.".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
        ToolDef {
            name: "search_context".into(),
            description: "Full-text search across all project sources. Returns snippets with source_id and path.".into(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["query"],"additionalProperties":false}),
        },
        ToolDef {
            name: "read_source".into(),
            description: "Read chunks of one source. Use path to pick a file inside a folder source; offset and limit page through chunks.".into(),
            input_schema: json!({"type":"object","properties":{"source_id":{"type":"integer"},"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["source_id"],"additionalProperties":false}),
        },
        ToolDef {
            name: "read_transcript".into(),
            description: "Read transcript lines of this meeting (default) or another meeting in the project, optionally within a time range (unix ms).".into(),
            input_schema: json!({"type":"object","properties":{"meeting_id":{"type":"integer"},"from_ts":{"type":"integer"},"to_ts":{"type":"integer"},"limit":{"type":"integer","minimum":1,"maximum":200}},"additionalProperties":false}),
        },
        ToolDef {
            name: "list_meetings".into(),
            description: "List meetings in this project with dates and rolling summaries.".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        },
    ]
}

fn err(msg: impl std::fmt::Display) -> String {
    json!({ "error": msg.to_string() }).to_string()
}

pub fn handle_tool(ctx: &ToolContext, call: &ToolCall) -> String {
    match handle_inner(ctx, call) {
        Ok(v) => v.to_string(),
        Err(e) => err(e),
    }
}

fn handle_inner(ctx: &ToolContext, call: &ToolCall) -> Result<Value, String> {
    let a = &call.arguments;
    match call.name.as_str() {
        "list_sources" => {
            let Some(pid) = ctx.project_id else {
                return Ok(json!({ "sources": [] }));
            };
            let sources = ctx.store.list_sources(pid).map_err(|e| e.to_string())?;
            Ok(json!({ "sources": sources.iter().map(|s| json!({
                "id": s.id, "kind": s.kind, "title": s.title, "status": s.status, "digest": s.digest
            })).collect::<Vec<_>>() }))
        }
        "search_context" => {
            let query = a["query"].as_str().ok_or("query is required")?;
            let limit = a["limit"].as_u64().unwrap_or(8).clamp(1, 20) as usize;
            let Some(pid) = ctx.project_id else {
                return Ok(json!({ "hits": [] }));
            };
            let hits = ctx
                .store
                .search(pid, query, limit)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "hits": hits.iter().map(|h| json!({
                "source_id": h.source_id, "source_title": h.source_title, "path": h.path, "ord": h.ord, "snippet": h.snippet, "score": h.score
            })).collect::<Vec<_>>() }))
        }
        "read_source" => {
            let sid = a["source_id"].as_i64().ok_or("source_id is required")?;
            let source = ctx
                .store
                .get_source(sid)
                .map_err(|e| e.to_string())?
                .ok_or("source not found")?;
            if ctx.project_id.is_none() || Some(source.project_id) != ctx.project_id {
                return Err("source is not in this project".into());
            }
            let path = a["path"].as_str();
            let offset = a["offset"].as_u64().unwrap_or(0) as usize;
            let limit = a["limit"].as_u64().unwrap_or(2).clamp(1, 20) as usize;
            let chunks = ctx
                .store
                .read_chunks(sid, path, offset, limit)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({ "chunks": chunks.iter().map(|(p, o, c)| json!({ "path": p, "ord": o, "content": c })).collect::<Vec<_>>() }),
            )
        }
        "read_transcript" => {
            let mid = a["meeting_id"].as_i64().unwrap_or(ctx.meeting_id);
            if mid != ctx.meeting_id {
                let m = ctx
                    .store
                    .get_meeting(mid)
                    .map_err(|e| e.to_string())?
                    .ok_or("meeting not found")?;
                if m.project_id.is_none() || m.project_id != ctx.project_id {
                    return Err("meeting is not in this project".into());
                }
            }
            let limit = a["limit"].as_u64().unwrap_or(100).clamp(1, 200) as usize;
            let lines = ctx
                .store
                .lines_in_range(mid, a["from_ts"].as_i64(), a["to_ts"].as_i64(), limit)
                .map_err(|e| e.to_string())?;
            Ok(
                json!({ "lines": lines.iter().map(|l| json!({ "ts": l.ts, "speaker": l.speaker, "text": l.text })).collect::<Vec<_>>() }),
            )
        }
        "list_meetings" => {
            let Some(pid) = ctx.project_id else {
                return Ok(json!({ "meetings": [] }));
            };
            let meetings = ctx
                .store
                .list_meetings(Some(pid))
                .map_err(|e| e.to_string())?;
            Ok(json!({ "meetings": meetings.iter().map(|m| json!({
                "id": m.id, "title": m.title, "status": m.status, "created_at": m.created_at, "updated_at": m.updated_at, "rolling_summary": m.rolling_summary
            })).collect::<Vec<_>>() }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
