//! Text post-processing for transcription cleanup
//!
//! Handles removal of artifacts like leading/trailing dashes and
//! normalization of whitespace in transcription output.

use crate::config::PostProcessConfig;

/// Post-process transcription text to clean up artifacts
///
/// Applies configured cleaning rules to remove dashes and normalize whitespace
/// based on the provided configuration.
pub fn post_process_text(text: String, config: &PostProcessConfig) -> String {
    if !config.enabled {
        return text;
    }

    let mut processed = text;

    if config.remove_leading_dashes {
        processed = remove_leading_dashes(processed);
    }

    if config.remove_trailing_dashes {
        processed = remove_trailing_dashes(processed);
    }

    if config.normalize_whitespace {
        processed = normalize_whitespace(processed);
    }

    processed
}

/// Remove leading dashes and following whitespace
///
/// Removes patterns like "- text" at the start of a string
fn remove_leading_dashes(text: String) -> String {
    let trimmed = text.trim_start();
    if trimmed.starts_with('-') {
        trimmed.trim_start_matches('-').trim_start().to_string()
    } else {
        text
    }
}

/// Remove trailing dashes and preceding whitespace
///
/// Removes patterns like "text -" at the end of a string
fn remove_trailing_dashes(text: String) -> String {
    let trimmed = text.trim_end();
    if trimmed.ends_with('-') {
        trimmed.trim_end_matches('-').trim_end().to_string()
    } else {
        text
    }
}

/// Normalize whitespace in text
///
/// - Collapses multiple consecutive spaces into single space
/// - Removes leading and trailing whitespace
/// - Converts newlines and tabs to spaces
fn normalize_whitespace(text: String) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> PostProcessConfig {
        PostProcessConfig {
            enabled: true,
            remove_leading_dashes: true,
            remove_trailing_dashes: true,
            normalize_whitespace: true,
        }
    }

    #[test]
    fn test_remove_leading_dashes() {
        assert_eq!(
            remove_leading_dashes("- hello world".to_string()),
            "hello world"
        );
        assert_eq!(remove_leading_dashes("-- hello".to_string()), "hello");
        assert_eq!(
            remove_leading_dashes("hello world".to_string()),
            "hello world"
        );
    }

    #[test]
    fn test_remove_trailing_dashes() {
        assert_eq!(
            remove_trailing_dashes("hello world -".to_string()),
            "hello world"
        );
        assert_eq!(remove_trailing_dashes("hello --".to_string()), "hello");
        assert_eq!(
            remove_trailing_dashes("hello world".to_string()),
            "hello world"
        );
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(
            normalize_whitespace("hello   world".to_string()),
            "hello world"
        );
        assert_eq!(
            normalize_whitespace("  hello\nworld  ".to_string()),
            "hello world"
        );
        assert_eq!(
            normalize_whitespace("hello\t\tworld".to_string()),
            "hello world"
        );
    }

    #[test]
    fn test_post_process_all_enabled() {
        let text = "  - hello   world -  ".to_string();
        let config = default_config();
        assert_eq!(post_process_text(text, &config), "hello world");
    }

    #[test]
    fn test_post_process_disabled() {
        let text = "  - hello   world -  ".to_string();
        let config = PostProcessConfig {
            enabled: false,
            ..default_config()
        };
        assert_eq!(post_process_text(text, &config), "  - hello   world -  ");
    }
}
