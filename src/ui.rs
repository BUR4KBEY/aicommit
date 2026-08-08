use std::{
    collections::BTreeMap,
    fmt::Display,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Error, Result};
use console::{Term, style};
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, Editor, InquireError, MultiSelect, Select, Text};
use textwrap::Options;

const DEFAULT_MIN_CARD_WIDTH: usize = 44;
const MAX_CARD_WIDTH: usize = 92;
const DEFAULT_FILE_LIMIT: usize = 5;
const DEFAULT_ROOT_LIMIT: usize = 4;

// Vertical rhythm: sections and cards insert their own leading blank line via
// `ensure_blank_line`, so callers never manage spacing. The tracker only sees
// output routed through this module; the invariant holds because nothing else
// prints to stdout mid-session (never print a section or card while a spinner
// is live - `finish_and_clear` it first).
static LAST_LINE_BLANK: AtomicBool = AtomicBool::new(true);

fn mark_printed() {
    LAST_LINE_BLANK.store(false, Ordering::Relaxed);
}

fn ensure_blank_line() {
    if !LAST_LINE_BLANK.swap(true, Ordering::Relaxed) {
        println!();
    }
}

pub fn info(message: impl AsRef<str>) {
    println!("{}", message.as_ref());
    mark_printed();
}

pub fn success(message: impl AsRef<str>) {
    println!("{} {}", style("✔").green(), style(message.as_ref()).green());
    mark_printed();
}

pub fn warn(message: impl AsRef<str>) {
    eprintln!(
        "{} {}",
        style("warning:").yellow(),
        style(message.as_ref()).yellow()
    );
}

pub fn section(title: impl AsRef<str>) {
    ensure_blank_line();
    println!("{} {}", style("◇").cyan(), style(title.as_ref()).bold());
    mark_printed();
}

pub fn session_step(message: impl AsRef<str>) {
    println!(
        "{} {}",
        style("•").cyan().dim(),
        style(message.as_ref()).dim()
    );
    mark_printed();
}

pub fn blank_line() {
    println!();
    LAST_LINE_BLANK.store(true, Ordering::Relaxed);
}

pub fn bullet(message: impl AsRef<str>) {
    println!("  {} {}", style("•").cyan().dim(), message.as_ref());
    mark_printed();
}

pub fn secondary(message: impl AsRef<str>) {
    for line in message.as_ref().lines() {
        println!("  {}", style(line).dim());
    }
    mark_printed();
}

/// A dim next-step nudge, e.g. what to run after a command completes.
pub fn hint(message: impl AsRef<str>) {
    secondary(message);
}

pub fn metadata_row(items: &[String]) {
    if items.is_empty() {
        return;
    }

    secondary(items.join("  •  "));
}

pub fn headline(message: impl AsRef<str>) {
    println!("  {}", style(message.as_ref()).bold());
    mark_printed();
}

pub fn file_list(title: impl AsRef<str>, files: &[String]) {
    let title = title.as_ref();
    section(format!("{title} ({})", file_count_label(files.len())));
    for line in summarize_files(files, DEFAULT_FILE_LIMIT, DEFAULT_ROOT_LIMIT) {
        bullet(line);
    }
}

/// Like `file_list`, but rendered as a dim session step instead of opening a
/// new `◇` section - for lists that belong under an existing header.
pub fn file_list_step(title: impl AsRef<str>, files: &[String]) {
    session_step(format!(
        "{} ({})",
        title.as_ref(),
        file_count_label(files.len())
    ));
    for line in summarize_files(files, DEFAULT_FILE_LIMIT, DEFAULT_ROOT_LIMIT) {
        bullet(line);
    }
}

pub fn file_metadata(files: &[String]) {
    let summary = summarize_roots(files, DEFAULT_ROOT_LIMIT);
    let mut items = vec![file_count_label(files.len())];
    if !summary.is_empty() {
        items.push(format!("paths: {summary}"));
    }
    metadata_row(&items);
}

