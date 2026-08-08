mod commit;
mod git_guidance;
mod pull_request;
mod split_plan;

pub use commit::generate_commit_message;
pub use git_guidance::{GitGuidanceRequest, fallback_git_guidance, generate_git_guidance};
pub use pull_request::generate_pull_request;
pub use split_plan::generate_split_plan;

/// Stage transitions reported while a (possibly chunked) generation runs, so
/// callers can surface real progress instead of a static spinner message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationProgress {
    Splitting,
    Chunk { current: usize, total: usize },
    Synthesizing,
}

pub type ProgressFn<'a> = &'a (dyn Fn(GenerationProgress) + Send + Sync);

fn report(progress: Option<ProgressFn<'_>>, event: GenerationProgress) {
    if let Some(callback) = progress {
        callback(event);
    }
}
