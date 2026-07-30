//! The terminal client: a conversation, streamed, with the tools it ran.
//!
//! Rendering only. Every decision it displays was made in `cymose-core`, so
//! the VS Code panel shows the same thing without a second implementation of
//! what the agent did.
//!
//! The shape is a transcript rather than a tree browser. A coding agent is
//! something you talk to and watch: the answer arrives token by token, tools
//! announce themselves before they run, and refusals are visible rather than
//! silent. The session graph is still underneath — it is what a branch
//! inherits — but it isn't what you look at while working.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use anyhow::Result;
use cymose_core::agent::Toolbox;
use cymose_core::api::{Account, ApiMessage, Client};
use cymose_core::runner::{AgentEvent, Outcome, Turn};
use cymose_core::session::Role;
use cymose_core::{SessionStatus, Store};
// Crossterm comes from ratatui rather than as a direct dependency: tui-textarea
// takes events from ratatui's copy, and two crossterm versions in the tree make
// a key event from one unusable by the other.
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::DefaultTerminal;
use tui_textarea::TextArea;

/// Everything the terminal needs that it didn't build itself.
pub struct Context {
    pub store: Store,
    pub workspace: String,
    pub session_id: String,
    pub account: Account,
    pub client: Client,
    pub toolbox: Toolbox,
    pub model: String,
    /// The agent loop is async and the render loop is not. Turns are spawned
    /// onto this and report back over a channel — polling a future from inside
    /// a draw call would block the frame that is supposed to be showing it.
    pub runtime: tokio::runtime::Handle,
}

pub fn run(ctx: Context) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = App::new(ctx).run(&mut terminal);
    // Restore before propagating: an error printed into raw mode is unreadable.
    ratatui::restore();
    result
}

/// One block in the transcript.
enum Entry {
    User(String),
    /// Grows as deltas arrive, which is why it is one entry and not many.
    Assistant(String),
    Tool {
        name: String,
        detail: String,
        state: ToolState,
    },
    /// Ours, not the conversation's — errors, limits, the opening line.
    Note(String),
}

enum ToolState {
    Running,
    Done { truncated: bool },
    Refused(String),
}

/// What the worker sends back: `AgentEvent`, plus the two ways a turn ends.
enum Msg {
    Event(AgentEvent),
    Done(Box<Outcome>),
    Failed(String),
}

struct App {
    ctx: Context,
    entries: Vec<Entry>,
    /// The conversation as the model will be shown it next time.
    history: Vec<ApiMessage>,
    input: TextArea<'static>,
    /// Some while a turn is in flight; the input is disabled and this is drained.
    inbox: Option<Receiver<Msg>>,
    /// Lines scrolled up from the bottom. 0 pins to the newest, which is where
    /// it stays unless the user deliberately goes looking.
    scrollback: u16,
    spinner: usize,
    quitting: bool,
}

const SPINNER: [&str; 4] = ["·", "•", "●", "•"];
const PLACEHOLDER: &str = "Ask for a change. Enter to send, Shift+Enter for a new line.";

impl App {
    fn new(ctx: Context) -> Self {
        let mut input = TextArea::default();
        input.set_placeholder_text(PLACEHOLDER);
        input.set_cursor_line_style(Style::default());

        let entries = vec![Entry::Note(format!(
            "{} · {} plan · workspace {}",
            ctx.model,
            ctx.account.plan_label(),
            short(&ctx.workspace),
        ))];

        App {
            ctx,
            entries,
            history: Vec::new(),
            input,
            inbox: None,
            scrollback: 0,
            spinner: 0,
            quitting: false,
        }
    }

