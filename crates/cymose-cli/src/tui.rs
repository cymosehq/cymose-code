//! The terminal client: session tree, detail pane, prompt line.
//!
//! Rendering only. Everything it shows comes out of `cymose-core`, so the VS
//! Code sidebar can show the same thing without a second implementation of
//! how a tree is built or what a session inherits.

use std::time::Duration;

use anyhow::Result;
use cymose_core::context::ContextBuilder;
use cymose_core::session::TreeNode;
use cymose_core::{SessionStatus, Store};
// Crossterm comes from ratatui rather than as a direct dependency: tui-textarea
// takes events from ratatui's copy, and two crossterm versions in the tree make
// a key event from one unusable by the other.
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::DefaultTerminal;
use tui_textarea::TextArea;

pub fn run(store: Store, workspace: String) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = App::new(store, workspace)?.run(&mut terminal);
    // Restore before propagating: an error printed into raw mode is unreadable.
    ratatui::restore();
    result
}

#[derive(PartialEq)]
enum Mode {
    Navigate,
    Prompt,
}

struct App {
    store: Store,
    workspace: String,
    nodes: Vec<TreeNode>,
    /// Indices into `nodes`, in display order, with the depth to indent by.
    rows: Vec<(usize, usize)>,
    selection: ListState,
    mode: Mode,
    input: TextArea<'static>,
    log: Vec<String>,
}

impl App {
    fn new(store: Store, workspace: String) -> Result<Self> {
        let mut input = TextArea::default();
        input.set_placeholder_text("describe the task, Enter to start a session");
        let mut app = App {
            store,
            workspace,
            nodes: Vec::new(),
            rows: Vec::new(),
            selection: ListState::default(),
            mode: Mode::Navigate,
            input,
            log: vec!["agent turns are not wired up yet — see docs/api-contract.md".into()],
        };
        app.reload()?;
        Ok(app)
    }

