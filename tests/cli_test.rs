use crossbeam_channel as channel;
use serial_test::serial;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn v0k_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_v0k"))
}

fn isolated_v0k_bin(test_name: &str) -> Command {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "v0k-test-{test_name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&home).expect("failed to create isolated home");

    let mut cmd = v0k_bin();
    cmd.env("HOME", home)
        .env("V0K_TEST_MODE", "1")
        .env_remove("V0K_API_KEY")
        .env_remove("V0K_API_BASE")
        .env_remove("V0K_MODEL")
        .stdin(Stdio::null());
    cmd
}

fn start_mock_ai_server(response_body: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind mock server");
    // Set listener to non-blocking to check if ready
    listener
        .set_nonblocking(true)
        .expect("failed to set nonblocking");

    let addr = listener
        .local_addr()
        .expect("failed to read mock server addr");
    let body = response_body.to_string();

    // Channel to signal when server is ready to accept
    let (ready_tx, ready_rx) = channel::bounded(1);

    let handle = thread::spawn(move || {
        // Set back to blocking for actual accept
        listener
            .set_nonblocking(false)
            .expect("failed to set blocking");

        // Signal ready before accepting
        ready_tx.send(()).expect("failed to signal ready");

        let (mut stream, _) = listener.accept().expect("failed to accept connection");
        let mut buffer = [0_u8; 4096];
        let _ = stream.read(&mut buffer);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        stream
            .write_all(response.as_bytes())
            .expect("failed to write mock response");
    });

    // Wait for server to be ready before returning
    ready_rx.recv().expect("server failed to start");

    (format!("http://{addr}/v1"), handle)
}

#[test]
#[serial]
fn test_ask_no_api_key_error() {
    // Without V0K_API_KEY, ask should fail gracefully
    let output = isolated_v0k_bin("ask-no-api-key")
        .args(["ask", "hello"])
        .output()
        .expect("failed to run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AI not configured"));
}

#[test]
#[serial]
fn test_fix_without_command_shows_integration_hint() {
    let output = isolated_v0k_bin("fix-no-command")
        .args(["fix"])
        .output()
        .expect("failed to run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing failed command"));
    assert!(stderr.contains("command v0k fix --command \"$last_cmd\" --exit-code \"$exit_code\""));
}

#[test]
#[serial]
fn test_fix_dry_run_json_outputs_suggestion_without_executing() {
    let response = r#"{"choices":[{"message":{"content":"{\"program\":\"git\",\"args\":[\"status\"],\"explanation\":\"The executable name was mistyped as gti; use git status instead.\",\"confidence\":0.96,\"recoverable\":true}"}}]}"#;
    let (api_base, server) = start_mock_ai_server(response);

    let output = isolated_v0k_bin("fix-dry-run-json")
        .env("V0K_API_KEY", "test-key")
        .env("V0K_API_BASE", api_base)
        .env("V0K_MODEL", "test-model")
        .args([
            "fix",
            "--command",
            "gti status",
            "--exit-code",
            "127",
            "--dry-run",
            "--json",
        ])
        .output()
        .expect("failed to run");

    server.join().expect("mock server thread failed");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(json["original_command"], "gti status");
    assert_eq!(json["exit_code"], 127);
    assert_eq!(json["recoverable"], true);
    assert_eq!(json["suggested_command"], "git status");
    assert_eq!(json["will_execute"], false);
}

#[test]
#[serial]
fn test_no_args_shows_help() {
    let output = v0k_bin().output().expect("failed to run");
    // clap exits with error when no subcommand is provided
    assert!(!output.status.success());
}

#[test]
#[serial]
fn test_unknown_command_passes_through_without_ai() {
    let output = isolated_v0k_bin("unknown-pass-through")
        .args(["echo", "hello"])
        .output()
        .expect("failed to run");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello");
}

#[test]
#[serial]
fn test_unknown_command_ai_review_without_rewrite() {
    let response = r#"{"choices":[{"message":{"content":"{\"program\":\"echo\",\"args\":[\"hello\"],\"explanation\":\"The command already looks correct.\",\"confidence\":0.98}"}}]}"#;
    let (api_base, server) = start_mock_ai_server(response);

    let output = isolated_v0k_bin("unknown-ai-review")
        .env("V0K_API_KEY", "test-key")
        .env("V0K_API_BASE", api_base)
        .env("V0K_MODEL", "test-model")
        .args(["echo", "hello"])
        .output()
        .expect("failed to run");

    server.join().expect("mock server thread failed");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello");
}

#[test]
#[serial]
fn test_unknown_command_ai_rewrite_requires_confirmation() {
    let response = r#"{"choices":[{"message":{"content":"{\"program\":\"printf\",\"args\":[\"hello\\n\"],\"explanation\":\"Use printf for a predictable literal newline.\",\"confidence\":0.94}"}}]}"#;
    let (api_base, server) = start_mock_ai_server(response);

    let mut child = isolated_v0k_bin("unknown-ai-rewrite")
        .env("V0K_API_KEY", "test-key")
        .env("V0K_API_BASE", api_base)
        .env("V0K_MODEL", "test-model")
        .args(["echo", "hello"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn");

    child
        .stdin
        .as_mut()
        .expect("missing stdin")
        .write_all(b"y\n")
        .expect("failed to send confirmation");

    let output = child.wait_with_output().expect("failed to wait on child");
    server.join().expect("mock server thread failed");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stdout.trim(), "hello");
    assert!(stderr.contains("Original: echo hello"));
    assert!(stderr.contains("Suggested: printf \"hello\\n\""));
    assert!(stderr.contains("Use AI-reviewed command instead?"));
}
