mod config;
mod gemini;
mod app;
mod history;
mod file_access;
mod markdown;
mod test_tui;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    io::stdout,
    time::Duration,
};
use anyhow::Result;
use app::ChatApp;
use config::Config;
use gemini::GeminiClient;
use history::HistoryManager;

#[tokio::main]
async fn main() -> Result<()> {
    // 環境変数でテストモードを確認
    if std::env::var("TEST_TUI").is_ok() {
        println!("Running TUI test mode...");
        if let Err(e) = test_tui::test_basic_tui() {
            eprintln!("TUI test failed: {}", e);
        }
        return Ok(());
    }
    
    // 最小限のアプリケーション
    if std::env::var("TEST_KEY").is_ok() {
        return test_key_input().await;
    }
    
    println!("Starting contui application...");
    
    // 設定を読み込む
    println!("Loading configuration...");
    let config = Config::load("token.toml")?;
    println!("Configuration loaded successfully");
    
    // 履歴管理を初期化
    println!("Initializing history manager...");
    let history_manager = HistoryManager::new()?;
    println!("History manager initialized");
    
    // Geminiクライアントを作成
    println!("Creating Gemini client...");
    let gemini_client = GeminiClient::new(config.llm);
    println!("Gemini client created");
    
    // アプリケーションを作成
    println!("Creating chat application...");
    let mut app = ChatApp::new(gemini_client, history_manager);
    println!("Chat application created");
    
    // ターミナルをセットアップ
    println!("Setting up terminal...");
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    println!("Terminal setup complete");

    let result = run_app(&mut terminal, &mut app).await;

    // ターミナルをクリーンアップ
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut ChatApp,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            app.render(f);
        })?;

        // イベントを非ブロッキングで処理
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key_event(key)? {
                        return Ok(());
                    }
                }
                Event::Resize(_, _) => {
                    // リサイズイベントを処理
                }
                _ => {}
            }
        }

        // チャットイベントを処理
        while let Ok(chat_event) = app.event_receiver.try_recv() {
            app.handle_chat_event(chat_event);
        }
    }
}

async fn test_key_input() -> Result<()> {
    println!("Starting key input test...");
    
    // ターミナルをセットアップ
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    println!("Terminal setup complete for key test");

    loop {
        // 画面を描画
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0)])
                .split(f.area());

            let text = "Key Test - Press 'q' to quit, any other key to test";
            let paragraph = Paragraph::new(text)
                .block(Block::default()
                    .title("Key Input Test")
                    .borders(Borders::ALL));

            f.render_widget(paragraph, chunks[0]);
        })?;

        // イベントを処理
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    println!("Key pressed: {:?}", key);
                    if let KeyCode::Char('q') = key.code {
                        break;
                    }
                }
                other => {
                    println!("Other event: {:?}", other);
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

    println!("Key test completed successfully");
    Ok(())
}
