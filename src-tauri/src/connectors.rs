use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use crate::runtime_config::read_runtime_env;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorStatus {
    Disconnected,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorInfo {
    pub provider: String,
    pub name: String,
    pub status: ConnectorStatus,
    pub scopes: Vec<String>,
    pub auth_url: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: String,
    pub input_schema: Value,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectConnectorResult {
    pub provider: String,
    pub status: ConnectorStatus,
    pub auth_url: Option<String>,
    pub message: String,
}

#[derive(Default)]
struct ConnectorStore {
    tokens: HashMap<String, String>,
}

#[derive(Clone, Default)]
pub struct ConnectorRegistry {
    store: Arc<Mutex<ConnectorStore>>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list_connectors(&self) -> Vec<ConnectorInfo> {
        vec![
            ConnectorInfo {
                provider: "google".to_string(),
                name: "Google Workspace".to_string(),
                status: self.provider_status("google"),
                scopes: vec![
                    "Gmail read/write/send".to_string(),
                    "Calendar read/write".to_string(),
                ],
                auth_url: self.oauth_url("google"),
                message: self.provider_message("google"),
            },
            ConnectorInfo {
                provider: "github".to_string(),
                name: "GitHub".to_string(),
                status: self.provider_status("github"),
                scopes: vec![
                    "Read repositories, issues, and pull requests".to_string(),
                    "Create issues and comments".to_string(),
                ],
                auth_url: self.oauth_url("github"),
                message: self.provider_message("github"),
            },
        ]
    }

    pub fn list_tools(&self) -> Vec<ConnectorTool> {
        connector_tools()
    }

    pub fn connect(&self, provider: &str) -> ConnectConnectorResult {
        let normalized = normalize_provider(provider);
        let status = self.provider_status(&normalized);
        let auth_url = self.oauth_url(&normalized);
        let message = match status {
            ConnectorStatus::Connected => format!("{} is connected.", provider_label(&normalized)),
            ConnectorStatus::Disconnected => {
                let oauth_note = if auth_url.is_some() {
                    " OAuth client settings were found, but the in-app OAuth callback is not implemented yet."
                } else {
                    ""
                };
                format!(
                    "Set {} in src-tauri/.env, then restart the app to connect {}.{}",
                    token_env_name(&normalized),
                    provider_label(&normalized),
                    oauth_note,
                )
            }
            ConnectorStatus::Error => format!("{} connector is misconfigured.", provider_label(&normalized)),
        };

        ConnectConnectorResult {
            provider: normalized,
            status,
            auth_url,
            message,
        }
    }

    pub fn disconnect(&self, provider: &str) -> Result<(), String> {
        let normalized = normalize_provider(provider);
        let mut store = self.store.lock().map_err(|e| e.to_string())?;
        store.tokens.remove(&normalized);
        Ok(())
    }

    pub async fn dispatch_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        if name.starts_with("github_") {
            return self.dispatch_github(name, args).await;
        }
        if name.starts_with("google_") {
            return self.dispatch_google(name, args).await;
        }
        Err(format!("Unknown connector tool: {}", name))
    }

    fn provider_status(&self, provider: &str) -> ConnectorStatus {
        if self.token(provider).is_some() {
            ConnectorStatus::Connected
        } else {
            ConnectorStatus::Disconnected
        }
    }

    fn provider_message(&self, provider: &str) -> Option<String> {
        if self.token(provider).is_some() {
            Some("Ready for connector actions.".to_string())
        } else {
            Some(format!(
                "Not connected. Set {} in src-tauri/.env, then restart the app.",
                token_env_name(provider)
            ))
        }
    }

    fn token(&self, provider: &str) -> Option<String> {
        if let Ok(store) = self.store.lock() {
            if let Some(token) = store.tokens.get(provider) {
                if !token.trim().is_empty() {
                    return Some(token.clone());
                }
            }
        }

        read_runtime_env(token_env_name(provider))
    }

    fn oauth_url(&self, provider: &str) -> Option<String> {
        match provider {
            "google" => {
                let client_id = read_runtime_env("GOOGLE_OAUTH_CLIENT_ID")?;
                let redirect_uri = read_runtime_env("GOOGLE_OAUTH_REDIRECT_URI")
                    .unwrap_or_else(|| "http://127.0.0.1:58231/oauth/google/callback".to_string());
                let scope = [
                    "https://www.googleapis.com/auth/gmail.readonly",
                    "https://www.googleapis.com/auth/gmail.compose",
                    "https://www.googleapis.com/auth/gmail.send",
                    "https://www.googleapis.com/auth/calendar.events",
                ].join(" ");
                Some(format!(
                    "https://accounts.google.com/o/oauth2/v2/auth?response_type=code&access_type=offline&prompt=consent&client_id={}&redirect_uri={}&scope={}",
                    url_encode(&client_id),
                    url_encode(&redirect_uri),
                    url_encode(&scope)
                ))
            }
            "github" => {
                let client_id = read_runtime_env("GITHUB_OAUTH_CLIENT_ID")?;
                let redirect_uri = read_runtime_env("GITHUB_OAUTH_REDIRECT_URI")
                    .unwrap_or_else(|| "http://127.0.0.1:58231/oauth/github/callback".to_string());
                Some(format!(
                    "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&allow_signup=true",
                    url_encode(&client_id),
                    url_encode(&redirect_uri),
                    url_encode("repo read:user")
                ))
            }
            _ => None,
        }
    }

    async fn dispatch_github(&self, name: &str, args: &Value) -> Result<String, String> {
        let token = self
            .token("github")
            .ok_or_else(|| "GitHub connector is not connected. Set GITHUB_TOKEN or connect GitHub in Connectors.".to_string())?;
        let client = reqwest::Client::new();

        match name {
            "github_get_repo" => {
                let owner = required_str(args, "owner")?;
                let repo = required_str(args, "repo")?;
                let url = format!("https://api.github.com/repos/{}/{}", owner, repo);
                github_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "github_search_issues" => {
                let query = required_str(args, "query")?;
                let url = format!(
                    "https://api.github.com/search/issues?q={}&per_page={}",
                    url_encode(query),
                    args["limit"].as_u64().unwrap_or(10).min(30)
                );
                github_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "github_list_pull_requests" => {
                let owner = required_str(args, "owner")?;
                let repo = required_str(args, "repo")?;
                let state = args["state"].as_str().unwrap_or("open");
                let url = format!(
                    "https://api.github.com/repos/{}/{}/pulls?state={}&per_page={}",
                    owner,
                    repo,
                    url_encode(state),
                    args["limit"].as_u64().unwrap_or(10).min(30)
                );
                github_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "github_create_issue" => {
                let owner = required_str(args, "owner")?;
                let repo = required_str(args, "repo")?;
                let title = required_str(args, "title")?;
                let body = args["body"].as_str().unwrap_or("");
                let url = format!("https://api.github.com/repos/{}/{}/issues", owner, repo);
                github_request(&client, reqwest::Method::POST, &url, &token, Some(json!({
                    "title": title,
                    "body": body
                }))).await
            }
            "github_comment_issue" => {
                let owner = required_str(args, "owner")?;
                let repo = required_str(args, "repo")?;
                let issue_number = args["issue_number"].as_u64().ok_or_else(|| "issue_number is required".to_string())?;
                let body = required_str(args, "body")?;
                let url = format!(
                    "https://api.github.com/repos/{}/{}/issues/{}/comments",
                    owner, repo, issue_number
                );
                github_request(&client, reqwest::Method::POST, &url, &token, Some(json!({ "body": body }))).await
            }
            _ => Err(format!("Unknown GitHub connector tool: {}", name)),
        }
    }

    async fn dispatch_google(&self, name: &str, args: &Value) -> Result<String, String> {
        let token = self
            .token("google")
            .ok_or_else(|| "Google connector is not connected. Set GOOGLE_ACCESS_TOKEN or connect Google in Connectors.".to_string())?;
        let client = reqwest::Client::new();

        match name {
            "google_gmail_search" => {
                let query = required_str(args, "query")?;
                let max = args["limit"].as_u64().unwrap_or(10).min(25);
                let url = format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages?q={}&maxResults={}",
                    url_encode(query),
                    max
                );
                google_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "google_gmail_read" => {
                let id = required_str(args, "message_id")?;
                let url = format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata",
                    url_encode(id)
                );
                google_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "google_gmail_create_draft" => {
                let raw = build_mime_message(args)?;
                let url = "https://gmail.googleapis.com/gmail/v1/users/me/drafts";
                google_request(&client, reqwest::Method::POST, url, &token, Some(json!({
                    "message": { "raw": raw }
                }))).await
            }
            "google_gmail_send" => {
                let raw = build_mime_message(args)?;
                let url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
                google_request(&client, reqwest::Method::POST, url, &token, Some(json!({ "raw": raw }))).await
            }
            "google_calendar_list_events" => {
                let calendar_id = args["calendar_id"].as_str().unwrap_or("primary");
                let max = args["limit"].as_u64().unwrap_or(10).min(25);
                let mut url = format!(
                    "https://www.googleapis.com/calendar/v3/calendars/{}/events?singleEvents=true&orderBy=startTime&maxResults={}",
                    url_encode(calendar_id),
                    max
                );
                if let Some(time_min) = args["time_min"].as_str() {
                    url.push_str("&timeMin=");
                    url.push_str(&url_encode(time_min));
                }
                google_request(&client, reqwest::Method::GET, &url, &token, None).await
            }
            "google_calendar_create_event" => {
                let calendar_id = args["calendar_id"].as_str().unwrap_or("primary");
                let summary = required_str(args, "summary")?;
                let start = required_str(args, "start")?;
                let end = required_str(args, "end")?;
                let url = format!(
                    "https://www.googleapis.com/calendar/v3/calendars/{}/events",
                    url_encode(calendar_id)
                );
                let attendees = args["attendees"].as_array().map(|items| {
                    items.iter()
                        .filter_map(|item| item.as_str())
                        .map(|email| json!({ "email": email }))
                        .collect::<Vec<_>>()
                }).unwrap_or_default();
                google_request(&client, reqwest::Method::POST, &url, &token, Some(json!({
                    "summary": summary,
                    "description": args["description"].as_str().unwrap_or(""),
                    "start": { "dateTime": start },
                    "end": { "dateTime": end },
                    "attendees": attendees
                }))).await
            }
            _ => Err(format!("Unknown Google connector tool: {}", name)),
        }
    }
}

pub fn is_connector_tool(name: &str) -> bool {
    name.starts_with("google_") || name.starts_with("github_")
}

pub fn connector_tool_is_write(name: &str) -> bool {
    matches!(
        name,
        "google_gmail_create_draft"
            | "google_gmail_send"
            | "google_calendar_create_event"
            | "github_create_issue"
            | "github_comment_issue"
    )
}

pub fn connector_tools_as_function_declarations() -> Vec<Value> {
    connector_tools()
        .into_iter()
        .map(|tool| json!({
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema
        }))
        .collect()
}

fn connector_tools() -> Vec<ConnectorTool> {
    vec![
        tool("google", "google_gmail_search", "Search Gmail messages. Use this for email lookup before using browser automation.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "query": { "type": "STRING", "description": "Gmail search query, e.g. from:alex newer_than:7d" },
                "limit": { "type": "INTEGER", "description": "Maximum messages to return, default 10" }
            },
            "required": ["query"]
        })),
        tool("google", "google_gmail_read", "Read Gmail message metadata by id.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "message_id": { "type": "STRING", "description": "Gmail message id from google_gmail_search" }
            },
            "required": ["message_id"]
        })),
        tool("google", "google_gmail_create_draft", "Create a Gmail draft. Requires user approval.", "dangerous", json!({
            "type": "OBJECT",
            "properties": {
                "to": { "type": "STRING" },
                "subject": { "type": "STRING" },
                "body": { "type": "STRING" },
                "cc": { "type": "STRING" }
            },
            "required": ["to", "subject", "body"]
        })),
        tool("google", "google_gmail_send", "Send an email through Gmail. Requires user approval.", "dangerous", json!({
            "type": "OBJECT",
            "properties": {
                "to": { "type": "STRING" },
                "subject": { "type": "STRING" },
                "body": { "type": "STRING" },
                "cc": { "type": "STRING" }
            },
            "required": ["to", "subject", "body"]
        })),
        tool("google", "google_calendar_list_events", "List Google Calendar events.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "calendar_id": { "type": "STRING", "description": "Calendar id, default primary" },
                "time_min": { "type": "STRING", "description": "RFC3339 lower bound" },
                "limit": { "type": "INTEGER" }
            }
        })),
        tool("google", "google_calendar_create_event", "Create a Google Calendar event. Requires user approval.", "dangerous", json!({
            "type": "OBJECT",
            "properties": {
                "calendar_id": { "type": "STRING", "description": "Calendar id, default primary" },
                "summary": { "type": "STRING" },
                "description": { "type": "STRING" },
                "start": { "type": "STRING", "description": "RFC3339 start date-time" },
                "end": { "type": "STRING", "description": "RFC3339 end date-time" },
                "attendees": { "type": "ARRAY", "items": { "type": "STRING" } }
            },
            "required": ["summary", "start", "end"]
        })),
        tool("github", "github_get_repo", "Get GitHub repository details.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "owner": { "type": "STRING" },
                "repo": { "type": "STRING" }
            },
            "required": ["owner", "repo"]
        })),
        tool("github", "github_search_issues", "Search GitHub issues and pull requests.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "query": { "type": "STRING", "description": "GitHub search query" },
                "limit": { "type": "INTEGER" }
            },
            "required": ["query"]
        })),
        tool("github", "github_list_pull_requests", "List GitHub pull requests for a repository.", "safe", json!({
            "type": "OBJECT",
            "properties": {
                "owner": { "type": "STRING" },
                "repo": { "type": "STRING" },
                "state": { "type": "STRING", "description": "open, closed, or all" },
                "limit": { "type": "INTEGER" }
            },
            "required": ["owner", "repo"]
        })),
        tool("github", "github_create_issue", "Create a GitHub issue. Requires user approval.", "dangerous", json!({
            "type": "OBJECT",
            "properties": {
                "owner": { "type": "STRING" },
                "repo": { "type": "STRING" },
                "title": { "type": "STRING" },
                "body": { "type": "STRING" }
            },
            "required": ["owner", "repo", "title"]
        })),
        tool("github", "github_comment_issue", "Comment on a GitHub issue or pull request. Requires user approval.", "dangerous", json!({
            "type": "OBJECT",
            "properties": {
                "owner": { "type": "STRING" },
                "repo": { "type": "STRING" },
                "issue_number": { "type": "INTEGER" },
                "body": { "type": "STRING" }
            },
            "required": ["owner", "repo", "issue_number", "body"]
        })),
    ]
}

