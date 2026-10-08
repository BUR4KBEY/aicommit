mod drafts;
mod flow;
mod groups;

pub(crate) use drafts::{SplitCommitDraft, generate_split_commit_drafts};
pub(crate) use flow::{generate_confirm_and_commit, maybe_execute_split_flow, should_offer_split};