pub fn spinner(message: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message(message.into());
    pb
}

const STATUS_ROTATE_SECS: u64 = 4;
// Roughly one in this many rotations shows a line from the [rare] pool.
const RARE_STATUS_CADENCE: u64 = 6;

/// Which rotating status-detail pool from `status_messages.toml` to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusPool {
    Waiting,
    Splitting,
    Summary,
    Synthesis,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StatusMessagesFile {
    #[serde(default)]
    waiting: StatusMessagePool,
    #[serde(default)]
    splitting: StatusMessagePool,
    #[serde(default)]
    summary: StatusMessagePool,
    #[serde(default)]
    synthesis: StatusMessagePool,
    #[serde(default)]
    rare: StatusMessagePool,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StatusMessagePool {
    #[serde(default)]
    messages: Vec<String>,
}

#[derive(Debug, Default)]
struct StatusMessages {
    waiting: Vec<String>,
    splitting: Vec<String>,
    summary: Vec<String>,
    synthesis: Vec<String>,
    rare: Vec<String>,
}

impl StatusMessages {
    fn pool(&self, pool: StatusPool) -> &[String] {
        match pool {
            StatusPool::Waiting => &self.waiting,
            StatusPool::Splitting => &self.splitting,
            StatusPool::Summary => &self.summary,
            StatusPool::Synthesis => &self.synthesis,
        }
    }

    fn merge(base: StatusMessagesFile, over: StatusMessagesFile) -> Self {
        let pick = |base: StatusMessagePool, over: StatusMessagePool| {
            if over.messages.is_empty() {
                base.messages
            } else {
                over.messages
            }
        };
        Self {
            waiting: pick(base.waiting, over.waiting),
            splitting: pick(base.splitting, over.splitting),
            summary: pick(base.summary, over.summary),
            synthesis: pick(base.synthesis, over.synthesis),
            rare: pick(base.rare, over.rare),
        }
    }
}

fn status_messages() -> &'static StatusMessages {
    static MESSAGES: std::sync::OnceLock<StatusMessages> = std::sync::OnceLock::new();
    MESSAGES.get_or_init(|| {
        let built_in: StatusMessagesFile =
            toml_edit::de::from_str(include_str!("status_messages.toml")).unwrap_or_default();
        let user_override = directories::BaseDirs::new()
            .map(|base| base.home_dir().join(crate::config::STATUS_MESSAGES_FILE))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|content| toml_edit::de::from_str(&content).ok())
            .unwrap_or_default();
        StatusMessages::merge(built_in, user_override)
    })
}

// Random-enough seed without a rand dependency: RandomState keys differ per
// instance, so hashing a constant yields a fresh value per spinner.
fn status_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(0);
    hasher.finish()
}

/// A spinner with a factual stage plus a rotating status detail and elapsed
/// time, so multi-minute generations visibly progress instead of showing one
/// frozen message (e.g. `⠹ Summarizing chunk 2/4 - condensing changes (0:47)`).
pub struct StatusSpinner {
    bar: ProgressBar,
    state: std::sync::Arc<std::sync::Mutex<StatusState>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ticker: Option<std::thread::JoinHandle<()>>,
}

struct StatusState {
    stage: String,
    pool: StatusPool,
    seed: u64,
}

impl StatusState {
    /// Pick the detail line for the current rotation step: walk the stage's
    /// pool in a per-spinner pseudo-random order, occasionally swapping in a
    /// line from the `[rare]` pool so long waits stay a little surprising.
    fn detail(&self, elapsed_secs: u64) -> Option<&str> {
        let messages = status_messages();
        let pool = messages.pool(self.pool);
        if pool.is_empty() {
            return None;
        }

        let step = (elapsed_secs / STATUS_ROTATE_SECS).wrapping_add(self.seed);
        let rare = &messages.rare;
        if !rare.is_empty() && step % RARE_STATUS_CADENCE == RARE_STATUS_CADENCE - 1 {
            return Some(&rare[(step / RARE_STATUS_CADENCE) as usize % rare.len()]);
        }

        let stride = (self.seed >> 32 | 1) as usize;
        Some(&pool[(step as usize).wrapping_mul(stride) % pool.len()])
    }
}