fn tool(provider: &str, name: &str, description: &str, risk: &str, input_schema: Value) -> ConnectorTool {
    ConnectorTool {
        id: format!("{}/{}", provider, name),
        name: name.to_string(),
        description: description.to_string(),
        provider: provider.to_string(),
        input_schema,
        risk: risk.to_string(),
    }
}

async fn github_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    token: &str,
    body: Option<Value>,
) -> Result<String, String> {
    let mut request = client
        .request(method, url)
        .header(USER_AGENT, "Ritual")
        .header(ACCEPT, "application/vnd.github+json")
        .header(AUTHORIZATION, format!("Bearer {}", token));
    if let Some(body) = body {
        request = request.json(&body);
    }
    response_text(request.send().await).await
}

async fn google_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    token: &str,
    body: Option<Value>,
) -> Result<String, String> {
    let mut request = client
        .request(method, url)
        .bearer_auth(token)
        .header(ACCEPT, "application/json");
    if let Some(body) = body {
        request = request.json(&body);
    }
    response_text(request.send().await).await
}

async fn response_text(response: Result<reqwest::Response, reqwest::Error>) -> Result<String, String> {
    let response = response.map_err(|e| format!("Connector request failed: {}", e))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Connector request failed ({}): {}", status, truncate(&body, 2000)));
    }
    Ok(truncate(&body, 4000))
}

