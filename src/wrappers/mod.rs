/// Detect if running on Windows shell environment
pub fn is_windows_shell() -> bool {
    // PowerShell sets PSModulePath
    if std::env::var("PSModulePath").is_ok() {
        return true;
    }
    // cmd.exe sets COMSPEC
    if std::env::var("COMSPEC").is_ok() {
        return true;
    }
    false
}

/// Unix wrapper configs: (name, help_arg)
#[allow(clippy::redundant_static_lifetimes)]
pub const UNIX_WRAPPERS: &[(&'static str, &'static str)] = &[
    ("curl", "--help"),
    ("git", "--help"),
    ("docker", "--help"),
    ("tar", "--help"),
    ("find", "--help"),
    ("ffmpeg", "-help"),
];

/// Windows wrapper configs: (name, help_arg)
#[allow(clippy::redundant_static_lifetimes)]
pub const WINDOWS_WRAPPERS: &[(&'static str, &'static str)] = &[
    ("curl", "--help"),
    ("git", "--help"),
    ("docker", "--help"),
    ("tar", "--help"),
    ("find", "/?"),
    ("ffmpeg", "-help"),
];

/// Get the appropriate wrapper configs for current shell
pub fn get_wrapper_configs() -> &'static [(&'static str, &'static str)] {
    if is_windows_shell() {
        WINDOWS_WRAPPERS
    } else {
        UNIX_WRAPPERS
    }
}

/// Get help_arg for a command
pub fn get_help_arg(name: &str) -> Option<&'static str> {
    let configs = get_wrapper_configs();
    configs
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, help)| *help)
}

/// Check if command has a known wrapper on current platform
pub fn is_known_wrapper(cmd: &str) -> bool {
    get_wrapper_configs().iter().any(|(name, _)| *name == cmd)
}

/// Get AI prompt extension for a wrapper
pub fn ai_prompt_extension(name: &str) -> Option<String> {
    let help_arg = get_help_arg(name)?;
    let help_text = std::process::Command::new(name)
        .arg(help_arg)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    if help_text.trim().is_empty() {
        None
    } else {
        Some(format!(
            "This request targets the `{}` command.\n\nHere is its help output for reference:\n{}",
            name, help_text
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_known_wrapper() {
        assert!(is_known_wrapper("curl"));
        assert!(is_known_wrapper("git"));
        assert!(is_known_wrapper("ffmpeg"));
        assert!(!is_known_wrapper("unknown"));
    }

    #[test]
    fn test_get_help_arg() {
        // On Unix (this test environment), should return Unix help args
        if !is_windows_shell() {
            assert_eq!(get_help_arg("curl"), Some("--help"));
            assert_eq!(get_help_arg("ffmpeg"), Some("-help"));
            assert_eq!(get_help_arg("find"), Some("--help"));
        }
        assert_eq!(get_help_arg("unknown"), None);
    }

    #[test]
    fn test_ai_prompt_extension_format() {
        if let Some(prompt) = ai_prompt_extension("curl") {
            assert!(prompt.contains("targets the `curl` command"));
        }
    }

    #[test]
    fn test_windows_shell_detection() {
        // On this Unix test environment, should return false
        #[cfg(not(windows))]
        assert!(!is_windows_shell());
    }
}