    fn reload(&mut self) -> Result<()> {
        self.nodes = self.store.tree(&self.workspace)?;
        self.rows = flatten(&self.nodes, None, 0);
        if !self.rows.is_empty() && self.selection.selected().is_none() {
            self.selection.select(Some(0));
        }
        Ok(())
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            // Poll rather than block: once turns stream, this loop also has to
            // drain agent events, and a blocking read would sit on them.
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match self.mode {
                Mode::Navigate => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
                    KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
                    KeyCode::Char('i') | KeyCode::Char('n') => self.mode = Mode::Prompt,
                    KeyCode::Char('r') => self.reload()?,
                    _ => {}
                },
                Mode::Prompt => match key.code {
                    KeyCode::Esc => self.mode = Mode::Navigate,
                    KeyCode::Enter => {
                        let title = self.input.lines().join(" ").trim().to_string();
                        if !title.is_empty() {
                            self.start_session(&title)?;
                            self.input = TextArea::default();
                        }
                        self.mode = Mode::Navigate;
                    }
                    _ => {
                        self.input.input(key);
                    }
                },
            }
        }
    }

    /// A new session inherits from whatever is selected — which is the whole
    /// point of the tree being on screen while you type.
    fn start_session(&mut self, title: &str) -> Result<()> {
        let parent = self
            .selection
            .selected()
            .and_then(|i| self.rows.get(i))
            .map(|(node, _)| self.nodes[*node].id.clone());

        let session = self
            .store
            .create_session(&self.workspace, title, parent.as_deref())?;
        let inherited = ContextBuilder::new(&self.store).build(&session.id)?;
        self.log.push(format!(
            "session {} created, inheriting {} summary(ies)",
            &session.id[..8],
            inherited.items.len()
        ));
        self.reload()?;
        Ok(())
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selection.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.rows.len() as isize - 1);
        self.selection.select(Some(next as usize));
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .areas(frame.area());
        let [tree_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
                .areas(body);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Cymose Code ", Style::new().bold()),
                Span::raw(format!(" {} sessions", self.nodes.len())),
            ]))
            .style(Style::new().reversed()),
            header,
        );

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|(index, depth)| {
                let node = &self.nodes[*index];
                let (mark, colour) = match node.status {
                    SessionStatus::Done => ("✓", Color::Green),
                    SessionStatus::Failed => ("✗", Color::Red),
                    SessionStatus::Running => ("⟳", Color::Yellow),
                    SessionStatus::Pending => ("·", Color::Gray),
                };
                ListItem::new(Line::from(vec![
                    Span::raw("  ".repeat(*depth)),
                    Span::styled(format!("{mark} "), Style::new().fg(colour)),
                    Span::raw(node.title.clone()),
                ]))
            })
            .collect();

        frame.render_stateful_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" tree "))
                .highlight_style(Style::new().reversed()),
            tree_area,
            &mut self.selection,
        );

        frame.render_widget(
            Paragraph::new(self.detail())
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(" session ")),
            detail_area,
        );

        let title = match self.mode {
            Mode::Navigate => " j/k move · i new session · r reload · q quit ",
            Mode::Prompt => " Enter to create · Esc to cancel ",
        };
        self.input
            .set_block(Block::default().borders(Borders::ALL).title(title));
        frame.render_widget(&self.input, footer);
    }

    fn detail(&self) -> String {
        let Some((index, _)) = self.selection.selected().and_then(|i| self.rows.get(i)) else {
            return self.log.join("\n");
        };
        let node = &self.nodes[*index];

        let mut out = format!(
            "{}\n{}\nstatus: {}\nmodel: {}\n",
            node.title,
            node.id,
            node.status.as_str(),
            node.model.as_deref().unwrap_or("—"),
        );
        if let Some(summary) = &node.summary {
            out.push_str(&format!("\nsummary\n{summary}\n"));
        }
        match ContextBuilder::new(&self.store).build(&node.id) {
            Ok(ctx) if !ctx.is_empty() => {
                out.push_str("\ninherited\n");
                out.push_str(&ctx.render());
            }
            Ok(_) => out.push_str("\ninherited: nothing (root session)\n"),
            // Drawing must not fail on a store hiccup — show it in the pane.
            Err(e) => out.push_str(&format!("\ninherited: unavailable ({e})\n")),
        }
        out.push_str("\n---\n");
        out.push_str(&self.log.join("\n"));
        out
    }
}

/// Depth-first display order: each node followed by its children.
fn flatten(nodes: &[TreeNode], parent: Option<&str>, depth: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.parent_id.as_deref() != parent {
            continue;
        }
        out.push((index, depth));
        out.extend(flatten(nodes, Some(&node.id), depth + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn node(id: &str, parent: Option<&str>) -> TreeNode {
        TreeNode {
            id: id.into(),
            parent_id: parent.map(str::to_string),
            title: id.into(),
            status: SessionStatus::Done,
            model: None,
            summary: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn children_follow_their_parent_and_are_indented() {
        let nodes = vec![
            node("root", None),
            node("child", Some("root")),
            node("grandchild", Some("child")),
            node("other-root", None),
        ];
        let rows = flatten(&nodes, None, 0);
        let order: Vec<_> = rows
            .iter()
            .map(|(i, d)| (nodes[*i].id.as_str(), *d))
            .collect();
        assert_eq!(
            order,
            vec![
                ("root", 0),
                ("child", 1),
                ("grandchild", 2),
                ("other-root", 0)
            ]
        );
    }

    #[test]
    fn an_orphaned_node_is_not_drawn_twice_or_lost_in_a_loop() {
        // parent_id pointing at a session from another workspace: it must not
        // appear at the root, and it must not send the walk into a loop.
        let nodes = vec![node("root", None), node("orphan", Some("missing"))];
        let rows = flatten(&nodes, None, 0);
        assert_eq!(rows.len(), 1);
    }
}
