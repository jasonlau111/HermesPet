use base64::Engine;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::fs;
use std::path::PathBuf;
use std::slice;
use std::time::{SystemTime, UNIX_EPOCH};

#[no_mangle]
pub extern "C" fn hermes_rust_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[no_mangle]
pub extern "C" fn hermes_parse_opencode_sse_event(
    json_ptr: *const u8,
    json_len: usize,
    session_ptr: *const u8,
    session_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(json_ptr, json_len) }) else {
        return std::ptr::null_mut();
    };
    let target_session = unsafe { read_utf8(session_ptr, session_len) }.unwrap_or("");
    let Ok(event) = serde_json::from_str::<Value>(raw) else {
        return std::ptr::null_mut();
    };

    let Some(event_type) = event.get("type").and_then(Value::as_str) else {
        return json_string(json!({"kind":"ignore"}));
    };
    let Some(props) = event.get("properties") else {
        return json_string(json!({"kind":"ignore"}));
    };
    if let Some(session_id) = props.get("sessionID").and_then(Value::as_str) {
        if !target_session.is_empty() && session_id != target_session {
            return json_string(json!({"kind":"ignore"}));
        }
    }

    match event_type {
        "message.part.delta" => {
            let field = props.get("field").and_then(Value::as_str).unwrap_or("");
            let delta = props.get("delta").and_then(Value::as_str).unwrap_or("");
            if field == "text" && !delta.is_empty() {
                json_string(json!({"kind":"text_delta","text":delta}))
            } else {
                json_string(json!({"kind":"ignore"}))
            }
        }
        "message.part.updated" => parse_part_updated(props),
        "permission.asked" => json_string(json!({"kind":"permission_asked","payload":props})),
        "permission.replied" => {
            let request_id = props.get("requestID").and_then(Value::as_str).unwrap_or("");
            json_string(json!({"kind":"permission_replied","requestID":request_id}))
        }
        "question.asked" => json_string(json!({"kind":"question_asked","payload":props})),
        "question.replied" | "question.rejected" => {
            let request_id = props.get("requestID").and_then(Value::as_str).unwrap_or("");
            json_string(json!({"kind":"question_dismissed","requestID":request_id}))
        }
        _ => json_string(json!({"kind":"ignore"})),
    }
}

#[no_mangle]
pub extern "C" fn hermes_index_messages_json(
    json_ptr: *const u8,
    json_len: usize,
    visible_limit: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(json_ptr, json_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(messages) = serde_json::from_str::<Value>(raw) else {
        return std::ptr::null_mut();
    };
    let Some(items) = messages.as_array() else {
        return std::ptr::null_mut();
    };

    let total = items.len();
    let start = total.saturating_sub(visible_limit);
    let mut ids = Vec::with_capacity(total.saturating_sub(start));
    let mut total_chars = 0usize;
    let mut streaming_ids = Vec::new();

    for item in items {
        if let Some(content) = item.get("content").and_then(Value::as_str) {
            total_chars += content.chars().count();
        }
        if item
            .get("isStreaming")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                streaming_ids.push(id.to_string());
            }
        }
    }
    for item in items.iter().skip(start) {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            ids.push(id.to_string());
        }
    }

    json_string(json!({
        "total": total,
        "visibleStart": start,
        "visibleIDs": ids,
        "hidden": start,
        "totalChars": total_chars,
        "streamingIDs": streaming_ids
    }))
}

#[no_mangle]
pub extern "C" fn hermes_tts_readable_text(text_ptr: *const u8, text_len: usize) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(text_ptr, text_len) }) else {
        return std::ptr::null_mut();
    };
    json_string(json!({
        "text": readable_text(raw)
    }))
}

