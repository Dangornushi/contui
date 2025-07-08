use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io::stdout, time::Duration};

pub fn test_basic_tui() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting basic TUI test...");
    
    // ターミナルをセットアップ
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    println!("Terminal setup complete for test");

    let mut counter = 0;
    
    loop {
        // 画面を描画
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0)])
                .split(f.area());

            let text = format!("Hello TUI! Counter: {} (Press 'q' to quit)", counter);
            let paragraph = Paragraph::new(Span::styled(text, Style::default().fg(Color::White)))
                .block(Block::default()
                    .title("Test TUI")
                    .borders(Borders::ALL));

            f.render_widget(paragraph, chunks[0]);
        })?;

        // イベントを処理
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char(' ') => counter += 1,
                    _ => {}
                }
            }
        }
    }

    // クリーンアップ
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    println!("TUI test completed successfully");
    Ok(())
}
