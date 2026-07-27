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
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
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
            Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                .areas(body);

        self.draw_header(frame, header);
        self.draw_tree(frame, tree_area);
        self.draw_detail(frame, detail_area);
        self.draw_input(frame, footer);
    }

    /// One line, and it has to earn it: what you're in, what it costs, what
    /// state things are in. A reversed bar the width of the terminal is the
    /// default every TUI ships with and reads as unfinished.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let done = self
            .nodes
            .iter()
            .filter(|n| n.status == SessionStatus::Done)
            .count();
        let failed = self
            .nodes
            .iter()
            .filter(|n| n.status == SessionStatus::Failed)
            .count();

        let mut spans = vec![
            Span::styled(" cymose ", Style::new().fg(ACCENT).bold()),
            Span::styled("· ", Style::new().fg(FAINT)),
            Span::styled(
                format!(
                    "{} session{}",
                    self.nodes.len(),
                    if self.nodes.len() == 1 { "" } else { "s" }
                ),
                Style::new().fg(MUTED),
            ),
        ];
        if done > 0 {
            spans.push(Span::styled(format!("  {done} ✓"), Style::new().fg(DONE)));
        }
        if failed > 0 {
            spans.push(Span::styled(
                format!("  {failed} ✗"),
                Style::new().fg(FAILED),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_tree(&mut self, frame: &mut Frame, area: Rect) {
        let selected = self.selection.selected();
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(row, (index, depth))| {
                let node = &self.nodes[*index];
                let (mark, colour) = match node.status {
                    SessionStatus::Done => ("✓", DONE),
                    SessionStatus::Failed => ("✗", FAILED),
                    SessionStatus::Running => ("◐", RUNNING),
                    SessionStatus::Pending => ("○", FAINT),
                };
                // Guides rather than blank indentation: at three levels deep a
                // tree of spaces stops reading as a tree at all.
                let guide = if *depth > 0 {
                    format!("{}└ ", "  ".repeat(depth - 1))
                } else {
                    String::new()
                };
                let title = if Some(row) == selected {
                    Style::new().fg(TEXT).bold()
                } else {
                    Style::new().fg(MUTED)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(guide, Style::new().fg(FAINT)),
                    Span::styled(format!("{mark} "), Style::new().fg(colour)),
                    Span::styled(node.title.clone(), title),
                ]))
            })
            .collect();

        frame.render_stateful_widget(
            List::new(items)
                .block(pane(" sessions ", false))
                // A left bar plus a brighter title, instead of inverting the
                // whole row: inversion fights every colour already in the row,
                // and the status marks are the point of the row.
                .highlight_symbol("▏")
                .highlight_style(Style::new().fg(ACCENT)),
            area,
            &mut self.selection,
        );
    }

    fn draw_detail(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(self.detail_lines())
                .wrap(Wrap { trim: false })
                .block(pane(" session ", false)),
            area,
        );
    }

    fn draw_input(&mut self, frame: &mut Frame, area: Rect) {
        let prompting = self.mode == Mode::Prompt;
        let title = if prompting {
            " new session · Enter to create · Esc to cancel "
        } else {
            " j/k move · i new · r reload · q quit "
        };
        self.input.set_block(pane(title, prompting));
        self.input.set_cursor_style(if prompting {
            Style::new().fg(BACKGROUND).bg(ACCENT)
        } else {
            // Hide the caret when the pane isn't focused, so there aren't
            // two things on screen claiming to be where you are.
            Style::new()
        });
        frame.render_widget(&self.input, area);
    }

    /// The right-hand pane: what this session is, what it concluded, and what
    /// it inherited.
    ///
    /// Built as styled lines rather than one string so labels can recede and
    /// content can lead. In a pane of flat text the eye reads the labels
    /// first, which are the least interesting thing on screen.
    fn detail_lines(&self) -> Text<'static> {
        let Some((index, _)) = self.selection.selected().and_then(|i| self.rows.get(i)) else {
            return Text::from(
                self.log
                    .iter()
                    .map(|line| Line::styled(line.clone(), Style::new().fg(MUTED)))
                    .collect::<Vec<_>>(),
            );
        };
        let node = &self.nodes[*index];

        let field = |name: &'static str, value: String| {
            Line::from(vec![
                Span::styled(format!("{name:<8}"), Style::new().fg(FAINT)),
                Span::styled(value, Style::new().fg(MUTED)),
            ])
        };
        let heading = |text: &'static str| Line::styled(text, Style::new().fg(ACCENT).bold());

        let status_colour = match node.status {
            SessionStatus::Done => DONE,
            SessionStatus::Failed => FAILED,
            SessionStatus::Running => RUNNING,
            SessionStatus::Pending => FAINT,
        };

        let mut lines = vec![
            Line::styled(node.title.clone(), Style::new().fg(TEXT).bold()),
            Line::from(vec![
                Span::styled(format!("{:<8}", "status"), Style::new().fg(FAINT)),
                Span::styled(
                    node.status.as_str().to_string(),
                    Style::new().fg(status_colour),
                ),
            ]),
            field("model", node.model.clone().unwrap_or_else(|| "—".into())),
            field("id", node.id.clone()),
        ];

        if let Some(summary) = &node.summary {
            lines.push(Line::raw(""));
            lines.push(heading("summary"));
            for line in summary.lines() {
                lines.push(Line::styled(line.to_string(), Style::new().fg(TEXT)));
            }
        }

        lines.push(Line::raw(""));
        match ContextBuilder::new(&self.store).build(&node.id) {
            Ok(ctx) if !ctx.is_empty() => {
                lines.push(heading("inherited"));
                for item in &ctx.items {
                    let colour = if item.outcome == SessionStatus::Failed {
                        FAILED
                    } else {
                        DONE
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("[{}] ", item.outcome.as_str()),
                            Style::new().fg(colour),
                        ),
                        Span::styled(item.title.clone(), Style::new().fg(TEXT)),
                    ]));
                    lines.push(Line::styled(
                        format!("  {}", item.text.trim()),
                        Style::new().fg(MUTED),
                    ));
                }
                if ctx.dropped > 0 {
                    lines.push(Line::styled(
                        format!(
                            "  ({} older session(s) dropped for context budget)",
                            ctx.dropped
                        ),
                        Style::new().fg(FAINT),
                    ));
                }
            }
            Ok(_) => lines.push(Line::styled(
                "inherited: nothing (root session)",
                Style::new().fg(FAINT),
            )),
            // Drawing must not fail on a store hiccup — show it in the pane.
            Err(e) => lines.push(Line::styled(
                format!("inherited: unavailable ({e})"),
                Style::new().fg(FAILED),
            )),
        }

        if !self.log.is_empty() {
            lines.push(Line::raw(""));
            for line in &self.log {
                lines.push(Line::styled(line.clone(), Style::new().fg(FAINT)));
            }
        }
        Text::from(lines)
    }
}