#[no_mangle]
pub extern "C" fn hermes_build_mimo_tts_request_json(
    options_ptr: *const u8,
    options_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<Value>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid options json"}));
    };

    let text = options.get("text").and_then(Value::as_str).unwrap_or("");
    let text = readable_text(text);
    if text.trim().is_empty() {
        return json_string(json!({"ok": false, "error": "empty text"}));
    }

    let base_url = options
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("https://token-plan-sgp.xiaomimimo.com/v1")
        .trim_end_matches('/');
    let model = options
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("mimo-v2.5-tts");
    let voice = options
        .get("voice")
        .and_then(Value::as_str)
        .unwrap_or("冰糖");
    let style_prompt = options
        .get("stylePrompt")
        .and_then(Value::as_str)
        .unwrap_or("");
    let voice_design_desc = options
        .get("voiceDesignDesc")
        .and_then(Value::as_str)
        .unwrap_or("");

    let user_content = if model == "mimo-v2.5-tts-voicedesign" {
        let design_desc = if voice_design_desc.trim().is_empty() {
            "默认音色"
        } else {
            voice_design_desc
        };
        if style_prompt.trim().is_empty() {
            design_desc.to_string()
        } else {
            format!("{}\n风格指令：{}", design_desc, style_prompt)
        }
    } else {
        style_prompt.to_string()
    };

    let mut audio = json!({"format": "wav"});
    if model != "mimo-v2.5-tts-voicedesign" {
        audio["voice"] = json!(voice);
    }

    json_string(json!({
        "ok": true,
        "url": format!("{}/chat/completions", base_url),
        "body": {
            "model": model,
            "messages": [
                {"role": "user", "content": user_content},
                {"role": "assistant", "content": text}
            ],
            "audio": audio
        }
    }))
}

#[no_mangle]
pub extern "C" fn hermes_mimo_cache_audio_from_response(
    response_ptr: *const u8,
    response_len: usize,
    cache_dir_ptr: *const u8,
    cache_dir_len: usize,
    cache_key_ptr: *const u8,
    cache_key_len: usize,
) -> *mut c_char {
    let Some(response_raw) = (unsafe { read_utf8(response_ptr, response_len) }) else {
        return std::ptr::null_mut();
    };
    let Some(cache_dir) = (unsafe { read_utf8(cache_dir_ptr, cache_dir_len) }) else {
        return std::ptr::null_mut();
    };
    let cache_key = unsafe { read_utf8(cache_key_ptr, cache_key_len) }.unwrap_or("tts");
    let Ok(response) = serde_json::from_str::<Value>(response_raw) else {
        return json_string(json!({"ok": false, "error": "invalid response json"}));
    };
    let Some(audio_base64) = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("message"))
        .and_then(|message| message.get("audio"))
        .and_then(|audio| audio.get("data"))
        .and_then(Value::as_str)
    else {
        return json_string(json!({"ok": false, "error": "audio data not found"}));
    };

    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(audio_base64) else {
        return json_string(json!({"ok": false, "error": "invalid base64 audio"}));
    };

    let dir = PathBuf::from(cache_dir);
    if let Err(err) = fs::create_dir_all(&dir) {
        return json_string(
            json!({"ok": false, "error": format!("create cache dir failed: {}", err)}),
        );
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("mimo-{}-{}.wav", stamp, sanitize_file_stem(cache_key));
    let path = dir.join(file_name);
    if let Err(err) = fs::write(&path, &bytes) {
        return json_string(json!({"ok": false, "error": format!("write audio failed: {}", err)}));
    }

    json_string(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "byteCount": bytes.len(),
        "format": "wav"
    }))
}

#[no_mangle]
pub extern "C" fn hermes_validate_text(
    kind_ptr: *const u8,
    kind_len: usize,
    text_ptr: *const u8,
    text_len: usize,
) -> *mut c_char {
    let kind = unsafe { read_utf8(kind_ptr, kind_len) }.unwrap_or("json");
    let Some(text) = (unsafe { read_utf8(text_ptr, text_len) }) else {
        return std::ptr::null_mut();
    };
    let result = match kind {
        "yaml" | "yml" => serde_yaml::from_str::<serde_yaml::Value>(text)
            .map(|_| json!({"ok": true}))
            .unwrap_or_else(|err| json!({"ok": false, "error": err.to_string()})),
        _ => serde_json::from_str::<Value>(text)
            .map(|_| json!({"ok": true}))
            .unwrap_or_else(|err| json!({"ok": false, "error": err.to_string()})),
    };
    json_string(result)
}

