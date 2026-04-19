mod brain;
mod config;
mod executor;
mod parser;
mod wrappers;

use clap::{Parser, Subcommand};
use colored::Colorize;
use config::V0kConfig;
use executor::PreparedCommand;
use std::env;
use std::io::{self, Write};
use wrappers::{detect_shell_type, ShellType};

#[derive(Parser)]
#[command(name = "v0k", version, about = "Semantic-level intelligent CLI agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Natural language command — let AI figure out what to run
    ///
    /// Examples:
    ///   v0k ask "用curl下载 example.com 的首页"
    ///   v0k ask "生成一个自签名SSL证书"
    Ask {
        /// Natural language query
        query: Vec<String>,
    },
    /// Interactive setup for v0k configuration
    Setup,
}

#[tokio::main]
async fn main() {
    let raw_args = env::args().skip(1).collect::<Vec<_>>();
    let config = V0kConfig::load();

    let result = if should_use_clap_dispatch(&raw_args) {
        let cli = Cli::parse();

        match cli.command {
            Commands::Ask { query } => handle_ask(&config, query).await,
            Commands::Setup => handle_setup().await,
        }
    } else if let Some((program, args)) = raw_args.split_first() {
        if let Some(res) = handle_any_wrapper(&config, program, args.to_vec()).await {
            res
        } else {
            handle_external_command(&config, raw_args).await
        }
    } else {
        Ok(())
    };

    if let Err(e) = result {
        eprintln!("v0k: {e}");
        std::process::exit(1);
    }
}

/// Dispatcher for dynamic/known wrapper subcommands
async fn handle_any_wrapper(
    config: &V0kConfig,
    program: &str,
    args: Vec<String>,
) -> Option<Result<(), String>> {
    // Check if command exists first
    if which::which(program).is_err() {
        return None;
    }

    wrappers::get_help_arg(program)?;
    Some(handle_wrapper(config, program, args).await)
}

/// Generic handler for any smart wrapper supporting AI fallback semantics:
/// Forwards parsing of the command parameters to the AI completely.
async fn handle_wrapper(config: &V0kConfig, name: &str, args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        return Err(format!("no arguments provided for {}.", name));
    }

    if !config.has_ai() {
        return Err("could not parse arguments and no AI configured.\n\
             Hint: set V0K_API_KEY or add api_key to ~/.v0k/config.toml"
            .to_string());
    }

    let shell_type = detect_shell_type();
    let user_input = format!("{} {}", name, args.join(" "));
    let extension = wrappers::ai_prompt_extension(name);
    let brain_resp =
        brain::infer_with_extension_for_shell(config, &user_input, extension, shell_type).await?;
    execute_brain_response(config, brain_resp).await
}

/// Handle `v0k ask ...` — pure natural language, always uses AI.
/// Only prints the suggested command; never executes it.
async fn handle_ask(config: &V0kConfig, query: Vec<String>) -> Result<(), String> {
    if query.is_empty() {
        return Err("no query provided. Usage: v0k ask \"your question\"".into());
    }

    if !config.has_ai() {
        return Err(
            "AI not configured. Set V0K_API_KEY or add api_key to ~/.v0k/config.toml".into(),
        );
    }

    let user_input = query.join(" ");
    let resp = brain::infer(config, &user_input).await?;
    let cmd = prepared_command(resp.program.clone(), resp.args.clone());

    eprintln!("{}", resp.explanation.green());
    println!("{}", format!("$ {}", cmd.display).blue());

    Ok(())
}

async fn handle_setup() -> Result<(), String> {
    let current_cfg = config::V0kConfig::load_file().unwrap_or_default();

    println!("Welcome to v0k setup! Press Enter to keep the current value.");

    let api_base = prompt(
        "API Base URL",
        current_cfg
            .api_base
            .as_deref()
            .unwrap_or("https://api.openai.com/v1"),
    )
    .map_err(|e| e.to_string())?;

    let key_hint = if current_cfg
        .api_key
        .as_ref()
        .is_some_and(|k| !k.trim().is_empty())
    {
        "sk-...**"
    } else {
        ""
    };
    let raw_key = prompt_password("API Key", key_hint).map_err(|e| e.to_string())?;

    let api_key = if raw_key == "sk-...**" || raw_key.trim().is_empty() {
        current_cfg.api_key
    } else {
        Some(raw_key)
    };

    let model = prompt(
        "Model",
        current_cfg.model.as_deref().unwrap_or("gpt-4o-mini"),
    )
    .map_err(|e| e.to_string())?;

    let new_cfg = config::FileConfig {
        api_base: Some(api_base).filter(|s| !s.is_empty()),
        api_key,
        model: Some(model).filter(|s| !s.is_empty()),
    };

    config::V0kConfig::save_file(&new_cfg).map_err(|e| format!("Failed to save config: {e}"))?;
    println!("Configuration saved successfully to ~/.v0k/config.toml");
    Ok(())
}