impl StatusSpinner {
    pub fn start(stage: impl Into<String>, pool: StatusPool) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed:.dim}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.enable_steady_tick(std::time::Duration::from_millis(80));

        let state = std::sync::Arc::new(std::sync::Mutex::new(StatusState {
            stage: stage.into(),
            pool,
            seed: status_seed(),
        }));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        Self::render(&bar, &state);

        let ticker = {
            let bar = bar.clone();
            let state = std::sync::Arc::clone(&state);
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    Self::render(&bar, &state);
                }
            })
        };

        Self {
            bar,
            state,
            stop,
            ticker: Some(ticker),
        }
    }

    pub fn set_stage(&self, stage: impl Into<String>, pool: StatusPool) {
        if let Ok(mut state) = self.state.lock() {
            state.stage = stage.into();
            state.pool = pool;
        }
        Self::render(&self.bar, &self.state);
    }

    /// Map generator stage transitions onto spinner stages. `task_label`
    /// names the deliverable ("commit message", "pull request draft").
    pub fn on_generation_progress(
        &self,
        event: crate::generator::GenerationProgress,
        task_label: &str,
    ) {
        use crate::generator::GenerationProgress;

        match event {
            GenerationProgress::Splitting => {
                self.set_stage(
                    format!("Splitting diff for {task_label}"),
                    StatusPool::Splitting,
                );
            }
            GenerationProgress::Chunk {
                current: 1,
                total: 1,
            } => {
                self.set_stage(format!("Generating {task_label}"), StatusPool::Waiting);
            }
            GenerationProgress::Chunk { current, total } => {
                if current == 1 {
                    self.note(format!(
                        "Large diff split into {total} chunks - one AI request per chunk plus synthesis"
                    ));
                }
                self.set_stage(
                    format!("Summarizing chunk {current}/{total}"),
                    StatusPool::Summary,
                );
            }
            GenerationProgress::Synthesizing => {
                self.set_stage(format!("Synthesizing {task_label}"), StatusPool::Synthesis);
            }
        }
    }

    /// Print a dim one-liner above the spinner without disturbing it.
    pub fn note(&self, message: impl AsRef<str>) {
        self.bar.println(format!(
            "{} {}",
            style("•").cyan().dim(),
            style(message.as_ref()).dim()
        ));
        mark_printed();
    }

    pub fn finish_and_clear(self) {
        // Drop does the work; taking self by value ends the ticker eagerly.
    }

    fn render(bar: &ProgressBar, state: &std::sync::Mutex<StatusState>) {
        let Ok(state) = state.lock() else {
            return;
        };
        let message = match state.detail(bar.elapsed().as_secs()) {
            Some(detail) => format!("{} - {}", state.stage, detail),
            None => state.stage.clone(),
        };
        bar.set_message(message);
    }
}

impl Drop for StatusSpinner {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
        self.bar.finish_and_clear();
    }
}

pub fn primary_card(title: &str, body: &str) {
    ensure_blank_line();
    for line in render_card_lines(title, body, card_width()) {
        println!("{line}");
    }
    mark_printed();
}

pub fn markdown_card(title: &str, body: &str) {
    ensure_blank_line();
    for line in render_markdown_card_lines(title, body, card_width()) {
        println!("{line}");
    }
    mark_printed();
}