#[no_mangle]
pub extern "C" fn hermes_normalize_permission_payload(
    json_ptr: *const u8,
    json_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(json_ptr, json_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(payload) = serde_json::from_str::<Value>(raw) else {
        return json_string(json!({"ok": false, "error": "bad json"}));
    };
    let tool_name = payload
        .get("tool_name")
        .or_else(|| payload.get("toolName"))
        .or_else(|| payload.get("tool"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown");
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionID"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let tool_input = payload
        .get("tool_input")
        .or_else(|| payload.get("toolInput"))
        .cloned()
        .unwrap_or(Value::Object(Default::default()));

    json_string(json!({
        "ok": true,
        "toolName": tool_name,
        "sessionID": session_id,
        "toolInput": tool_input
    }))
}

#[no_mangle]
pub extern "C" fn hermes_parse_opencode_listening_port(
    line_ptr: *const u8,
    line_len: usize,
) -> *mut c_char {
    let Some(line) = (unsafe { read_utf8(line_ptr, line_len) }) else {
        return std::ptr::null_mut();
    };
    let port = parse_listening_port(line);
    json_string(match port {
        Some(port) => json!({"ok": true, "port": port}),
        None => json!({"ok": false}),
    })
}

#[no_mangle]
pub extern "C" fn hermes_parse_opencode_health_json(
    json_ptr: *const u8,
    json_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(json_ptr, json_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return json_string(json!({"ok": false, "healthy": false, "error": "bad json"}));
    };
    let healthy = value
        .get("healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    json_string(json!({
        "ok": true,
        "healthy": healthy,
        "version": value.get("version").and_then(Value::as_str).unwrap_or("")
    }))
}

#[no_mangle]
pub extern "C" fn hermes_periodic_review_prepare(
    options_ptr: *const u8,
    options_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<PeriodicReviewOptions>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid periodic review options"}));
    };

    match build_periodic_review_prompt(&options) {
        Ok(value) => json_string(value),
        Err(err) => json_string(json!({"ok": false, "error": err})),
    }
}

#[no_mangle]
pub extern "C" fn hermes_growth_timeline_load(
    options_ptr: *const u8,
    options_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<TimelineOptions>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid timeline options"}));
    };

    match load_timeline(&options.timeline_path) {
        Ok(store) => json_string(json!({"ok": true, "entries": store.entries})),
        Err(err) => json_string(json!({"ok": false, "error": err})),
    }
}