    fn busy(&self) -> bool {
        self.inbox.is_some()
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quitting {
            terminal.draw(|frame| self.draw(frame))?;
            self.drain();
            // Short enough that a streamed token looks continuous, long enough
            // that an idle terminal isn't spinning a core.
            if event::poll(Duration::from_millis(60))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key.code, key.modifiers);
                    }
                }
            }
            self.spinner = self.spinner.wrapping_add(1);
        }
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        match code {
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quitting = true,
            KeyCode::Esc if !self.busy() => self.quitting = true,
            KeyCode::PageUp => self.scrollback = self.scrollback.saturating_add(8),
            KeyCode::PageDown => self.scrollback = self.scrollback.saturating_sub(8),
            KeyCode::Enter if !mods.contains(KeyModifiers::SHIFT) && !self.busy() => self.send(),
            // Everything else while a turn runs would edit a box that is about
            // to be replaced.
            _ if self.busy() => {}
            other => {
                self.input.input(KeyEvent::new(other, mods));
            }
        }
    }

    /// Hands the prompt to the agent loop and starts listening.
    fn send(&mut self) {
        let prompt = self.input.lines().join("\n").trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.input = TextArea::default();
        self.input.set_placeholder_text("Working…");
        self.input.set_cursor_line_style(Style::default());

        self.entries.push(Entry::User(prompt.clone()));
        self.history.push(ApiMessage::text("user", &prompt));
        self.scrollback = 0;

        // The transcript belongs in the store, not only on screen. A session
        // is a node in the graph, and what a branch inherits is a summary of
        // these messages — a terminal that kept them in memory would leave
        // every branch opened from it inheriting nothing.
        self.record(Role::User, &prompt, 0, 0);
        let _ = self
            .ctx
            .store
            .set_status(&self.ctx.session_id, SessionStatus::Running);

        let (tx, rx) = mpsc::channel();
        self.inbox = Some(rx);

        // Cloned rather than borrowed: the turn outlives this call, and the
        // App has to stay usable for drawing while it runs. History is sent
        // without the prompt — the runner appends that itself.
        let client = self.ctx.client.clone();
        let toolbox = self.ctx.toolbox.clone();
        let session_id = self.ctx.session_id.clone();
        let model = self.ctx.model.clone();
        let history = self.history[..self.history.len() - 1].to_vec();
        let system = system_prompt();

        self.ctx.runtime.spawn(async move {
            let events = tx.clone();
            let turn = Turn {
                client: &client,
                toolbox: &toolbox,
                session_id: &session_id,
                model: &model,
                system: &system,
                history,
                prompt: &prompt,
            };
            let result = turn
                .run(|event| {
                    // A closed channel means the terminal is gone. The turn
                    // finishes anyway rather than half-applying an edit.
                    let _ = events.send(Msg::Event(event));
                })
                .await;
            let _ = match result {
                Ok(outcome) => tx.send(Msg::Done(Box::new(outcome))),
                Err(error) => tx.send(Msg::Failed(error.to_string())),
            };
        });
    }

    /// Takes whatever the worker has sent since the last frame.
    fn drain(&mut self) {
        let Some(rx) = &self.inbox else { return };

        let mut batch = Vec::new();
        let mut ended = None;
        loop {
            match rx.try_recv() {
                Ok(Msg::Event(event)) => batch.push(event),
                Ok(Msg::Done(outcome)) => {
                    ended = Some(Ok(*outcome));
                    break;
                }
                Ok(Msg::Failed(message)) => {
                    ended = Some(Err(message));
                    break;
                }
                Err(TryRecvError::Empty) => break,
                // The worker dropped without a verdict. Rare, and silence is
                // worse than saying so.
                Err(TryRecvError::Disconnected) => {
                    ended = Some(Err("the turn ended without a result".into()));
                    break;
                }
            }
        }

        for event in batch {
            self.apply(event);
        }
        match ended {
            Some(Ok(outcome)) => self.finish(outcome),
            Some(Err(message)) => {
                self.entries.push(Entry::Note(message));
                self.idle();
            }
            None => {}
        }
    }

    fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Text(delta) => match self.entries.last_mut() {
                Some(Entry::Assistant(text)) => text.push_str(&delta),
                _ => self.entries.push(Entry::Assistant(delta)),
            },
            AgentEvent::ToolStarted { name, detail } => self.entries.push(Entry::Tool {
                name,
                detail,
                state: ToolState::Running,
            }),
            AgentEvent::ToolFinished {
                name, truncated, ..
            } => self.close_tool(&name, ToolState::Done { truncated }),
            AgentEvent::ToolRefused { name, reason } => {
                self.close_tool(&name, ToolState::Refused(reason))
            }
            AgentEvent::ModelSwitched { from, to } => self
                .entries
                .push(Entry::Note(format!("{from} failed — trying {to}"))),
        }
    }

    /// Marks the most recent running call of that tool as finished.
    ///
    /// By name and from the end, because several calls can be in flight in one
    /// step and the last one started is the one being reported.
    fn close_tool(&mut self, name: &str, state: ToolState) {
        for entry in self.entries.iter_mut().rev() {
            if let Entry::Tool {
                name: n,
                state: slot @ ToolState::Running,
                ..
            } = entry
            {
                if n == name {
                    *slot = state;
                    return;
                }
            }
        }
    }

    fn finish(&mut self, outcome: Outcome) {
        if !outcome.text.trim().is_empty() {
            self.history
                .push(ApiMessage::text("assistant", &outcome.text));
            self.record(
                Role::Assistant,
                &outcome.text,
                outcome.tokens.input,
                outcome.tokens.output,
            );
        }
        let _ = self
            .ctx
            .store
            .set_status(&self.ctx.session_id, SessionStatus::Done);
        if outcome.hit_step_limit {
            self.entries.push(Entry::Note(
                "stopped at the step limit — the task isn't finished. Ask it to carry on.".into(),
            ));
        }
        if !outcome.files_touched.is_empty() {
            self.entries.push(Entry::Note(format!(
                "changed {}",
                outcome.files_touched.join(", ")
            )));
        }
        self.idle();
    }

    /// Writes one message to the store.
    ///
    /// A failure here is shown but doesn't stop the turn: losing the local
    /// record of an answer is bad, and throwing away the answer itself
    /// because of it is worse.
    fn record(&mut self, role: Role, content: &str, tokens_in: u32, tokens_out: u32) {
        if let Err(error) = self.ctx.store.append_message(
            &self.ctx.session_id,
            role,
            content,
            tokens_in,
            tokens_out,
        ) {
            self.entries.push(Entry::Note(format!(
                "couldn't save to the session: {error}"
            )));
        }
    }

    fn idle(&mut self) {
        self.inbox = None;
        self.input.set_placeholder_text(PLACEHOLDER);
    }

    // ---- drawing ----------------------------------------------------------

    fn draw(&mut self, frame: &mut Frame) {
        let input_height = (self.input.lines().len() as u16).clamp(1, 6) + 2;
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(input_height),
        ])
        .areas(frame.area());

        self.draw_header(frame, header);
        self.draw_transcript(frame, body);
        self.draw_input(frame, footer);
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let left = Line::from(vec![
            Span::styled(" cymose ", Style::new().fg(TEXT).bold()),
            Span::styled(short(&self.ctx.workspace), Style::new().fg(FAINT)),
        ]);
        let remaining = self
            .ctx
            .account
            .standard_cap
            .saturating_sub(self.ctx.account.standard_used);
        let right = Line::from(vec![
            Span::styled(self.ctx.model.clone(), Style::new().fg(MUTED)),
            Span::styled(
                format!("  {} · {remaining} left ", self.ctx.account.plan_label()),
                Style::new().fg(FAINT),
            ),
        ])
        .right_aligned();

        frame.render_widget(Paragraph::new(left), area);
        frame.render_widget(Paragraph::new(right), area);
    }

    fn draw_transcript(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::User(text) => {
                    lines.push(Line::from(""));
                    for (i, part) in text.lines().enumerate() {
                        lines.push(Line::from(vec![
                            Span::styled(if i == 0 { "› " } else { "  " }, Style::new().fg(TEXT)),
                            Span::styled(part.to_string(), Style::new().fg(TEXT).bold()),
                        ]));
                    }
                }
                Entry::Assistant(text) => {
                    lines.push(Line::from(""));
                    for part in text.lines() {
                        lines.push(Line::from(Span::styled(
                            part.to_string(),
                            Style::new().fg(TEXT),
                        )));
                    }
                }
                Entry::Tool {
                    name,
                    detail,
                    state,
                } => {
                    let (mark, colour) = match state {
                        ToolState::Running => {
                            (SPINNER[(self.spinner / 3) % SPINNER.len()], RUNNING)
                        }
                        ToolState::Done { truncated: false } => ("✓", DONE),
                        ToolState::Done { truncated: true } => ("✓", RUNNING),
                        ToolState::Refused(_) => ("✗", FAILED),
                    };
                    let mut spans = vec![
                        Span::styled(format!("  {mark} "), Style::new().fg(colour)),
                        Span::styled(name.clone(), Style::new().fg(MUTED)),
                        Span::styled(format!("  {detail}"), Style::new().fg(FAINT)),
                    ];
                    if let ToolState::Done { truncated: true } = state {
                        spans.push(Span::styled(" (truncated)", Style::new().fg(FAINT)));
                    }
                    lines.push(Line::from(spans));
                    // The reason goes on its own line: a refusal the user can't
                    // read is a refusal they report as the agent "doing
                    // nothing".
                    if let ToolState::Refused(reason) = state {
                        lines.push(Line::from(Span::styled(
                            format!("      {reason}"),
                            Style::new().fg(FAILED),
                        )));
                    }
                }
                Entry::Note(text) => lines.push(Line::from(Span::styled(
                    format!("  {text}"),
                    Style::new().fg(FAINT),
                ))),
            }
        }

        // Pin to the bottom unless the user has scrolled up. Counting wrapped
        // rows exactly would mean redoing the wrapper's arithmetic; this
        // overshoots and lets the widget clamp, which never hides the newest
        // line — the one that matters while a turn is streaming.
        let total = lines.len() as u16;
        let max_offset = total.saturating_sub(area.height);
        let offset = max_offset.saturating_sub(self.scrollback.min(max_offset));

        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((offset, 0)),
            area,
        );
    }

    fn draw_input(&mut self, frame: &mut Frame, area: Rect) {
        let (title, colour) = if self.busy() {
            (
                format!(" {} working ", SPINNER[(self.spinner / 3) % SPINNER.len()]),
                RUNNING,
            )
        } else {
            (" ask ".to_string(), FAINT)
        };
        self.input.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(colour))
                .title(Span::styled(title, Style::new().fg(colour))),
        );
        frame.render_widget(&self.input, area);
    }
}

/// The first eight characters of an id — enough to tell two apart without a
/// uuid across the header.
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn system_prompt() -> String {
    "You are Cymose Code, a coding agent working inside the user's project. \
     Use the tools to read before you change anything, and prefer small, \
     verifiable edits. When a tool is refused, say what you would have done \
     and carry on without it rather than repeating the call. Answer briefly: \
     the user is reading this in a terminal beside their code."
        .to_string()
}

// Fixed values rather than the terminal's ANSI slots, so the meaning of a
// colour doesn't change with somebody's theme — and no orange: the brand
// dropped it, and a client still using it looks like a different product.
const TEXT: Color = Color::Rgb(0xF2, 0xF4, 0xF6);
const MUTED: Color = Color::Rgb(0x9A, 0xA1, 0xAC);
const FAINT: Color = Color::Rgb(0x5A, 0x61, 0x6C);
const DONE: Color = Color::Rgb(0x7F, 0xB0, 0x8A);
const FAILED: Color = Color::Rgb(0xD1, 0x76, 0x76);
const RUNNING: Color = Color::Rgb(0x8A, 0xA6, 0xC0);
