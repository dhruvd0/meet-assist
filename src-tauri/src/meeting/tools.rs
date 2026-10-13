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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::store::{ProjectStore, SourceKind};
    use serde_json::{json, Value};

    fn ctx() -> (tempfile::TempDir, ToolContext) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let p = store.create_project("P").unwrap();
        let src = store
            .add_source(p.id, SourceKind::Text, "Spec", "")
            .unwrap();
        store.set_source_digest(src.id, "Spec digest").unwrap();
        store
            .replace_chunks(
                src.id,
                &[("".into(), "The retry budget is five attempts.".into())],
            )
            .unwrap();
        let m = store.create_meeting(Some(p.id), "Standup").unwrap();
        let sess = store.create_session(m.id).unwrap();
        store
            .append_line(sess.id, "me", "we keep circling the retry budget", 1000)
            .unwrap();
        (
            dir,
            ToolContext {
                store,
                project_id: Some(p.id),
                meeting_id: m.id,
            },
        )
    }

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            arguments: args,
        }
    }

    #[test]
    fn defs_have_five_tools_with_schemas() {
        let defs = tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "list_sources",
                "search_context",
                "read_source",
                "read_transcript",
                "list_meetings"
            ]
        );
        assert!(defs.iter().all(|d| d.input_schema["type"] == "object"));
    }

    #[test]
    fn list_sources_returns_digest_not_locator() {
        let (_d, c) = ctx();
        let out: Value =
            serde_json::from_str(&handle_tool(&c, &call("list_sources", json!({})))).unwrap();
        assert_eq!(out["sources"][0]["title"], "Spec");
        assert_eq!(out["sources"][0]["digest"], "Spec digest");
        assert!(out["sources"][0].get("locator").is_none());
    }

    #[test]
    fn search_and_read_source() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("search_context", json!({"query":"retry budget"})),
        ))
        .unwrap();
        assert_eq!(out["hits"].as_array().unwrap().len(), 1);
        let sid = out["hits"][0]["source_id"].as_i64().unwrap();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("read_source", json!({"source_id": sid})),
        ))
        .unwrap();
        assert!(out["chunks"][0]["content"]
            .as_str()
            .unwrap()
            .contains("five"));
    }

    #[test]
    fn read_source_defaults_to_two_chunks() {
        let (_d, c) = ctx();
        let pid = c.project_id.unwrap();
        let src = c
            .store
            .add_source(pid, SourceKind::Text, "Long", "")
            .unwrap();
        let chunks: Vec<(String, String)> =
            (0..5).map(|i| ("".into(), format!("chunk {i}"))).collect();
        c.store.replace_chunks(src.id, &chunks).unwrap();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("read_source", json!({"source_id": src.id})),
        ))
        .unwrap();
        assert_eq!(out["chunks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn read_transcript_and_list_meetings() {
        let (_d, c) = ctx();
        let out: Value =
            serde_json::from_str(&handle_tool(&c, &call("read_transcript", json!({})))).unwrap();
        assert_eq!(out["lines"][0]["text"], "we keep circling the retry budget");
        let out: Value =
            serde_json::from_str(&handle_tool(&c, &call("list_meetings", json!({})))).unwrap();
        assert_eq!(out["meetings"][0]["title"], "Standup");
    }

    #[test]
    fn unknown_tool_and_bad_args_return_error_json() {
        let (_d, c) = ctx();
        let out: Value = serde_json::from_str(&handle_tool(&c, &call("nope", json!({})))).unwrap();
        assert!(out["error"].as_str().unwrap().contains("unknown tool"));
        let out: Value =
            serde_json::from_str(&handle_tool(&c, &call("read_source", json!({})))).unwrap();
        assert!(out["error"].is_string());
    }

    #[test]
    fn quick_meeting_without_project_has_no_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(ProjectStore::open(dir.path().join("meet.db")).unwrap());
        let m = store.create_meeting(None, "Quick").unwrap();
        let c = ToolContext {
            store,
            project_id: None,
            meeting_id: m.id,
        };
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("search_context", json!({"query":"x"})),
        ))
        .unwrap();
        assert_eq!(out["hits"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn read_source_rejects_other_project_and_quick_meeting() {
        let (_d, c) = ctx();
        let p2 = c.store.create_project("Other").unwrap();
        let s2 = c
            .store
            .add_source(p2.id, SourceKind::Text, "Secret", "")
            .unwrap();
        c.store
            .replace_chunks(s2.id, &[("".into(), "hidden".into())])
            .unwrap();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("read_source", json!({"source_id": s2.id})),
        ))
        .unwrap();
        assert!(out["error"].is_string());
        let q = ToolContext {
            store: c.store.clone(),
            project_id: None,
            meeting_id: c.meeting_id,
        };
        let out: Value = serde_json::from_str(&handle_tool(
            &q,
            &call("read_source", json!({"source_id": s2.id})),
        ))
        .unwrap();
        assert!(out["error"].is_string());
    }

    #[test]
    fn read_transcript_rejects_meeting_from_other_project() {
        let (_d, c) = ctx();
        let p2 = c.store.create_project("Other").unwrap();
        let m2 = c.store.create_meeting(Some(p2.id), "Elsewhere").unwrap();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("read_transcript", json!({"meeting_id": m2.id})),
        ))
        .unwrap();
        assert!(out["error"].is_string());
        let m3 = c.store.create_meeting(c.project_id, "Same").unwrap();
        let out: Value = serde_json::from_str(&handle_tool(
            &c,
            &call("read_transcript", json!({"meeting_id": m3.id})),
        ))
        .unwrap();
        assert!(out["error"].is_null());
        assert!(out["lines"].is_array());
    }
}