#[no_mangle]
pub extern "C" fn hermes_growth_timeline_save(
    options_ptr: *const u8,
    options_len: usize,
    review_ptr: *const u8,
    review_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Some(review_text) = (unsafe { read_utf8(review_ptr, review_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<PeriodicReviewOptions>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid periodic review options"}));
    };

    match save_timeline_entry(&options, review_text) {
        Ok((entry, entries)) => json_string(json!({"ok": true, "entry": entry, "entries": entries})),
        Err(err) => json_string(json!({"ok": false, "error": err})),
    }
}

#[no_mangle]
pub extern "C" fn hermes_growth_timeline_mark_synced(
    options_ptr: *const u8,
    options_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<TimelineSyncOptions>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid sync options"}));
    };

    match mark_timeline_entry_synced(&options) {
        Ok(entries) => json_string(json!({"ok": true, "entries": entries})),
        Err(err) => json_string(json!({"ok": false, "error": err})),
    }
}

#[no_mangle]
pub extern "C" fn hermes_growth_timeline_clear(
    options_ptr: *const u8,
    options_len: usize,
) -> *mut c_char {
    let Some(raw) = (unsafe { read_utf8(options_ptr, options_len) }) else {
        return std::ptr::null_mut();
    };
    let Ok(options) = serde_json::from_str::<TimelineOptions>(raw) else {
        return json_string(json!({"ok": false, "error": "invalid timeline options"}));
    };

    match write_timeline(
        &options.timeline_path,
        &TimelineStore {
            version: 1,
            entries: Vec::new(),
        },
    ) {
        Ok(()) => json_string(json!({"ok": true, "entries": []})),
        Err(err) => json_string(json!({"ok": false, "error": err})),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeriodicReviewOptions {
    activity_db_path: String,
    timeline_path: String,
    period: String,
    date: String,
    start_timestamp: f64,
    end_timestamp: f64,
    max_questions: Option<usize>,
    max_intents: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineOptions {
    timeline_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineSyncOptions {
    timeline_path: String,
    entry_id: String,
    conclusion_id: Option<String>,
    synced_at: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TimelineEntry {
    id: String,
    period: String,
    date: String,
    title: String,
    review: String,
    sync_summary: String,
    created_at: f64,
    synced_at: Option<f64>,
    honcho_conclusion_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TimelineStore {
    version: u32,
    entries: Vec<TimelineEntry>,
}

fn build_periodic_review_prompt(options: &PeriodicReviewOptions) -> Result<Value, String> {
    let conn = Connection::open_with_flags(
        &options.activity_db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
        .map_err(|err| format!("open activity sqlite failed: {}", err))?;
    let stats = query_review_stats(&conn, options)?;
    let questions = query_review_questions(&conn, options)?;
    let intents = query_review_intents(&conn, options)?;

    if stats.is_empty() && questions.is_empty() && intents.is_empty() {
        return Ok(json!({
            "ok": true,
            "hasData": false,
            "entryID": entry_id(options),
            "prompt": "",
            "activitySummary": {
                "stats": stats,
                "questions": questions,
                "intents": intents
            }
        }));
    }

    let prompt = build_review_prompt_text(options, &stats, &questions, &intents);
    Ok(json!({
        "ok": true,
        "hasData": true,
        "entryID": entry_id(options),
        "date": options.date,
        "period": options.period,
        "prompt": prompt,
        "activitySummary": {
            "stats": stats,
            "questions": questions,
            "intents": intents
        }
    }))
}

fn query_review_stats(conn: &Connection, options: &PeriodicReviewOptions) -> Result<Vec<Value>, String> {
    let mut stats = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT app_name, total_seconds, session_count, keyboard_events, mouse_clicks
             FROM app_usage_stats
             WHERE date = ?
             ORDER BY total_seconds DESC
             LIMIT 8",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![&options.date], |row| {
            Ok(json!({
                "appName": row.get::<_, String>(0).unwrap_or_default(),
                "totalSeconds": row.get::<_, i64>(1).unwrap_or(0),
                "sessionCount": row.get::<_, i64>(2).unwrap_or(0),
                "keyboardEvents": row.get::<_, i64>(3).unwrap_or(0),
                "mouseClicks": row.get::<_, i64>(4).unwrap_or(0)
            }))
        })
        .map_err(|err| err.to_string())?;
    for row in rows {
        if let Ok(value) = row {
            stats.push(value);
        }
    }
    if !stats.is_empty() {
        return Ok(stats);
    }

    let mut fallback = conn
        .prepare(
            "SELECT app_name,
                    SUM(duration_seconds) AS total_seconds,
                    COUNT(*) AS session_count,
                    SUM(keyboard_events) AS keyboard_events,
                    SUM(mouse_clicks) AS mouse_clicks
             FROM activity_sessions
             WHERE start_time >= ? AND start_time < ? AND is_excluded = 0
             GROUP BY app_bundle_id, app_name
             ORDER BY total_seconds DESC
             LIMIT 8",
        )
        .map_err(|err| err.to_string())?;
    let rows = fallback
        .query_map(params![options.start_timestamp, options.end_timestamp], |row| {
            Ok(json!({
                "appName": row.get::<_, String>(0).unwrap_or_default(),
                "totalSeconds": row.get::<_, i64>(1).unwrap_or(0),
                "sessionCount": row.get::<_, i64>(2).unwrap_or(0),
                "keyboardEvents": row.get::<_, i64>(3).unwrap_or(0),
                "mouseClicks": row.get::<_, i64>(4).unwrap_or(0)
            }))
        })
        .map_err(|err| err.to_string())?;
    for row in rows {
        if let Ok(value) = row {
            stats.push(value);
        }
    }
    Ok(stats)
}

fn query_review_questions(conn: &Connection, options: &PeriodicReviewOptions) -> Result<Vec<Value>, String> {
    let limit = options.max_questions.unwrap_or(20).min(50) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT mode, content, timestamp, has_images, has_documents
             FROM user_questions
             WHERE timestamp >= ? AND timestamp < ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![options.start_timestamp, options.end_timestamp, limit], |row| {
            let content: String = row.get(1).unwrap_or_default();
            Ok(json!({
                "mode": row.get::<_, String>(0).unwrap_or_default(),
                "content": truncate_chars(&content.replace('\n', " "), 180),
                "timestamp": row.get::<_, f64>(2).unwrap_or(0.0),
                "hasImages": row.get::<_, i64>(3).unwrap_or(0) != 0,
                "hasDocuments": row.get::<_, i64>(4).unwrap_or(0) != 0
            }))
        })
        .map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(value) = row {
            out.push(value);
        }
    }
    Ok(out)
}

fn query_review_intents(conn: &Connection, options: &PeriodicReviewOptions) -> Result<Vec<Value>, String> {
    let limit = options.max_intents.unwrap_or(20).min(60) as i64;
    let mut stmt = conn
        .prepare(
            "SELECT trigger_type, app_name, window_title, ocr_text, is_blacklisted, timestamp
             FROM user_intents
             WHERE timestamp >= ? AND timestamp < ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![options.start_timestamp, options.end_timestamp, limit], |row| {
            let ocr: Option<String> = row.get(3).ok();
            Ok(json!({
                "trigger": row.get::<_, String>(0).unwrap_or_default(),
                "appName": row.get::<_, Option<String>>(1).ok().flatten().unwrap_or_default(),
                "windowTitle": row.get::<_, Option<String>>(2).ok().flatten().unwrap_or_default(),
                "text": ocr.map(|s| truncate_chars(&s.replace('\n', " "), 160)).unwrap_or_default(),
                "isBlacklisted": row.get::<_, i64>(4).unwrap_or(0) != 0,
                "timestamp": row.get::<_, f64>(5).unwrap_or(0.0)
            }))
        })
        .map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(value) = row {
            out.push(value);
        }
    }
    Ok(out)
}

fn build_review_prompt_text(
    options: &PeriodicReviewOptions,
    stats: &[Value],
    questions: &[Value],
    intents: &[Value],
) -> String {
    let mut lines = Vec::new();
    lines.push("# 任务".to_string());
    lines.push("你是 Jason hermes 的周期回顾引擎。基于本地活动数据，生成一条可以保存到「成长时间线」的回顾。".to_string());
    lines.push("目标不是流水账，而是提炼用户长期习惯、关注主题、重复阻塞点和下一步建议。".to_string());
    lines.push(String::new());
    lines.push("## 输出要求".to_string());
    lines.push("- 使用简体中文，第二人称「你」。".to_string());
    lines.push("- Markdown 格式，长度 500-900 字。".to_string());
    lines.push("- 不要暴露完整窗口标题、OCR 原文或隐私流水，只提炼主题。".to_string());
    lines.push("- 必须包含这些小节：`## 这段时间的主线`、`## 观察到的模式`、`## 可以继续推进的事`、`## 可同步到 Hermes 的精选摘要`。".to_string());
    lines.push("- `可同步到 Hermes 的精选摘要` 只写 3-5 条长期可复用结论；不要写临时 app 切换、一次性状态、未经验证猜测。".to_string());
    lines.push(String::new());
    lines.push("## 周期".to_string());
    lines.push(format!("- 类型：{}", options.period));
    lines.push(format!("- 日期：{}", options.date));
    lines.push(String::new());

    if !stats.is_empty() {
        lines.push("## App 使用概览".to_string());
        for item in stats.iter().take(8) {
            let app = item.get("appName").and_then(Value::as_str).unwrap_or("");
            let seconds = item.get("totalSeconds").and_then(Value::as_i64).unwrap_or(0);
            let sessions = item.get("sessionCount").and_then(Value::as_i64).unwrap_or(0);
            let keys = item.get("keyboardEvents").and_then(Value::as_i64).unwrap_or(0);
            lines.push(format!(
                "- {}：{}，{} 次会话，{} 次按键",
                app,
                format_seconds(seconds),
                sessions,
                keys
            ));
        }
        lines.push(String::new());
    }

    if !questions.is_empty() {
        lines.push("## 用户对 AI 提过的问题".to_string());
        for item in questions.iter().take(20) {
            let mode = item.get("mode").and_then(Value::as_str).unwrap_or("");
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("- [{}] {}", mode, content));
        }
        lines.push(String::new());
    }

    if !intents.is_empty() {
        lines.push("## 本地意图采样摘要".to_string());
        for item in intents.iter().take(20) {
            if item.get("isBlacklisted").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let app = item.get("appName").and_then(Value::as_str).unwrap_or("");
            let title = item.get("windowTitle").and_then(Value::as_str).unwrap_or("");
            let text = item.get("text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() && title.is_empty() {
                lines.push(format!("- {}", app));
            } else {
                lines.push(format!("- {} / {} / {}", app, title, text));
            }
        }
        lines.push(String::new());
    }

    lines.push("---".to_string());
    lines.push("现在直接输出回顾正文，不要解释数据来源，不要说你无法访问更多信息。".to_string());
    lines.join("\n")
}

fn save_timeline_entry(
    options: &PeriodicReviewOptions,
    review_text: &str,
) -> Result<(TimelineEntry, Vec<TimelineEntry>), String> {
    let mut store = load_timeline(&options.timeline_path)?;
    let now = current_unix_timestamp();
    let entry = TimelineEntry {
        id: entry_id(options),
        period: options.period.clone(),
        date: options.date.clone(),
        title: format!("{} 周期回顾", options.date),
        review: review_text.trim().to_string(),
        sync_summary: build_sync_summary(&options.date, review_text),
        created_at: now,
        synced_at: None,
        honcho_conclusion_id: None,
    };

    store.entries.retain(|item| item.id != entry.id);
    store.entries.insert(0, entry.clone());
    store
        .entries
        .sort_by(|a, b| b.created_at.partial_cmp(&a.created_at).unwrap_or(std::cmp::Ordering::Equal));
    write_timeline(&options.timeline_path, &store)?;
    Ok((entry, store.entries))
}

fn mark_timeline_entry_synced(options: &TimelineSyncOptions) -> Result<Vec<TimelineEntry>, String> {
    let mut store = load_timeline(&options.timeline_path)?;
    let mut found = false;
    for entry in &mut store.entries {
        if entry.id == options.entry_id {
            entry.synced_at = Some(options.synced_at);
            entry.honcho_conclusion_id = options.conclusion_id.clone();
            found = true;
            break;
        }
    }
    if !found {
        return Err("timeline entry not found".to_string());
    }
    write_timeline(&options.timeline_path, &store)?;
    Ok(store.entries)
}

fn load_timeline(path: &str) -> Result<TimelineStore, String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Ok(TimelineStore {
            version: 1,
            entries: Vec::new(),
        });
    }
    let raw = fs::read_to_string(&path).map_err(|err| format!("read timeline failed: {}", err))?;
    if raw.trim().is_empty() {
        return Ok(TimelineStore {
            version: 1,
            entries: Vec::new(),
        });
    }
    serde_json::from_str::<TimelineStore>(&raw).map_err(|err| format!("parse timeline failed: {}", err))
}

