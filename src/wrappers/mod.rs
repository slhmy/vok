/// Shell type enum for platform-specific behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ShellType {
    Unix,       // sh/bash/zsh on Linux/macOS
    PowerShell, // Windows PowerShell (5.x) or pwsh (7.x)
    CmdExe,     // Windows cmd.exe
}

/// Detect the current shell type
pub fn detect_shell_type() -> ShellType {
    #[cfg(not(windows))]
    return ShellType::Unix;

    #[cfg(windows)]
    {
        // PowerShell sets PSModulePath with Windows-specific paths
        if let Ok(path) = std::env::var("PSModulePath") {
            // PowerShell module path contains WindowsPowerShell or PowerShell
            if path.contains("WindowsPowerShell") || path.contains("PowerShell\\") {
                return ShellType::PowerShell;
            }
        }
        // cmd.exe sets COMSPEC
        if std::env::var("COMSPEC").is_ok() {
            return ShellType::CmdExe;
        }
        ShellType::CmdExe // default on Windows
    }
}

/// Detect if running on Windows shell environment (legacy compatibility)
#[allow(dead_code)]
pub fn is_windows_shell() -> bool {
    detect_shell_type() != ShellType::Unix
}

/// Unix wrapper configs: (name, help_arg)
pub const UNIX_WRAPPERS: &[(&str, &str)] = &[
    ("curl", "--help"),
    ("git", "--help"),
    ("docker", "--help"),
    ("tar", "--help"),
    ("find", "--help"),
    ("ffmpeg", "-help"),
];

/// PowerShell wrapper configs: (name, help_arg)
/// PowerShell uses -? or -Help for cmdlets, --help for external tools
pub const POWERSHELL_WRAPPERS: &[(&str, &str)] = &[
    // PowerShell cmdlets
    ("Get-ChildItem", "-?"),
    ("Get-Content", "-?"),
    ("Set-Content", "-?"),
    ("Select-String", "-?"),
    ("Remove-Item", "-?"),
    ("New-Item", "-?"),
    ("Copy-Item", "-?"),
    ("Move-Item", "-?"),
    // External tools that work in PowerShell
    ("git", "--help"),
    ("docker", "--help"),
    ("curl", "--help"),
    ("ffmpeg", "-help"),
];

/// cmd.exe wrapper configs: (name, help_arg)
pub const CMDEXE_WRAPPERS: &[(&str, &str)] = &[
    ("curl", "--help"),
    ("git", "--help"),
    ("docker", "--help"),
    // Note: Windows 'find' is text search (like grep), NOT Unix find
    ("ffmpeg", "-help"),
];

/// Get the appropriate wrapper configs for current shell
pub fn get_wrapper_configs() -> &'static [(&'static str, &'static str)] {
    match detect_shell_type() {
        ShellType::Unix => UNIX_WRAPPERS,
        ShellType::PowerShell => POWERSHELL_WRAPPERS,
        ShellType::CmdExe => CMDEXE_WRAPPERS,
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
        #[cfg(not(windows))]
        {
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
}