fn prompt(label: &str, default: &str) -> std::io::Result<String> {
    use std::io::Write;
    print!("{} [{}]: ", label, default);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_password(label: &str, default: &str) -> std::io::Result<String> {
    let p = if default.is_empty() {
        format!("{label}: ")
    } else {
        format!("{label} [{default}]: ")
    };
    let input = rpassword::prompt_password(p)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

/// Handle unknown top-level commands by passing them through directly.
/// If AI is configured, review the command once before execution.
async fn handle_external_command(config: &V0kConfig, raw_args: Vec<String>) -> Result<(), String> {
    let (program, args) = raw_args
        .split_first()
        .ok_or("no command provided")
        .map(|(program, args)| (program.clone(), args.to_vec()))?;

    if wrappers::is_known_wrapper(&program) {
        return Err(format!(
            "wrapper command `{program}` should be handled by the built-in dispatcher"
        ));
    }

    let shell_type = detect_shell_type();
    let original = prepared_command(program.clone(), args.clone());

    if !config.has_ai() {
        return execute_with_healing(config, original, shell_type).await;
    }

    let program_exists = which::which(&program).is_ok();

    let review = match brain::review_command_for_shell(
        config,
        &original.display,
        program_exists,
        shell_type,
    )
    .await
    {
        Ok(review) => review,
        Err(err) => {
            eprintln!("warning: AI review failed: {err}");
            return execute_with_healing(config, original, shell_type).await;
        }
    };

    // If AI identified a wrapper command, re-infer with wrapper help for better accuracy
    if wrappers::is_known_wrapper(&review.program) {
        let wrapper_hint = wrappers::ai_prompt_extension(&review.program);
        let user_intent = format!("{} {}", review.program, review.args.join(" "));
        let refined =
            brain::infer_with_extension_for_shell(config, &user_intent, wrapper_hint, shell_type)
                .await?;
        return execute_brain_response(config, refined).await;
    }

    let rewritten = prepared_command(review.program.clone(), review.args.clone());
    let changed = command_changed(&original, &rewritten);

    if changed {
        eprintln!("{}", review.explanation.green());
        eprintln!("{}", format!("Original: {}", original.display).blue());
        eprintln!("{}", format!("Suggested: {}", rewritten.display).blue());

        if review.confidence < 0.85 {
            let msg = format!(
                "Confidence: {:.0}% — AI is not fully sure about this rewrite.",
                review.confidence * 100.0
            );
            eprintln!("{}", msg.yellow());
        }
        if is_dangerous(&rewritten.program, &rewritten.args) {
            eprintln!("{}", "The suggested command may be destructive!".red());
        }

        let use_rewrite = confirm(
            &"Use AI-reviewed command instead? [y/N] "
                .yellow()
                .to_string(),
        )?;
        if use_rewrite {
            return execute_with_healing(config, rewritten, shell_type).await;
        }

        if which::which(&original.program).is_err() {
            eprintln!(
                "{}",
                "Aborting because original input is not a recognized command.".yellow()
            );
            return Err("aborted by user".into());
        }

        if is_dangerous(&original.program, &original.args) {
            eprintln!(
                "{}",
                "Falling back to the original command, which may be destructive.".red()
            );
            if !confirm(
                &"Proceed with the original command? [y/N] "
                    .red()
                    .to_string(),
            )? {
                return Err("aborted by user".into());
            }
        }

        return execute_with_healing(config, original, shell_type).await;
    }

    let needs_confirm = review.confidence < 0.85 || is_dangerous(&original.program, &original.args);
    if needs_confirm {
        eprintln!("{}", review.explanation.green());
        eprintln!("{}", format!("reviewed: {}", original.display).blue());

        if review.confidence < 0.85 {
            let msg = format!(
                "Confidence: {:.0}% — AI is not fully sure this command is correct.",
                review.confidence * 100.0
            );
            eprintln!("{}", msg.yellow());
        }
        if is_dangerous(&original.program, &original.args) {
            eprintln!("{}", "This may be destructive!".red());
        }

        if !confirm(&"Proceed? [y/N] ".yellow().to_string())? {
            return Err("aborted by user".into());
        }
    }

    execute_with_healing(config, original, shell_type).await
}

/// Execute a command with self-healing on failure.
const MAX_HEAL_ATTEMPTS: u32 = 3;

async fn execute_with_healing(
    config: &V0kConfig,
    cmd: PreparedCommand,
    shell_type: ShellType,
) -> Result<(), String> {
    let mut current_cmd = cmd;
    let mut attempts = 0;

    loop {
        // First try: normal execution (preserve interactive support)
        let first_try = executor::execute(current_cmd.clone()).await;

        if first_try.is_ok() {
            return Ok(());
        }

        // Command failed - check if we should try healing
        attempts += 1;
        if attempts >= MAX_HEAL_ATTEMPTS {
            return first_try;
        }

        if !config.has_ai() {
            return first_try;
        }

        // Re-run with capture to get error details
        eprintln!("{}", "Command failed, analyzing...".yellow());
        let captured = executor::execute_captured(current_cmd.clone()).await;

        // Get wrapper help if this is a known wrapper
        let wrapper_hint = wrappers::ai_prompt_extension(&current_cmd.program);

        // Analyze failure
        let heal = match brain::analyze_failure_for_shell(
            config,
            &current_cmd.display,
            &captured.stdout,
            &captured.stderr,
            captured.exit_code,
            wrapper_hint.as_deref(),
            shell_type,
        )
        .await
        {
            Ok(h) => h,
            Err(e) => {
                eprintln!("warning: AI healing failed: {e}");
                return Err(format!(
                    "`{}` failed with code {}",
                    current_cmd.display, captured.exit_code
                ));
            }
        };

        if !heal.recoverable {
            return Err(format!(
                "`{}` failed: {}",
                current_cmd.display, heal.explanation
            ));
        }

        // Show suggestion and prompt
        eprintln!("{}", heal.explanation.green());
        eprintln!("{}", format!("Failed: {}", current_cmd.display).red());
        let fixed_cmd = prepared_command(heal.program.clone(), heal.args.clone());
        eprintln!("{}", format!("Suggested fix: {}", fixed_cmd.display).blue());

        if heal.confidence < 0.7 {
            eprintln!(
                "{}",
                format!("Confidence: {:.0}%", heal.confidence * 100.0).yellow()
            );
        }

        if !confirm("Try the suggested fix? [y/N] ")? {
            return Err("user declined fix".into());
        }

        current_cmd = fixed_cmd;
    }
}

/// Execute a BrainResponse with confirmation when confidence is low or command is dangerous.
async fn execute_brain_response(
    config: &V0kConfig,
    resp: brain::BrainResponse,
) -> Result<(), String> {
    let shell_type = detect_shell_type();
    let cmd = prepared_command(resp.program.clone(), resp.args.clone());

    let needs_confirm = resp.confidence < 0.85 || is_dangerous(&resp.program, &resp.args);

    if needs_confirm {
        eprintln!("{}", resp.explanation.green());
        eprintln!("{}", format!("v0k wants to run: {}", cmd.display).blue());

        if resp.confidence < 0.85 {
            let msg = format!(
                "Confidence: {:.0}% — AI is not fully sure about this.",
                resp.confidence * 100.0
            );
            eprintln!("{}", msg.yellow());
        }
        if is_dangerous(&resp.program, &resp.args) {
            eprintln!("{}", "This may be destructive!".red());
        }

        if !confirm(&"Proceed? [y/N] ".yellow().to_string())? {
            return Err("aborted by user".into());
        }
    } else {
        eprintln!("{}", format!("$ {}", cmd.display).blue());
    }

    execute_with_healing(config, cmd, shell_type).await
}

/// Check if a command looks dangerous.
fn is_dangerous(program: &str, args: &[String]) -> bool {
    let joined = args.join(" ").to_lowercase();
    let dangerous_patterns = [
        "--force", "-rf", "rm -r", "drop ", "delete ", "--hard", "format ", "mkfs",
    ];
    let dangerous_programs = ["rm", "shutdown", "reboot"];

    dangerous_programs.contains(&program) || dangerous_patterns.iter().any(|p| joined.contains(p))
}

/// Join args for display, quoting those with spaces.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| display_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_arg(arg: &str) -> String {
    if arg.is_empty()
        || arg
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\'))
    {
        format!("{arg:?}")
    } else {
        arg.to_string()
    }
}

fn prepared_command(program: String, args: Vec<String>) -> PreparedCommand {
    let display = if args.is_empty() {
        program.clone()
    } else {
        format!("{} {}", program, shell_join(&args))
    };

    PreparedCommand {
        program,
        args,
        display,
    }
}

fn should_use_clap_dispatch(raw_args: &[String]) -> bool {
    raw_args.is_empty()
        || raw_args[0].starts_with('-')
        || raw_args[0] == "help"
        || is_builtin_command(&raw_args[0])
}

fn is_builtin_command(command: &str) -> bool {
    matches!(command, "ts" | "b64" | "ask" | "setup")
}

fn command_changed(original: &PreparedCommand, reviewed: &PreparedCommand) -> bool {
    original.program != reviewed.program || original.args != reviewed.args
}

fn confirm(prompt: &str) -> Result<bool, String> {
    eprint!("{prompt}");
    io::stderr().flush().ok();

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("failed to read input: {e}"))?;

    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_use_clap_dispatch_for_builtins() {
        assert!(should_use_clap_dispatch(&["ask".into()]));
        assert!(should_use_clap_dispatch(&["--help".into()]));
    }

    #[test]
    fn test_should_bypass_clap_for_unknown_commands() {
        assert!(!should_use_clap_dispatch(&["echo".into(), "hello".into()]));
    }

    #[test]
    fn test_prepared_command_without_args() {
        let cmd = prepared_command("git".into(), Vec::new());
        assert_eq!(cmd.display, "git");
    }

    #[test]
    fn test_shell_join_escapes_control_characters() {
        assert_eq!(shell_join(&["hello\n".into()]), "\"hello\\n\"");
    }
}