fn write_timeline(path: &str, store: &TimelineStore) -> Result<(), String> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create timeline dir failed: {}", err))?;
    }
    let raw = serde_json::to_vec_pretty(store).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|err| format!("write timeline temp failed: {}", err))?;
    fs::rename(&tmp, &path).map_err(|err| format!("replace timeline failed: {}", err))?;
    Ok(())
}

fn build_sync_summary(date: &str, review_text: &str) -> String {
    let section = extract_sync_section(review_text);
    let body = if section.trim().is_empty() {
        truncate_chars(review_text.trim(), 900)
    } else {
        section
    };
    format!(
        "source=jason-hermes-periodic-review date={}\n{}",
        date,
        body.trim()
    )
}

fn extract_sync_section(review_text: &str) -> String {
    let mut capture = false;
    let mut lines = Vec::new();
    for line in review_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') && (trimmed.contains("精选摘要") || trimmed.contains("同步到 Hermes")) {
            capture = true;
            continue;
        }
        if capture && trimmed.starts_with('#') {
            break;
        }
        if capture {
            lines.push(line.to_string());
        }
    }
    lines.join("\n").trim().to_string()
}

fn entry_id(options: &PeriodicReviewOptions) -> String {
    format!("{}-{}", sanitize_file_stem(&options.period), sanitize_file_stem(&options.date))
}

