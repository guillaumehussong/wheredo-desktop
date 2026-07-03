//! Vision call: screenshot + question → spoken answer + grounded pointer actions.
//! Port of macOS `Vision.swift` — same system prompt (accuracy-first rules),
//! same tolerant JSON parsing, same 401-refresh-retry behavior.

use serde_json::{json, Value};

use super::{config, oauth};

/// A grounded UI action. Coordinates are NORMALIZED (0–1000) on the screenshot.
#[derive(Debug, Clone)]
pub struct UIAction {
    pub point: Option<Point>,
    pub click: bool,
    pub label: String,
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug)]
pub struct VisionResult {
    pub spoken_answer: String,
    pub actions: Vec<UIAction>,
}

#[derive(Debug)]
pub enum VisionError {
    RequestFailed(u16, String),
    Network(String),
    Auth(oauth::OAuthError),
}

impl std::fmt::Display for VisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VisionError::RequestFailed(code, body) => write!(f, "Vision HTTP {code}: {body}"),
            VisionError::Network(e) => write!(f, "Network error: {e}"),
            VisionError::Auth(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VisionError {}

impl From<reqwest::Error> for VisionError {
    fn from(e: reqwest::Error) -> Self {
        VisionError::Network(e.to_string())
    }
}

impl From<oauth::OAuthError> for VisionError {
    fn from(e: oauth::OAuthError) -> Self {
        VisionError::Auth(e)
    }
}

/// Heuristic: does the question ask WHERE something is / HOW to do something?
pub fn needs_pointing(question: &str) -> bool {
    let q = question.to_lowercase();
    [
        "how ", "where", "comment", "ouvrir", "ouvre", "cliqu", "click",
        "nouvelle", "new ", "trouver", "find", "faire", "go to", "open ",
    ]
    .iter()
    .any(|k| q.contains(k))
}

fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "macOS"
    }
}

fn system_prompt(app_context: &str, strict_pointing: bool) -> String {
    let pointing_rules = if strict_pointing {
        "The user is asking WHERE something is: if the relevant control is visible on screen, return its coordinates.\n"
    } else {
        ""
    };

    format!(
        r#"You are a vocal {os} desktop tutor helping the user with the app currently on screen.
You see a screenshot of the active window. {ctx}
Reply in the same language as the user's question. Be concise and natural for text-to-speech.
{pointing}
A red guide cursor will appear at each point you return — use it to show WHERE to click.

ACCURACY FIRST:
- Use your real knowledge of the app to answer correctly (menus, settings, features).
- Only point at a control if it is ACTUALLY VISIBLE in the screenshot and truly relevant.
- If the feature lives elsewhere (a menu, a settings page), say the exact path in "speak"
  (e.g. "Open Settings, then the MCP tab") and point at the first visible step (e.g. the gear icon) if one exists.
- Never point at an unrelated control just to have an action. An empty "actions" array is better than a wrong pointer.
- If you are not sure, say what you would check — do not guess.

Reply with STRICT JSON only:
{{
  "speak": "short phrase to say aloud",
  "actions": [
    {{ "point": {{ "x": 850, "y": 50, "screen": 0 }}, "label": "visible text or icon", "click": false }}
  ]
}}
Rules:
- x and y are integers 0 (left/top) to 1000 (right/bottom) on the captured screenshot.
- Put the point on the CENTER of the clickable control.
- label: exact visible text OR icon symbol ("+", "×", "…") — never invent names.
- click: false unless the user explicitly asked you to click for them.
- No markdown, no text outside the JSON object."#,
        os = os_name(),
        ctx = app_context,
        pointing = pointing_rules,
    )
}

pub async fn analyze(
    image_base64: &str,
    question: &str,
    app_context: &str,
) -> Result<VisionResult, VisionError> {
    let strict = needs_pointing(question);
    let body = json!({
        "model": config::vision_model(),
        "messages": [
            { "role": "system", "content": system_prompt(app_context, strict) },
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": question },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/jpeg;base64,{image_base64}"),
                            "detail": config::vision_image_detail()
                        }
                    }
                ]
            }
        ],
        "response_format": { "type": "json_object" },
        "temperature": config::vision_temperature(),
        "max_tokens": config::vision_max_tokens()
    });

    let url = format!("{}/chat/completions", config::api_base());
    let resp = authorized_post(&url, &body).await?;
    if resp.status != 200 {
        return Err(VisionError::RequestFailed(resp.status, resp.text()));
    }

    let chat: Value =
        serde_json::from_slice(&resp.body).map_err(|e| VisionError::Network(e.to_string()))?;
    let raw = chat["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}");

    let result = parse_vision_json(raw);
    if result.actions.is_empty() && strict {
        super::feedback::log("⚠️  No pointer coordinates returned — guide overlay cannot appear.");
    } else if !result.actions.is_empty() {
        super::feedback::log(&format!("   {} pointer action(s) parsed.", result.actions.len()));
    }
    Ok(result)
}

/// POST with bearer auth; on 401 the token is refreshed once and the request replayed.
async fn authorized_post(
    url: &str,
    body: &Value,
) -> Result<super::http::HttpResponse, VisionError> {
    let token = oauth::access_token().await?;
    let resp = super::http::post_json(url, body, Some(&token)).await?;
    if resp.status != 401 {
        return Ok(resp);
    }
    let Some(creds) = oauth::load() else { return Ok(resp) };
    let refreshed = oauth::refresh(&creds).await?;
    oauth::save(&refreshed);
    Ok(super::http::post_json(url, body, Some(&refreshed.access_token)).await?)
}

/// Deliberately forgiving parse: on malformed JSON the raw text becomes the
/// spoken answer (the user still hears something useful).
pub fn parse_vision_json(raw: &str) -> VisionResult {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return VisionResult { spoken_answer: raw.to_string(), actions: vec![] };
    };

    let speak = parsed["speak"].as_str().unwrap_or(raw).to_string();
    let actions = parse_actions(&parsed["actions"]);
    VisionResult { spoken_answer: speak, actions }
}

/// Tolerant action decoding: numbers may arrive as int, float or string.
fn parse_actions(value: &Value) -> Vec<UIAction> {
    let Some(arr) = value.as_array() else { return vec![] };
    arr.iter()
        .map(|item| {
            let point = item.get("point").and_then(|pt| {
                let x = flex_number(pt.get("x"))?;
                let y = flex_number(pt.get("y"))?;
                Some(Point { x, y })
            });
            UIAction {
                point,
                click: item["click"].as_bool().unwrap_or(false),
                label: item["label"].as_str().unwrap_or("").to_string(),
            }
        })
        .collect()
}

fn flex_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}
