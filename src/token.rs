use std::sync::OnceLock;

use anyhow::Result;
use tiktoken_rs::{CoreBPE, cl100k_base};

use crate::ai::ChatMessage;

// Building the cl100k BPE encoder parses a ~100k-entry vocabulary and costs
// tens of milliseconds; doing it per count_tokens call made split_diff
// quadratic on large diffs. Build it once and keep the whitespace fallback.
fn encoder() -> Option<&'static CoreBPE> {
    static ENCODER: OnceLock<Option<CoreBPE>> = OnceLock::new();
    ENCODER.get_or_init(|| cl100k_base().ok()).as_ref()
}

pub fn count_tokens(input: &str) -> usize {
    match encoder() {
        Some(encoder) => encoder.encode_with_special_tokens(input).len(),
        None => input.split_whitespace().count(),
    }
}

pub fn count_messages(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| count_tokens(&message.content) + 4)
        .sum()
}

pub fn split_diff(diff: &str, max_tokens: usize) -> Result<Vec<String>> {
    let max_tokens = max_tokens.max(1);

    if count_tokens(diff) <= max_tokens {
        return Ok(vec![diff.to_owned()]);
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    // Running sum of per-line counts (+1 per newline join). Counting each line
    // once keeps this linear; the sum slightly over-estimates the joined text's
    // count, so chunks stay within budget.
    let mut current_tokens = 0usize;

    for line in diff.lines() {
        let line_tokens = count_tokens(line);
        let joiner = usize::from(!current.is_empty());

        if current_tokens + joiner + line_tokens > max_tokens {
            if current.is_empty() {
                chunks.extend(split_long_line(line, max_tokens)?);
                continue;
            }
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
            if line_tokens > max_tokens {
                chunks.extend(split_long_line(line, max_tokens)?);
            } else {
                current.push_str(line);
                current_tokens = line_tokens;
            }
        } else {
            if joiner == 1 {
                current.push('\n');
            }
            current.push_str(line);
            current_tokens += joiner + line_tokens;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    Ok(chunks)
}

fn split_long_line(line: &str, max_tokens: usize) -> Result<Vec<String>> {
    if count_tokens(line) <= max_tokens {
        return Ok(vec![line.to_owned()]);
    }

    let max_chars = (max_tokens * 4).max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();

    for ch in line.chars() {
        if current.len() + ch.len_utf8() > max_chars && !current.is_empty() {
            chunks.push(current);
            current = ch.to_string();
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_diff_keeps_small_diff_whole() {
        let chunks = split_diff("one\ntwo", 100).unwrap();
        assert_eq!(chunks, vec!["one\ntwo"]);
    }

    #[test]
    fn split_diff_splits_single_long_line() {
        let line = "word ".repeat(100);
        let chunks = split_diff(line.trim(), 10).unwrap();
        assert!(chunks.len() > 1);
        assert_eq!(chunks.join(""), line.trim());
    }

    #[test]
    fn split_diff_chunks_multi_line_diff_losslessly() {
        let diff = (0..200)
            .map(|i| format!("line {i} with some diff content"))
            .collect::<Vec<_>>()
            .join("\n");
        let max_tokens = 50;
        let chunks = split_diff(&diff, max_tokens).unwrap();

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(count_tokens(chunk) <= max_tokens);
        }
        let rejoined = chunks
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(rejoined, diff);
    }

    #[test]
    fn split_diff_handles_large_diff_quickly() {
        // Regression tripwire for the quadratic re-tokenization cliff: a
        // multi-thousand-line over-budget diff must split in well under CI
        // timeout territory (previously this shape took minutes to hours).
        let diff = (0..20_000)
            .map(|i| format!("+    let value_{i} = compute_something({i});"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = split_diff(&diff, 2_000).unwrap();
        assert!(chunks.len() > 1);
    }

    #[test]
    fn counts_messages_with_overhead() {
        let messages = vec![ChatMessage::user("hello")];
        assert!(count_messages(&messages) >= 5);
    }
}