/// Align inquire's prompt rendering with the module's glyph vocabulary
/// (`?` pending, `✔` answered, `❯` highlighted). Call once at startup.
pub fn init_prompt_theme() {
    use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};

    inquire::set_global_render_config(
        RenderConfig::default_colored()
            .with_prompt_prefix(Styled::new("?").with_fg(Color::LightCyan))
            .with_answered_prompt_prefix(Styled::new("✔").with_fg(Color::LightGreen))
            .with_answer(StyleSheet::new().with_fg(Color::LightCyan))
            .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
            .with_highlighted_option_prefix(Styled::new("❯").with_fg(Color::LightCyan)),
    );
}

/// Wrap `text` in an OSC-8 terminal hyperlink when stdout is an interactive,
/// color-capable terminal; plain text otherwise. Never use inside card bodies:
/// the escape sequence would break their fixed-width borders.
pub fn hyperlink(text: &str, url: &str) -> String {
    if console::colors_enabled() && Term::stdout().is_term() {
        format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
    } else {
        text.to_owned()
    }
}

pub fn confirm(message: &str, default: bool) -> Result<bool> {
    let answer = Confirm::new(message).with_default(default).prompt()?;
    mark_printed();
    Ok(answer)
}

pub fn select<T>(message: &str, options: Vec<T>) -> Result<T>
where
    T: Clone + Display,
{
    let answer = Select::new(message, options).prompt()?;
    mark_printed();
    Ok(answer)
}

pub fn multiselect(message: &str, options: Vec<String>) -> Result<Vec<String>> {
    let answer = MultiSelect::new(message, options).prompt()?;
    mark_printed();
    Ok(answer)
}

pub fn text(message: &str, initial: Option<&str>) -> Result<String> {
    let prompt = Text::new(message);
    let prompt = if let Some(initial) = initial {
        prompt.with_initial_value(initial)
    } else {
        prompt
    };
    let answer = prompt.prompt()?;
    mark_printed();
    Ok(answer)
}

pub fn editor(message: &str, initial: &str) -> Result<String> {
    let answer = Editor::new(message)
        .with_predefined_text(initial)
        .with_file_extension(".md")
        .prompt()?;
    mark_printed();
    Ok(answer)
}

pub fn is_prompt_cancelled(error: &Error) -> bool {
    matches!(
        error.downcast_ref::<InquireError>(),
        Some(InquireError::OperationCanceled | InquireError::OperationInterrupted)
    )
}

pub(crate) fn file_count_label(count: usize) -> String {
    match count {
        1 => "1 file".to_owned(),
        value => format!("{value} files"),
    }
}

pub(crate) fn summarize_files(
    files: &[String],
    direct_limit: usize,
    root_limit: usize,
) -> Vec<String> {
    let mut lines = files.iter().take(direct_limit).cloned().collect::<Vec<_>>();

    let remaining = files.len().saturating_sub(direct_limit);
    if remaining > 0 {
        let roots = summarize_roots(files, root_limit);
        if roots.is_empty() {
            lines.push(format!("+{remaining} more"));
        } else {
            lines.push(format!("+{remaining} more across {roots}"));
        }
    }

    lines
}

pub(crate) fn summarize_roots(files: &[String], limit: usize) -> String {
    let mut groups = BTreeMap::<String, usize>::new();
    for file in files {
        *groups.entry(file_root(file)).or_default() += 1;
    }

    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(left_root, left_count), (right_root, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_root.cmp(right_root))
    });

    groups
        .into_iter()
        .map(|(root, count)| format!("{root} ({count})"))
        .take(limit)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn render_card_lines(title: &str, body: &str, width: usize) -> Vec<String> {
    let dimensions = card_dimensions(width);
    let wrapped_lines = wrap_plain_lines(body, dimensions.content_width);
    render_card_frame(title, &wrapped_lines, dimensions)
}

fn render_markdown_card_lines(title: &str, body: &str, width: usize) -> Vec<String> {
    let dimensions = card_dimensions(width);
    let rendered_lines = render_markdown_lines(body, dimensions.content_width);
    render_card_frame(title, &rendered_lines, dimensions)
}

