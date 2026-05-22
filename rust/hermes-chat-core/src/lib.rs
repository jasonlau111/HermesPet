use base64::Engine;
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
        if style_prompt.trim().is_empty() {
            if voice_design_desc.trim().is_empty() {
                "默认音色".to_string()
            } else {
                voice_design_desc.to_string()
            }
        } else {
            format!("{}\n风格指令：{}", voice_design_desc, style_prompt)
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
}
