/// Wrapper configurations: (command name, help argument)
pub const WRAPPER_CONFIGS: &[(&str, &str)] = &[
    ("curl", "--help"),
    ("git", "--help"),
    ("docker", "--help"),
    ("tar", "--help"),
    ("find", "--help"),
    ("ffmpeg", "-help"),
];

/// Find wrapper config by command name, returns help_arg
pub fn find_wrapper(cmd: &str) -> Option<&'static str> {
    WRAPPER_CONFIGS
        .iter()
        .find(|(name, _)| *name == cmd)
        .map(|(_, help_arg)| *help_arg)
}

/// Check if command has a known wrapper
pub fn is_known_wrapper(cmd: &str) -> bool {
    WRAPPER_CONFIGS.iter().any(|(name, _)| *name == cmd)
}

/// Get AI prompt extension for a wrapper
pub fn ai_prompt_extension(name: &str, help_arg: &str) -> Option<String> {
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
    fn test_find_wrapper() {
        assert_eq!(find_wrapper("curl"), Some("--help"));
        assert_eq!(find_wrapper("ffmpeg"), Some("-help"));
        assert_eq!(find_wrapper("unknown"), None);
    }

    #[test]
    fn test_ai_prompt_extension_format() {
        if let Some(prompt) = ai_prompt_extension("curl", "--help") {
            assert!(prompt.contains("targets the `curl` command"));
        }
    }
}