fn card_width() -> usize {
    let (_, columns) = Term::stdout().size();
    usize::from(columns)
        .saturating_sub(4)
        .clamp(DEFAULT_MIN_CARD_WIDTH, MAX_CARD_WIDTH)
}

#[derive(Clone, Copy)]
struct CardDimensions {
    content_width: usize,
    border_inner_width: usize,
}

fn card_dimensions(width: usize) -> CardDimensions {
    let width = width.clamp(DEFAULT_MIN_CARD_WIDTH, MAX_CARD_WIDTH);
    let content_width = width.saturating_sub(4).max(20);
    CardDimensions {
        content_width,
        border_inner_width: content_width + 2,
    }
}

fn render_card_frame(
    title: &str,
    body_lines: &[String],
    dimensions: CardDimensions,
) -> Vec<String> {
    let title = format!(" {title} ");
    let title_width = console::measure_text_width(&title);
    let top_fill = "─".repeat(dimensions.border_inner_width.saturating_sub(title_width));
    let mut lines = vec![format!("  {}", style(format!("┌{title}{top_fill}┐")).dim())];

    if body_lines.is_empty() {
        lines.push(render_card_body_line("", dimensions.content_width));
    } else {
        for line in body_lines {
            lines.push(render_card_body_line(line, dimensions.content_width));
        }
    }

    lines.push(format!(
        "  {}",
        style(format!("└{}┘", "─".repeat(dimensions.border_inner_width))).dim()
    ));
    lines
}

fn render_card_body_line(line: &str, content_width: usize) -> String {
    let visible_width = console::measure_text_width(line);
    let padding = " ".repeat(content_width.saturating_sub(visible_width));
    format!(
        "  {}{}{}{}",
        style("│ ").dim(),
        line,
        padding,
        style(" │").dim()
    )
}

fn wrap_plain_lines(body: &str, content_width: usize) -> Vec<String> {
    let mut wrapped_lines = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            wrapped_lines.push(String::new());
            continue;
        }

        let options = Options::new(content_width)
            .break_words(false)
            .word_separator(textwrap::WordSeparator::AsciiSpace);
        wrapped_lines.extend(
            textwrap::wrap(line, &options)
                .into_iter()
                .map(|segment| segment.into_owned()),
        );
    }

    if wrapped_lines.is_empty() {
        wrapped_lines.push(String::new());
    }

    wrapped_lines
}

