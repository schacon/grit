//! Minimal Cursor Cloud Agents HTTP client (`https://api.cursor.com`).

use std::time::Duration;

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.cursor.com";

fn api_key() -> Result<String> {
    std::env::var("CURSOR_API_KEY")
        .or_else(|_| std::env::var("GIT_CLOUD_API_KEY"))
        .context("set CURSOR_API_KEY (or GIT_CLOUD_API_KEY) for Cursor Cloud API access")
}

fn auth_header() -> Result<String> {
    let key = api_key()?;
    let token = format!("{key}:");
    let b64 = B64.encode(token.as_bytes());
    Ok(format!("Basic {b64}"))
}

fn client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("build HTTP client")
}

#[derive(Debug, Serialize)]
struct PromptText {
    text: String,
}

#[derive(Debug, Serialize)]
struct Source {
    repository: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#ref: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateAgentBody {
    prompt: PromptText,
    source: Source,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub source: AgentSource,
    #[serde(default)]
    pub target: AgentTarget,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSource {
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub r#ref: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTarget {
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub url: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub pr_url: Option<String>,
}

/// `POST /v0/agents` — start a cloud agent on `repository` at `ref` (default `main`).
pub fn launch_agent(repository: &str, prompt: &str, git_ref: &str) -> Result<AgentInfo> {
    let body = CreateAgentBody {
        prompt: PromptText {
            text: prompt.to_string(),
        },
        source: Source {
            repository: repository.to_string(),
            r#ref: Some(git_ref.to_string()),
        },
    };
    let c = client()?;
    let res = c
        .post(format!("{API_BASE}/v0/agents"))
        .header("Authorization", auth_header()?)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .context("POST /v0/agents")?;
    if !res.status().is_success() {
        let txt = res.text().unwrap_or_default();
        anyhow::bail!("POST /v0/agents failed: {txt}");
    }
    let info: AgentInfo = res.json().context("parse create agent JSON")?;
    Ok(info)
}

/// `GET /v0/agents/{id}` — poll agent status.
pub fn get_agent(id: &str) -> Result<AgentInfo> {
    let c = client()?;
    let res = c
        .get(format!("{API_BASE}/v0/agents/{id}"))
        .header("Authorization", auth_header()?)
        .send()
        .context("GET agent")?;
    if !res.status().is_success() {
        let txt = res.text().unwrap_or_default();
        anyhow::bail!("GET /v0/agents/{id} failed: {txt}");
    }
    res.json().context("parse agent JSON")
}

/// Returns true when the agent reached a terminal success state (`FINISHED`).
pub fn is_finished_success(status: &str) -> bool {
    status.eq_ignore_ascii_case("FINISHED")
}

/// Returns true when the agent is done (success or failure).
pub fn is_terminal(status: &str) -> bool {
    matches!(
        status.to_ascii_uppercase().as_str(),
        "FINISHED" | "FAILED" | "CANCELLED" | "ERROR" | "EXPIRED"
    )
}

/// Probe API key with `GET /v0/me`.
pub fn verify_auth() -> Result<()> {
    let c = client()?;
    let res = c
        .get(format!("{API_BASE}/v0/me"))
        .header("Authorization", auth_header()?)
        .send()
        .context("GET /v0/me")?;
    if !res.status().is_success() {
        let txt = res.text().unwrap_or_default();
        anyhow::bail!("CURSOR_API_KEY rejected: {txt}");
    }
    let v: serde_json::Value = res.json().context("parse /v0/me")?;
    let email = v
        .get("userEmail")
        .and_then(|x| x.as_str())
        .unwrap_or("(unknown)");
    eprintln!(
        "{}Authenticated as {}{}",
        crate::ansi::GREEN,
        email,
        crate::ansi::RESET
    );
    Ok(())
}
