use crate::config::Config;
use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
        ExecutableCommand,
    },
    layout::{Constraint, Direction, Layout, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, List},
    Terminal,
};
use std::io;

pub async fn run() -> Result<Config> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(Clear(ClearType::All))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut config = Config::default();
    let mut current_step = 0;
    let steps = vec!["Welcome", "Hardware", "LLM Setup", "Channels", "Security", "Complete"];

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(f.area());

            // Title
            let title = Paragraph::new("🦐 AetherClaw Initialization Wizard")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).border_style(Color::Blue));
            f.render_widget(title, chunks[0]);

            // Progress
            let progress = Paragraph::new(format!("Step {}/{}: {}", current_step + 1, steps.len(), steps[current_step]))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(progress, chunks[2]);

            // Main content based on step
            match current_step {
                0 => render_welcome(f, chunks[1]),
                1 => render_hardware(f, chunks[1]),
                2 => render_llm(f, chunks[1]),
                _ => render_complete(f, chunks[1]),
            }
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Right | KeyCode::Enter => {
                    if current_step < steps.len() - 1 {
                        current_step += 1;
                    } else {
                        config.save()?;
                        break;
                    }
                }
                KeyCode::Left => {
                    if current_step > 0 {
                        current_step -= 1;
                    }
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    Ok(config)
}

fn render_welcome(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = Text::from(vec![
        Line::from(""),
        Line::from("Welcome to AetherClaw — The Ultra-Lean Edge AI"),
        Line::from(""),
        Line::from("• Target: <5MB RAM, <500ms boot"),
        Line::from("• Local-first with cloud fallback"),
        Line::from("• Secure sandboxed execution"),
        Line::from(""),
        Line::from("Press → or Enter to continue..."),
    ]);
    let para = Paragraph::new(text).alignment(Alignment::Center);
    f.render_widget(para, area);
}

fn render_hardware(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let items = [
        "RISC-V ($10 LicheeRV-Nano)",
        "ARM64 (Raspberry Pi / Android)",
        "x86_64 (Desktop/Server)",
    ];
    let list = List::new(items)
        .block(Block::default().title("Select Hardware Profile").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(list, area);
}

fn render_llm(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = Text::from(vec![
        Line::from("LLM Configuration"),
        Line::from(""),
        Line::from("Local models path: ~/.aetherclaw/models/"),
        Line::from("Default: phi-2-q4.gguf (1.6GB download)"),
        Line::from(""),
        Line::from("Cloud fallback (optional):"),
        Line::from("OpenRouter / OpenAI / Anthropic"),
    ]);
    f.render_widget(Paragraph::new(text).block(Block::default().borders(Borders::ALL)), area);
}

fn render_complete(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = Text::from(vec![
        Line::from("✓ Configuration complete!"),
        Line::from(""),
        Line::from("Run 'aetherclaw agent' to start"),
        Line::from("Or 'aetherclaw gateway' for chat channels"),
        Line::from(""),
        Line::from("皮皮虾，我们走！🚀"),
    ]);
    f.render_widget(Paragraph::new(text).alignment(Alignment::Center), area);
}