fn build_mime_message(args: &Value) -> Result<String, String> {
    let to = required_str(args, "to")?;
    let subject = required_str(args, "subject")?;
    let body = required_str(args, "body")?;
    let cc = args["cc"].as_str().unwrap_or("");
    let mut mime = format!("To: {}\r\nSubject: {}\r\n", to, subject);
    if !cc.trim().is_empty() {
        mime.push_str(&format!("Cc: {}\r\n", cc));
    }
    mime.push_str("Content-Type: text/plain; charset=\"UTF-8\"\r\n\r\n");
    mime.push_str(body);
    Ok(URL_SAFE_NO_PAD.encode(mime.as_bytes()))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{} is required", key))
}

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "google" => "Google Workspace",
        "github" => "GitHub",
        _ => "Connector",
    }
}

fn token_env_name(provider: &str) -> &'static str {
    match provider {
        "google" => "GOOGLE_ACCESS_TOKEN",
        "github" => "GITHUB_TOKEN",
        _ => "CONNECTOR_TOKEN",
    }
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_lowercase()
}

fn truncate(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        input.chars().take(max_chars).collect::<String>() + "\n...truncated"
    }
}

fn url_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![b as char]
            }
            b' ' => vec!['%','2','0'],
            _ => {
                let hex = format!("%{:02X}", b);
                hex.chars().collect()
            }
        })
        .collect()
}