fn current_unix_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn format_seconds(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    if h > 0 {
        format!("{}h{}m", h, m)
    } else {
        format!("{}m", m)
    }
}

unsafe fn read_utf8<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    let bytes = slice::from_raw_parts(ptr, len);
    std::str::from_utf8(bytes).ok()
}

fn parse_part_updated(props: &Value) -> *mut c_char {
    let Some(part) = props.get("part") else {
        return json_string(json!({"kind":"ignore"}));
    };
    if let Some(tool_event) = parse_tool_part(part) {
        return json_string(tool_event);
    }

    let role = props
        .get("role")
        .and_then(Value::as_str)
        .or_else(|| {
            props
                .get("message")
                .and_then(|m| m.get("role"))
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    if role == "assistant" || role == "model" {
        if let Some(text) = extract_assistant_text(part) {
            if !text.trim().is_empty() {
                return json_string(json!({"kind":"part_updated_text","text":text}));
            }
        }
    }

    json_string(json!({"kind":"ignore"}))
}

fn parse_tool_part(part: &Value) -> Option<Value> {
    let part_type = part.get("type").and_then(Value::as_str)?;
    if part_type != "tool" {
        return None;
    }
    let tool = part.get("tool").and_then(Value::as_str)?;
    let state = part.get("state")?;
    let status = state.get("status").and_then(Value::as_str).unwrap_or("");
    let input = state.get("input").unwrap_or(&Value::Null);
    let file_path = first_string(input, &["filePath", "path", "file"]);
    let command = input.get("command").and_then(Value::as_str);
    let url = input.get("url").and_then(Value::as_str);
    let arg = if let Some(path) = file_path {
        last_path_component(path).to_string()
    } else if let Some(cmd) = command {
        truncate_chars(cmd, 40)
    } else if let Some(url) = url {
        url.to_string()
    } else {
        String::new()
    };
    let name = map_tool_name(tool);

    match status {
        "running" | "started" => Some(json!({
            "kind":"tool_started",
            "name":name,
            "arg":arg,
            "file_path":file_path.unwrap_or("")
        })),
        "completed" => Some(json!({
            "kind":"tool_ended",
            "name":name,
            "file_path":file_path.unwrap_or("")
        })),
        _ => Some(json!({"kind":"ignore"})),
    }
}

fn extract_assistant_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        return Some(text.to_string());
                    }
                }
            }
            if let Some(role) = map.get("role").and_then(Value::as_str) {
                if role != "assistant" && role != "model" {
                    return None;
                }
            }
            for key in ["part", "message", "data"] {
                if let Some(nested) = map.get(key) {
                    if let Some(text) = extract_assistant_text(nested) {
                        return Some(text);
                    }
                }
            }
            if let Some(parts) = map.get("parts").and_then(Value::as_array) {
                let texts: Vec<String> = parts.iter().filter_map(extract_assistant_text).collect();
                if !texts.is_empty() {
                    return Some(texts.join(""));
                }
            }
            None
        }
        Value::Array(items) => {
            let texts: Vec<String> = items.iter().filter_map(extract_assistant_text).collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        }
        _ => None,
    }
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn last_path_component(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

fn truncate_chars(value: &str, max: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn readable_text(raw: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let cleaned = line
            .replace(['#', '*', '`', '_', '>'], "")
            .replace("[", "")
            .replace("]", "")
            .replace("(", " ")
            .replace(")", " ");
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(cleaned);
    }
    if out.trim().is_empty() {
        raw.trim().to_string()
    } else {
        out.trim().to_string()
    }
}

fn sanitize_file_stem(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        }
        if out.len() >= 64 {
            break;
        }
    }
    if out.is_empty() {
        "tts".to_string()
    } else {
        out
    }
}

