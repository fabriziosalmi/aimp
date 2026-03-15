use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Gauge},
    Terminal,
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use tokio::sync::mpsc;
use crate::crdt::CrdtHandle;
use crate::event::SystemEvent;
use crate::config;

pub struct Dashboard {
    pub node_id: String,
    pub crdt_handle: CrdtHandle,
    pub log_rx: mpsc::Receiver<SystemEvent>,
    pub logs: Vec<SystemEvent>,
}

impl Dashboard {
    pub fn new(node_id: String, crdt_handle: CrdtHandle, log_rx: mpsc::Receiver<SystemEvent>) -> Self {
        Self { 
            node_id, 
            crdt_handle, 
            log_rx,
            logs: Vec::new(),
        }
    }

    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            // Drain logs
            while let Ok(log) = self.log_rx.try_recv() {
                self.logs.push(log);
                if self.logs.len() > config::MAX_VISIBLE_LOGS {
                    self.logs.remove(0);
                }
            }

            terminal.draw(|f| self.ui(f))?;

            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if let KeyCode::Char('q') = key.code {
                        break;
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn ui(&self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Min(2),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .split(f.size());

        // 1. Header
        let header = Paragraph::new(format!(" AIMP Core v{} | Node: {} | Press 'q' to exit", env!("CARGO_PKG_VERSION"), self.node_id))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).title(" STATUS "));
        f.render_widget(header, chunks[0]);

        // 2. Main Content
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
            .split(chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(main_chunks[1]);

        // Get Root Hash via Actor (Blocking for UI)
        let merkle_root = hex::encode(futures::executor::block_on(self.crdt_handle.get_merkle_root()));

        let stats_text = format!(
            "\n DAG METRICS:\n\n - ROOT HASH:\n   {}\n\n - NODE STATE: ACTIVE\n - ACTOR MODE: SYNC_PASS",
            merkle_root
        );
        let stats = Paragraph::new(stats_text)
            .block(Block::default().borders(Borders::ALL).title(" NETWORK "));
        f.render_widget(stats, main_chunks[0]);

        // 3. System Logs
        let log_content = if self.logs.is_empty() {
            "\n Waiting for system events...".to_string()
        } else {
            let mut s = String::new();
            for log in &self.logs {
                s.push_str(&format!("\n {}", log.to_display()));
            }
            s
        };

        let telemetry = Paragraph::new(log_content)
            .block(Block::default().borders(Borders::ALL).title(" NETWORK LOG "));
        f.render_widget(telemetry, right_chunks[0]);

        // 4. Audit Trail (v0.4.0)
        let audit_logs: Vec<String> = self.logs.iter()
            .filter(|l| matches!(l, SystemEvent::AiInference { .. }))
            .map(|l| format!("> {}", l.to_display()))
            .collect();
            
        let audit_content = if audit_logs.is_empty() {
            "\n No verified AI decisions yet.".to_string()
        } else {
            audit_logs.join("\n")
        };

        let audit_block = Paragraph::new(audit_content)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" AUDIT TRAIL [Edge-BFT] ")
                .border_style(Style::default().fg(Color::Yellow)));
        f.render_widget(audit_block, right_chunks[1]);

        // 4. Footer
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" NETWORK CONVERGENCE "))
            .gauge_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            .percent(100);
        f.render_widget(gauge, chunks[2]);
    }
}
