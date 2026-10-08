pub fn remove_content_tags(input: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut output = input.to_owned();

    while let (Some(start), Some(end)) = (output.find(&open), output.find(&close)) {
        if end < start {
            break;
        }
        let close_end = end + close.len();
        output.replace_range(start..close_end, "");
    }

    output.trim().to_owned()
}

pub fn sanitize_model_output(input: &str) -> String {
    let mut output = input.trim().to_owned();
    for tag in ["think", "thinking"] {
        output = remove_content_tags(&output, tag);
    }
    output.trim().to_owned()
}

/// Clean up formatting habits of small models that break a commit message:
/// Markdown bold, code fences around the body, and trailing double-space line
/// breaks, and a body that starts without a blank line after the subject.
/// With `normalize_emoji`, also swap a GitMoji that contradicts the
/// conventional-commit type (e.g. "✨ docs:") for the canonical one.
pub fn tidy_small_model_commit(message: &str, normalize_emoji: bool) -> String {
    let mut lines: Vec<String> = message
        .lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(|line| line.replace("**", "").trim_end().to_owned())
        .collect();

    if let Some(subject) = lines.iter().position(|line| !line.is_empty()) {
        if normalize_emoji {
            lines[subject] = normalize_gitmoji(&lines[subject]);
        }
        // Git treats the first paragraph as the subject, so a body that starts
        // on the very next line would be folded into it.
        if lines.get(subject + 1).is_some_and(|line| !line.is_empty()) {
            lines.insert(subject + 1, String::new());
        }
    }

    lines.join("\n").trim().to_owned()
}

fn normalize_gitmoji(subject: &str) -> String {
    let Some((first, rest)) = subject.split_once(' ') else {
        return subject.to_owned();
    };
    if first.chars().any(|c| c.is_ascii_alphanumeric()) {
        return subject.to_owned();
    }

    let commit_type = rest
        .split(['(', ':', '!'])
        .next()
        .unwrap_or_default()
        .trim();
    let emoji = match commit_type {
        "feat" => "✨",
        "fix" => "🐛",
        "docs" => "📝",
        "test" => "✅",
        "refactor" => "♻️",
        "ci" => "👷",
        "build" => "⬆️",
        "chore" => "🔧",
        _ => return subject.to_owned(),
    };
    format!("{emoji} {rest}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_small_model_commit_strips_fences_bold_and_trailing_spaces() {
        let raw = "**📝 docs(blog): add a post about fm**  \n\n```\n- Add the post  \n- Tweak the cover script\n```";
        assert_eq!(
            tidy_small_model_commit(raw, false),
            "📝 docs(blog): add a post about fm\n\n- Add the post\n- Tweak the cover script"
        );
    }

    #[test]
    fn tidy_small_model_commit_separates_body_from_subject() {
        assert_eq!(
            tidy_small_model_commit("👷 ci: sync fork\n- use the API\n- fail fast", false),
            "👷 ci: sync fork\n\n- use the API\n- fail fast"
        );
        assert_eq!(
            tidy_small_model_commit("👷 ci: sync fork", false),
            "👷 ci: sync fork"
        );
    }

    #[test]
    fn tidy_small_model_commit_fixes_mismatched_gitmoji() {
        assert_eq!(
            tidy_small_model_commit("✨ docs: add blog post", true),
            "📝 docs: add blog post"
        );
        assert_eq!(
            tidy_small_model_commit("🛠️ fix(ci): repair sync", true),
            "🐛 fix(ci): repair sync"
        );
        // Unknown types and emoji-less subjects are left alone.
        assert_eq!(
            tidy_small_model_commit("🎨 style: reformat", true),
            "🎨 style: reformat"
        );
        assert_eq!(
            tidy_small_model_commit("docs: add blog post", true),
            "docs: add blog post"
        );
    }

    #[test]
    fn removes_reasoning_tags() {
        assert_eq!(
            remove_content_tags("<think>hidden</think>\nfeat: add cli", "think"),
            "feat: add cli"
        );
    }

    #[test]
    fn sanitize_model_output_removes_known_reasoning_tags() {
        assert_eq!(
            sanitize_model_output("  <thinking>hidden</thinking>\nfeat: add cli  "),
            "feat: add cli"
        );
    }
}
