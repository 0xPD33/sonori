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
/// - Collapses multiple consecutive spaces (2+) into single spaces
/// - Removes leading and trailing whitespace
/// - Converts newlines and tabs to spaces
/// - Preserves single spaces and natural word boundaries from AI model
fn normalize_whitespace(text: String) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Preserve single spaces between words
            ' ' if !result.is_empty()
                && !result.ends_with(' ')
                && !chars.peek().map_or(true, |&next| next.is_whitespace()) =>
            {
                // This is a single space between non-whitespace, keep it
                result.push(' ');
            }
            // Collapse multiple consecutive whitespace characters
            c if c.is_whitespace() => {
                // Skip all consecutive whitespace characters
                while chars.peek().map_or(false, |&next| next.is_whitespace()) {
                    chars.next();
                }
                // Add single space if not at beginning or end
                if !result.is_empty()
                    && !matches!(chars.peek(), None | Some(' ' | '\t' | '\n' | '\r'))
                {
                    result.push(' ');
                }
            }
            _ => {
                result.push(c);
            }
        }
    }

    // Clean up any trailing space that might have been added
    result.trim().to_string()
}