fn parse_listening_port(line: &str) -> Option<u16> {
    let marker = "http://127.0.0.1:";
    let start = line.find(marker)? + marker.len();
    let digits: String = line[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse::<u16>().ok()
}

fn map_tool_name(tool: &str) -> String {
    match tool.to_ascii_lowercase().as_str() {
        "read" | "filesystem_read" | "list_files" => "Read".to_string(),
        "write" | "filesystem_write" => "Write".to_string(),
        "edit" | "filesystem_edit" | "multiedit" => "Edit".to_string(),
        "bash" | "shell" | "command_execution" => "Bash".to_string(),
        "search" | "grep" | "ripgrep" | "glob" => "Search".to_string(),
        "webfetch" | "web_fetch" | "fetch_url" => "WebFetch".to_string(),
        "task" | "subagent" => "Task".to_string(),
        "todo" | "todowrite" => "Todo".to_string(),
        _ => {
            let mut chars = tool.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

fn json_string(value: Value) -> *mut c_char {
    match CString::new(value.to_string()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn parse_opencode(raw: &str, session: &str) -> Value {
        let ptr = hermes_parse_opencode_sse_event(
            raw.as_ptr(),
            raw.len(),
            session.as_ptr(),
            session.len(),
        );
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        hermes_rust_free_string(ptr);
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn parses_matching_text_delta() {
        let event = r#"{"type":"message.part.delta","properties":{"sessionID":"s1","field":"text","delta":"hello"}}"#;
        let parsed = parse_opencode(event, "s1");
        assert_eq!(parsed["kind"], "text_delta");
        assert_eq!(parsed["text"], "hello");
    }

    #[test]
    fn ignores_other_session() {
        let event = r#"{"type":"message.part.delta","properties":{"sessionID":"s2","field":"text","delta":"hello"}}"#;
        let parsed = parse_opencode(event, "s1");
        assert_eq!(parsed["kind"], "ignore");
    }

    #[test]
    fn indexes_visible_message_ids() {
        let raw = r#"[
            {"id":"a","content":"one","isStreaming":false},
            {"id":"b","content":"two","isStreaming":true},
            {"id":"c","content":"three","isStreaming":false}
        ]"#;
        let ptr = hermes_index_messages_json(raw.as_ptr(), raw.len(), 2);
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        hermes_rust_free_string(ptr);
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["total"], 3);
        assert_eq!(parsed["hidden"], 1);
        assert_eq!(parsed["visibleIDs"][0], "b");
        assert_eq!(parsed["visibleIDs"][1], "c");
        assert_eq!(parsed["streamingIDs"][0], "b");
    }

    #[test]
    fn builds_mimo_request() {
        let raw = r#"{"text":"你好","baseUrl":"https://token-plan-sgp.xiaomimimo.com/v1","model":"mimo-v2.5-tts","voice":"冰糖","stylePrompt":"台湾腔"}"#;
        let ptr = hermes_build_mimo_tts_request_json(raw.as_ptr(), raw.len());
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        hermes_rust_free_string(ptr);
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["body"]["audio"]["voice"], "冰糖");
        assert_eq!(parsed["body"]["messages"][1]["content"], "你好");
    }

    #[test]
    fn validates_yaml() {
        let kind = "yaml";
        let raw = "a: 1\nb:\n  - c\n";
        let ptr = hermes_validate_text(kind.as_ptr(), kind.len(), raw.as_ptr(), raw.len());
        assert!(!ptr.is_null());
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        hermes_rust_free_string(ptr);
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["ok"], true);
    }

    #[test]
    fn parses_listening_port() {
        assert_eq!(
            parse_listening_port("opencode server listening on http://127.0.0.1:14098"),
            Some(14098)
        );
    }

    #[test]
    fn extracts_growth_timeline_sync_section() {
        let review = r#"
## 这段时间的主线
你主要在修 HermesPet。

## 可同步到 Hermes 的精选摘要
- 用户希望 Jason hermes 的周期回顾默认只写本地时间线。
- 只有用户手动点击同步时，才把精选摘要写入 Hermes/Honcho。

## 其他
不要同步这里。
"#;
        let summary = extract_sync_section(review);
        assert!(summary.contains("默认只写本地时间线"));
        assert!(!summary.contains("不要同步这里"));
    }

    #[test]
    fn saves_growth_timeline_entry() {
        let path = std::env::temp_dir().join(format!(
            "hermes-growth-timeline-test-{}.json",
            current_unix_timestamp()
        ));
        let options = PeriodicReviewOptions {
            activity_db_path: "/tmp/not-used.sqlite".to_string(),
            timeline_path: path.to_string_lossy().to_string(),
            period: "daily".to_string(),
            date: "2026-05-23".to_string(),
            start_timestamp: 0.0,
            end_timestamp: 1.0,
            max_questions: None,
            max_intents: None,
        };
        let (entry, entries) = save_timeline_entry(
            &options,
            "## 可同步到 Hermes 的精选摘要\n- 保留分层同步。",
        )
        .unwrap();
        assert_eq!(entry.id, "daily-2026-05-23");
        assert_eq!(entries.len(), 1);
        assert!(entry.sync_summary.contains("source=jason-hermes-periodic-review"));
        let _ = std::fs::remove_file(path);
    }
}
