//! Builds a new session's starting context out of its ancestors' summaries.
//!
//! The product claim rests on this file: a child inherits a few sentences per
//! ancestor instead of their transcripts, so context stays roughly flat as the
//! tree deepens instead of growing with everything that came before.

use crate::error::Result;
use crate::session::{SessionStatus, Summary};
use crate::store::Store;

/// Rough characters-per-token. Only used to keep the inherited block inside a
/// budget; the server counts tokens for real.
const CHARS_PER_TOKEN: usize = 4;

pub struct ContextBuilder<'a> {
    store: &'a Store,
    /// Ceiling on the inherited block, in tokens. The rest of the window is for
    /// the actual task, the files it reads, and the turn itself.
    budget_tokens: usize,
}

/// One ancestor's contribution, kept separate so a client can show what a
/// session inherited and from where.
#[derive(Debug, Clone)]
pub struct InheritedItem {
    pub session_id: String,
    pub title: String,
    pub outcome: SessionStatus,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct InheritedContext {
    pub items: Vec<InheritedItem>,
    /// Ancestors that existed but didn't fit the budget. Reported rather than
    /// dropped silently — "this session can't see the auth work" is something
    /// the user needs to know before blaming the model.
    pub dropped: usize,
}

impl InheritedContext {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Rendered for the model. Nearest ancestor first: the closest work is the
    /// most likely to matter, and it is also what survives truncation.
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "Context inherited from earlier sessions in this project. These are \
             summaries, not transcripts — treat them as established fact, and do \
             not repeat an approach recorded as failed.\n",
        );
        for item in &self.items {
            out.push_str(&format!(
                "\n- [{}] {}: {}",
                item.outcome.as_str(),
                item.title,
                item.text.trim()
            ));
        }
        out.push('\n');
        out
    }
}

impl<'a> ContextBuilder<'a> {
    pub fn new(store: &'a Store) -> Self {
        ContextBuilder {
            store,
            budget_tokens: 2000,
        }
    }

    pub fn with_budget(mut self, budget_tokens: usize) -> Self {
        self.budget_tokens = budget_tokens;
        self
    }

    /// Ancestor summaries, nearest first, truncated to the budget.
    ///
    /// An ancestor with no summary contributes nothing: a session that is still
    /// running has no conclusions to pass down, and inventing one from its
    /// title would be worse than silence.
    pub fn build(&self, session_id: &str) -> Result<InheritedContext> {
        let ancestors = self.store.ancestors(session_id)?;

        let mut ctx = InheritedContext::default();
        let mut used = 0usize;
        let budget_chars = self.budget_tokens * CHARS_PER_TOKEN;

        for ancestor in ancestors {
            let Some(summary) = self.store.summary(&ancestor.id)? else {
                continue;
            };
            let cost = summary.text.len() + ancestor.title.len() + 16;
            if used + cost > budget_chars {
                ctx.dropped += 1;
                continue;
            }
            used += cost;
            ctx.items.push(InheritedItem {
                session_id: ancestor.id,
                title: ancestor.title,
                outcome: summary.outcome,
                text: summary.text,
            });
        }
        Ok(ctx)
    }

    /// Sibling attempts under the same parent — the "we already tried that"
    /// signal that makes `explore` worth running. Failures are kept ahead of
    /// successes: knowing what not to do again saves more than a second
    /// description of what worked.
    pub fn siblings(&self, session_id: &str) -> Result<Vec<Summary>> {
        let session = self.store.session(session_id)?;
        let Some(parent_id) = session.parent_id else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for sibling in self.store.children(&parent_id)? {
            if sibling.id == session_id {
                continue;
            }
            if let Some(summary) = self.store.summary(&sibling.id)? {
                out.push(summary);
            }
        }
        out.sort_by_key(|s| s.outcome != SessionStatus::Failed);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::Path;

    fn seeded() -> (Store, String) {
        let store = Store::open_in_memory().unwrap();
        let ws = store
            .ensure_workspace(Path::new("/tmp/proj"), "proj")
            .unwrap();
        (store, ws)
    }

    fn summarize(store: &Store, id: &str, text: &str, outcome: SessionStatus) {
        store
            .put_summary(&Summary {
                session_id: id.to_string(),
                text: text.to_string(),
                files_touched: vec![],
                outcome,
                created_at: Utc::now(),
            })
            .unwrap();
    }

    #[test]
    fn a_child_inherits_nearest_ancestor_first() {
        let (store, ws) = seeded();
        let auth = store.create_session(&ws, "auth bug", None).unwrap();
        summarize(&store, &auth.id, "JWT expiry fixed", SessionStatus::Done);
        let limiter = store
            .create_session(&ws, "rate limiter", Some(&auth.id))
            .unwrap();
        summarize(
            &store,
            &limiter.id,
            "token bucket raced",
            SessionStatus::Failed,
        );
        let v2 = store
            .create_session(&ws, "rate limiter v2", Some(&limiter.id))
            .unwrap();

        let ctx = ContextBuilder::new(&store).build(&v2.id).unwrap();
        let titles: Vec<_> = ctx.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["rate limiter", "auth bug"]);

        let rendered = ctx.render();
        assert!(rendered.contains("[failed] rate limiter: token bucket raced"));
    }

    #[test]
    fn an_unsummarised_ancestor_contributes_nothing() {
        let (store, ws) = seeded();
        let running = store.create_session(&ws, "in flight", None).unwrap();
        let child = store
            .create_session(&ws, "child", Some(&running.id))
            .unwrap();

        let ctx = ContextBuilder::new(&store).build(&child.id).unwrap();
        assert!(ctx.is_empty());
        assert_eq!(ctx.render(), "");
    }

    #[test]
    fn the_budget_drops_the_furthest_ancestors_and_says_so() {
        let (store, ws) = seeded();
        let mut parent: Option<String> = None;
        let mut ids = Vec::new();
        for i in 0..5 {
            let s = store
                .create_session(&ws, &format!("session {i}"), parent.as_deref())
                .unwrap();
            summarize(&store, &s.id, &"x".repeat(100), SessionStatus::Done);
            parent = Some(s.id.clone());
            ids.push(s.id);
        }
        let leaf = store
            .create_session(&ws, "leaf", parent.as_deref())
            .unwrap();

        // ~50 chars of budget: two ancestors at most.
        let ctx = ContextBuilder::new(&store)
            .with_budget(60)
            .build(&leaf.id)
            .unwrap();
        assert!(ctx.items.len() < 5);
        assert_eq!(ctx.items.len() + ctx.dropped, 5);
        // What survived is the nearest work.
        assert_eq!(ctx.items[0].title, "session 4");
    }

    #[test]
    fn failed_siblings_come_first() {
        let (store, ws) = seeded();
        let parent = store.create_session(&ws, "rate limiter", None).unwrap();
        let a = store
            .create_session(&ws, "token bucket", Some(&parent.id))
            .unwrap();
        let b = store
            .create_session(&ws, "sliding window", Some(&parent.id))
            .unwrap();
        let c = store
            .create_session(&ws, "third try", Some(&parent.id))
            .unwrap();
        summarize(&store, &a.id, "race condition", SessionStatus::Failed);
        summarize(&store, &b.id, "works, merged", SessionStatus::Done);

        let siblings = ContextBuilder::new(&store).siblings(&c.id).unwrap();
        assert_eq!(siblings.len(), 2);
        assert_eq!(siblings[0].outcome, SessionStatus::Failed);
    }
}