fn render_markdown_lines(body: &str, width: usize) -> Vec<String> {
    let rendered = format!("{}", markdown_skin().text(body, Some(width)));
    let lines = rendered.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn markdown_skin() -> termimad::MadSkin {
    let mut skin = termimad::MadSkin {
        list_items_indentation_mode: termimad::ListItemsIndentationMode::Block,
        ..termimad::MadSkin::default()
    };
    for header in &mut skin.headers {
        header.align = termimad::Alignment::Left;
    }
    skin
}

fn file_root(file: &str) -> String {
    let path = Path::new(file);
    let mut parts = path.iter().filter_map(|part| part.to_str());
    match (parts.next(), parts.next()) {
        (Some(first), Some(_)) => format!("{first}/"),
        (None, _) => "root files".to_owned(),
        _ => "root files".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_status_messages_parse_with_populated_pools() {
        let parsed: StatusMessagesFile =
            toml_edit::de::from_str(include_str!("status_messages.toml")).unwrap();
        assert!(!parsed.waiting.messages.is_empty());
        assert!(!parsed.splitting.messages.is_empty());
        assert!(!parsed.summary.messages.is_empty());
        assert!(!parsed.synthesis.messages.is_empty());
        assert!(!parsed.rare.messages.is_empty());
    }

    #[test]
    fn status_detail_rotates_through_pool_and_rare_lines() {
        let state = StatusState {
            stage: "Generating".to_owned(),
            pool: StatusPool::Waiting,
            seed: 7,
        };
        let mut seen = std::collections::BTreeSet::new();
        for step in 0..40 {
            if let Some(detail) = state.detail(step * STATUS_ROTATE_SECS) {
                seen.insert(detail.to_owned());
            }
        }
        // Over enough rotations the detail line varies rather than freezing.
        assert!(seen.len() > 2);
    }

    #[test]
    fn hyperlink_falls_back_to_plain_text_without_a_terminal() {
        // Tests run with stdout piped, so the tty gate rejects the link.
        assert_eq!(hyperlink("abc123", "https://example.test"), "abc123");
    }

    #[test]
    fn measure_text_width_counts_osc8_hyperlink_payload() {
        // console does NOT strip OSC-8 sequences, so a hyperlink inside a
        // card body would wreck the fixed-width borders - hence the rule in
        // `hyperlink`'s doc comment. If console ever learns to strip them,
        // this assertion will flag that the rule can be relaxed.
        let linked = format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", "https://x.test", "abc");
        assert!(console::measure_text_width(&linked) > 3);
    }

    #[test]
    fn summarize_files_truncates_large_lists() {
        let files = vec![
            "src/main.rs".to_owned(),
            "src/lib.rs".to_owned(),
            "src/ui.rs".to_owned(),
            "docs/usage.md".to_owned(),
            "README.md".to_owned(),
            "tests/cli.rs".to_owned(),
        ];

        let lines = summarize_files(&files, 3, 3);
        assert_eq!(lines[0], "src/main.rs");
        assert_eq!(lines[1], "src/lib.rs");
        assert_eq!(lines[2], "src/ui.rs");
        assert_eq!(
            lines[3],
            "+3 more across src/ (3), docs/ (1), root files (1)"
        );
    }

    #[test]
    fn summarize_roots_groups_top_level_paths() {
        let files = vec![
            "src/main.rs".to_owned(),
            "src/lib.rs".to_owned(),
            "README.md".to_owned(),
            "docs/usage.md".to_owned(),
        ];

        assert_eq!(
            summarize_roots(&files, 4),
            "src/ (2), docs/ (1), root files (1)"
        );
    }

    #[test]
    fn render_card_lines_wraps_long_content() {
        let lines = render_card_lines(
            "Generated commit",
            "feat(ui): add a much longer line that should wrap inside the bordered card cleanly",
            44,
        );

        assert!(lines[0].contains("Generated commit"));
        assert!(lines.len() > 4);
        assert!(lines.iter().all(|line| line.starts_with("  ")));
        let widths = lines
            .iter()
            .map(|line| console::measure_text_width(line))
            .collect::<Vec<_>>();
        assert!(widths.windows(2).all(|window| window[0] == window[1]));
    }

    #[test]
    fn render_markdown_card_lines_preserves_markdown_formatting() {
        let lines = render_markdown_card_lines(
            "AI review",
            "**Warning**\n\n1. **Example finding**\n`src/main.rs`\n- keep markdown formatting readable",
            52,
        );

        assert!(lines[0].contains("AI review"));
        assert!(lines.iter().all(|line| line.starts_with("  ")));
        assert!(lines.iter().all(|line| !line.contains("**Warning**")));
        assert!(lines.iter().any(|line| line.contains("Warning")));
        let widths = lines
            .iter()
            .map(|line| console::measure_text_width(line))
            .collect::<Vec<_>>();
        assert!(widths.windows(2).all(|window| window[0] == window[1]));
    }

    #[test]
    fn render_markdown_card_lines_keep_width_with_mixed_width_content() {
        let lines = render_markdown_card_lines(
            "AI review",
            "## Warning\n- plain ascii text\n- wide chars: 漢字 mixed with `code`\n- wrapped line with markdown emphasis around **important** details",
            58,
        );

        assert!(lines[0].contains("AI review"));
        let widths = lines
            .iter()
            .map(|line| console::measure_text_width(line))
            .collect::<Vec<_>>();
        assert!(widths.windows(2).all(|window| window[0] == window[1]));
    }
}
