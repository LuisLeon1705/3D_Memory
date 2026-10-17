//! Google Antigravity: its hooks contract and its transcript.
//!
//! Antigravity has no "prompt" or "file read" hook that can add context. Only `PreInvocation`
//! (before every model call) can inject, with `injectSteps: [{ "ephemeralMessage": … }]`, which
//! lasts until the end of the user's turn. It does not receive the prompt, but it gets
//! `transcriptPath`, where every step is a JSON line. So one hook does everything:
//!
//! - read only the transcript lines added since the last call (`SessionState::offset`);
//! - a new `USER_INPUT` step starts a turn: inject the `CORE` and what the message and the
//!   working tree recall (the injected context of the previous turn is gone by then);
//! - each new `view_file` injects that file's rules once per turn;
//! - each `write_to_file` / `replace_file_content` refreshes that area of the memory.

use super::json::Json;

/// Fields of a hook's stdin that `cthg` uses (Antigravity keys are camelCase).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Payload {
    pub conversation: String,
    pub workspace: String,
    pub transcript: String,
}

pub fn parse_payload(raw: &str) -> Payload {
    let Some(v) = Json::parse(raw.trim()) else { return Payload::default() };
    let s = |k: &str| v.get(k).and_then(Json::str).unwrap_or("").to_string();
    Payload {
        conversation: s("conversationId"),
        workspace: v.get("workspacePaths").and_then(|w| w.arr().first()).and_then(Json::str).unwrap_or("").to_string(),
        transcript: s("transcriptPath"),
    }
}

/// What happened in the conversation, as far as memory is concerned.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The user wrote this (tags like `<USER_REQUEST>` removed).
    User { index: i64, text: String },
    /// The agent read this file (absolute path as the agent gave it).
    Read(String),
    /// The agent wrote or edited this file.
    Edit(String),
}

const READ_TOOLS: &[(&str, &str)] = &[("view_file", "AbsolutePath"), ("view_file_outline", "AbsolutePath"), ("view_code_item", "File")];
const EDIT_TOOLS: &[(&str, &str)] = &[("write_to_file", "TargetFile"), ("replace_file_content", "TargetFile"), ("multi_replace_file_content", "TargetFile")];

/// The user's words inside `<USER_REQUEST>…</USER_REQUEST>`, or the whole text without tags.
pub fn user_request(content: &str) -> String {
    let inner = match content.split_once("<USER_REQUEST>") {
        Some((_, rest)) => rest.split("</USER_REQUEST>").next().unwrap_or(rest),
        None => content,
    };
    inner.trim().to_string()
}

/// Parses complete transcript lines (a half-written last line is left for the next call).
/// Returns the steps and how many bytes were consumed.
pub fn scan(text: &str) -> (Vec<Step>, usize) {
    let consumed = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let mut steps = Vec::new();
    for line in text[..consumed].lines() {
        let Some(v) = Json::parse(line) else { continue };
        if v.get("type").and_then(Json::str) == Some("USER_INPUT") {
            let index = v.get("step_index").and_then(Json::num).unwrap_or(-1.0) as i64;
            let text = user_request(v.get("content").and_then(Json::str).unwrap_or(""));
            steps.push(Step::User { index, text });
        }
        for call in v.get("tool_calls").map(Json::arr).unwrap_or(&[]) {
            let name = call.get("name").and_then(Json::str).unwrap_or("");
            let args = call.get("args");
            let arg = |key: &str| args.and_then(|a| a.get(key)).and_then(Json::str).map(str::to_string);
            if let Some((_, key)) = READ_TOOLS.iter().find(|(n, _)| *n == name) {
                steps.extend(arg(key).map(Step::Read));
            } else if let Some((_, key)) = EDIT_TOOLS.iter().find(|(n, _)| *n == name) {
                steps.extend(arg(key).map(Step::Edit));
            }
        }
    }
    (steps, consumed)
}

/// The hook's stdout: one ephemeral message, or `{}` when there is nothing to add.
pub fn inject(text: &str) -> String {
    if text.trim().is_empty() {
        return "{}".to_string();
    }
    format!("{{\"injectSteps\":[{{\"ephemeralMessage\":\"{}\"}}]}}", super::hook::escape_json(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes copied from real Antigravity transcripts (`brain/<id>/.system_generated/logs/transcript.jsonl`).
    const TRANSCRIPT: &str = concat!(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-16T01:59:18Z","content":"<USER_REQUEST>\narregla el colisionador\n</USER_REQUEST>\n<ADDITIONAL_METADATA>x</ADDITIONAL_METADATA>"}"#,
        "\n",
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","tool_calls":[{"name":"view_file","args":{"AbsolutePath":"C:\\repo\\src\\core\\collider.rs","toolAction":"Viewing"}},{"name":"run_command","args":{"CommandLine":"cargo test"}}]}"#,
        "\n",
        r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","tool_calls":[{"name":"replace_file_content","args":{"TargetFile":"C:\\repo\\src\\core\\collider.rs","ReplacementContent":"x"}}]}"#,
        "\n",
        r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RES"#,
    );

    #[test]
    fn reads_payload_and_new_transcript_lines() {
        let p = parse_payload(r#"{"conversationId":"ec33","workspacePaths":["C:\\repo"],"transcriptPath":"C:\\t.jsonl","modelName":"auto","invocationNum":3}"#);
        assert_eq!((p.conversation.as_str(), p.workspace.as_str(), p.transcript.as_str()), ("ec33", r"C:\repo", r"C:\t.jsonl"));
        assert_eq!(parse_payload("no es json"), Payload::default());

        let (steps, consumed) = scan(TRANSCRIPT);
        assert_eq!(
            steps,
            vec![
                Step::User { index: 0, text: "arregla el colisionador".into() },
                Step::Read(r"C:\repo\src\core\collider.rs".into()),
                Step::Edit(r"C:\repo\src\core\collider.rs".into()),
            ]
        );
        assert!(TRANSCRIPT[consumed..].starts_with(r#"{"step_index":3"#), "the half-written line waits for the next call");
        let (again, _) = scan(&TRANSCRIPT[consumed..]);
        assert!(again.is_empty());
    }

    #[test]
    fn output_is_an_ephemeral_message_or_nothing() {
        assert_eq!(inject("  "), "{}");
        let out = inject("## Reglas\n- \"no\" uses unwrap");
        let v = Json::parse(&out).unwrap();
        let msg = v.get("injectSteps").unwrap().arr()[0].get("ephemeralMessage").unwrap().str().unwrap();
        assert_eq!(msg, "## Reglas\n- \"no\" uses unwrap");
    }
}
