use std::process::Stdio;
use tokio::process::Command;

/// A prepared command ready for execution.
#[derive(Clone)]
pub struct PreparedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub display: String,
}

/// Result of a captured execution for failure analysis.
#[allow(dead_code)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Execute a prepared command, printing the resolved invocation and streaming output.
pub async fn execute(cmd: PreparedCommand) -> Result<(), String> {
    let (shell, arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let mut child = Command::new(shell)
        .arg(arg)
        .arg(&cmd.display)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("failed to spawn `{}`: {}", cmd.display, e))?;

    let status = child
        .wait()
        .await
        .map_err(|e| format!("failed to wait on `{}`: {}", cmd.display, e))?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(format!("`{}` exited with code {}", cmd.program, code));
    }

    Ok(())
}

/// Execute a command with output capture (for failure analysis).
pub async fn execute_captured(cmd: PreparedCommand) -> ExecutionResult {
    let (shell, arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let output = Command::new(shell).arg(arg).arg(&cmd.display).output();

    match output.await {
        Ok(o) => ExecutionResult {
            success: o.status.success(),
            exit_code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).to_string(),
            stderr: String::from_utf8_lossy(&o.stderr).to_string(),
        },
        Err(e) => ExecutionResult {
            success: false,
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("failed to spawn: {}", e),
        },
    }
}