// A palette, rather than the terminal's sixteen.
//
// Indexed ANSI colours mean the app looks different in every terminal and
// fights whatever theme the user chose. These are fixed, low-saturation, and
// picked to sit on a dark background without any of them shouting: the only
// saturated colour is the accent, and it is spent on exactly two things —
// what is selected, and what a heading is.
const ACCENT: Color = Color::Rgb(0xE0, 0x7A, 0x5F);
const TEXT: Color = Color::Rgb(0xED, 0xF0, 0xF3);
const MUTED: Color = Color::Rgb(0x94, 0x9B, 0xA6);
const FAINT: Color = Color::Rgb(0x5A, 0x61, 0x6C);
const DONE: Color = Color::Rgb(0x7F, 0xB0, 0x8A);
const FAILED: Color = Color::Rgb(0xD1, 0x76, 0x76);
const RUNNING: Color = Color::Rgb(0xD6, 0xA8, 0x63);
const BACKGROUND: Color = Color::Rgb(0x0D, 0x0F, 0x12);

/// A pane border. Rounded, and dimmed unless it has focus — a screen of equally
/// bright boxes gives the eye nowhere to start.
fn pane(title: &'static str, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { FAINT }))
        .title(Span::styled(
            title,
            Style::new().fg(if focused { ACCENT } else { MUTED }),
        ))
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
