pub mod prompt;

use crate::config::V0kConfig;
use crate::wrappers::ShellType;
use prompt::{heal_prompt_for_shell, review_prompt_for_shell, system_prompt_for_shell};
use serde::{Deserialize, Serialize};

/// Structured response from the AI brain.
#[derive(Debug, Clone, Deserialize)]
pub struct BrainResponse {
    pub program: String,
    pub args: Vec<String>,
    pub explanation: String,
    pub confidence: f32,
}

/// Structured response for failure analysis and self-healing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HealResponse {
    pub program: String,
    pub args: Vec<String>,
    pub explanation: String,
    pub confidence: f32,
    pub recoverable: bool,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

/// Call the AI API to interpret user intent and return a structured command.
#[allow(dead_code)]
pub async fn infer(config: &V0kConfig, user_input: &str) -> Result<BrainResponse, String> {
    infer_with_extension(config, user_input, None).await
}

/// Call the AI API with shell context for command inference.
#[allow(dead_code)]
pub async fn infer_for_shell(
    config: &V0kConfig,
    user_input: &str,
    shell_type: ShellType,
) -> Result<BrainResponse, String> {
    infer_with_extension_for_shell(config, user_input, None, shell_type).await
}

/// Call the AI API with an optional prompt extension for command-specific guidance.
pub async fn infer_with_extension(
    config: &V0kConfig,
    user_input: &str,
    prompt_extension: Option<String>,
) -> Result<BrainResponse, String> {
    infer_with_extension_for_shell(config, user_input, prompt_extension, ShellType::Unix).await
}

/// Call the AI API with extension and shell context for command inference.
pub async fn infer_with_extension_for_shell(
    config: &V0kConfig,
    user_input: &str,
    prompt_extension: Option<String>,
    shell_type: ShellType,
) -> Result<BrainResponse, String> {
    let mut system_prompt = system_prompt_for_shell(shell_type);
    if let Some(extension) = prompt_extension {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&extension);
    }

    run_chat_completion(config, system_prompt, user_input.to_string()).await
}

/// Review an already-formed command and optionally suggest a safer rewrite.
#[allow(dead_code)]
pub async fn review_command(
    config: &V0kConfig,
    command: &str,
    exists_in_path: bool,
) -> Result<BrainResponse, String> {
    review_command_for_shell(config, command, exists_in_path, ShellType::Unix).await
}

/// Review a command with shell context.
pub async fn review_command_for_shell(
    config: &V0kConfig,
    command: &str,
    exists_in_path: bool,
    shell_type: ShellType,
) -> Result<BrainResponse, String> {
    #[derive(Serialize)]
    struct ReviewInput<'a> {
        command: &'a str,
        exists_in_path: bool,
    }

    let review_input = serde_json::to_string(&ReviewInput {
        command,
        exists_in_path,
    })
    .map_err(|e| format!("failed to serialize review input: {e}"))?;

    run_chat_completion(config, review_prompt_for_shell(shell_type), review_input).await
}

/// Analyze a failed command and suggest a fix if recoverable.
#[allow(dead_code)]
pub async fn analyze_failure(
    config: &V0kConfig,
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    wrapper_hint: Option<&str>,
) -> Result<HealResponse, String> {
    analyze_failure_for_shell(
        config,
        command,
        stdout,
        stderr,
        exit_code,
        wrapper_hint,
        ShellType::Unix,
    )
    .await
}

/// Analyze a failed command with shell context.
pub async fn analyze_failure_for_shell(
    config: &V0kConfig,
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    wrapper_hint: Option<&str>,
    shell_type: ShellType,
) -> Result<HealResponse, String> {
    #[derive(Serialize)]
    struct FailureInput<'a> {
        command: &'a str,
        stdout: &'a str,
        stderr: &'a str,
        exit_code: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        wrapper_hint: Option<&'a str>,
    }

    let input = serde_json::to_string(&FailureInput {
        command,
        stdout,
        stderr,
        exit_code,
        wrapper_hint,
    })
    .map_err(|e| format!("failed to serialize failure input: {e}"))?;

    run_heal_completion(config, input, shell_type).await
}

async fn run_heal_completion(
    config: &V0kConfig,
    user_input: String,
    shell_type: ShellType,
) -> Result<HealResponse, String> {
    let api_key = config.api_key.as_ref().ok_or("no API key configured")?;

    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

    let request = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: heal_prompt_for_shell(shell_type),
            },
            Message {
                role: "user".to_string(),
                content: user_input,
            },
        ],
        temperature: 0.1,
    };

    let client = reqwest::Client::builder()
        .user_agent(format!("v0k/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {e}"))?;

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({status}): {body}"));
    }

    let chat_resp: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse API response: {e}"))?;

    let content = chat_resp
        .choices
        .first()
        .ok_or("API returned no choices")?
        .message
        .content
        .clone();

    parse_heal_response(&content)
}

fn parse_heal_response(content: &str) -> Result<HealResponse, String> {
    let json_str = if let Some(start) = content.find('{') {
        let end = content.rfind('}').unwrap_or(content.len() - 1);
        &content[start..=end]
    } else {
        content
    };

    serde_json::from_str::<HealResponse>(json_str)
        .map_err(|e| format!("failed to parse heal output as JSON: {e}\nRaw output: {content}"))
}

async fn run_chat_completion(
    config: &V0kConfig,
    system_prompt: String,
    user_input: String,
) -> Result<BrainResponse, String> {
    let api_key = config
        .api_key
        .as_ref()
        .ok_or("no API key configured. Set V0K_API_KEY or add api_key to ~/.v0k/config.toml")?;

    let url = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));

    let request = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system_prompt,
            },
            Message {
                role: "user".to_string(),
                content: user_input,
            },
        ],
        temperature: 0.1,
    };

    let client = reqwest::Client::builder()
        .user_agent(format!("v0k/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("failed to create HTTP client: {e}"))?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({status}): {body}"));
    }

    let chat_resp: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse API response: {e}"))?;

    let content = chat_resp
        .choices
        .first()
        .ok_or("API returned no choices")?
        .message
        .content
        .clone();

    parse_brain_response(&content)
}

/// Extract JSON from the AI response, handling markdown code fences.
fn parse_brain_response(content: &str) -> Result<BrainResponse, String> {
    // Strip markdown code fences if present
    let json_str = if let Some(start) = content.find('{') {
        let end = content.rfind('}').unwrap_or(content.len() - 1);
        &content[start..=end]
    } else {
        content
    };

    serde_json::from_str::<BrainResponse>(json_str)
        .map_err(|e| format!("failed to parse AI output as JSON: {e}\nRaw output: {content}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_brain_response_clean_json() {
        let input = r#"{"program": "curl", "args": ["-X", "GET", "https://example.com"], "explanation": "Simple GET request", "confidence": 0.95}"#;
        let resp = parse_brain_response(input).unwrap();
        assert_eq!(resp.program, "curl");
        assert_eq!(resp.args.len(), 3);
        assert!(resp.confidence > 0.9);
    }

    #[test]
    fn test_parse_brain_response_with_fences() {
        let input = "Here is the command:\n```json\n{\"program\": \"curl\", \"args\": [\"-X\", \"GET\", \"https://example.com\"], \"explanation\": \"GET\", \"confidence\": 0.9}\n```";
        let resp = parse_brain_response(input).unwrap();
        assert_eq!(resp.program, "curl");
    }
}
