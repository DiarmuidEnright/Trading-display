use reqwest::Client;
use serde_json::Value;
use std::error::Error;
use std::io;
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tokio::time::{self, Duration};

const STOCK_API_URL: &str = "https://api.twelvedata.com/time_series";
const STOCK_API_KEY: &str = "ab9e27fedd3d4c4bb83c314a03ce4cd1";
const STOCK_SYMBOLS: &[&str] = &["AAPL", "EUR/USD", "ETH/BTC:Huobi", "TRP:TSX", "RHM.DE"];
const NEWS_API_URL: &str = "https://api.marketaux.com/v1/news/all";
const NEWS_API_KEY: &str = "UIg3lYafKnwqxNHmYPc2h282hN9zmhdLrmkz7PJK";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();
    let top_movers = fetch_top_movers().await;

    let mut stock_data = Vec::new();
    let mut interval = time::interval(Duration::from_secs(30));

    // Main loop
    loop {
        interval.tick().await;
        stock_data = fetch_all_stock_data(&client, &top_movers).await;

        let mut news_data = Vec::new();
        for (symbol, data) in &stock_data {
            if let Some(values) = data["values"].as_array() {
                if values.len() > 1 {
                    let latest = &values[0];
                    let previous = &values[1];
                    let latest_price: f64 = latest["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                    let previous_price: f64 = previous["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                    let percent_change = ((latest_price - previous_price) / previous_price) * 100.0;

                    if percent_change > 7.0 {
                        if let Ok(news) = fetch_stock_news(&client, symbol).await {
                            news_data.push((symbol.clone(), news));
                        }
                    }
                }
            }
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Percentage(10),
                        Constraint::Percentage(70),
                        Constraint::Percentage(20),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let title = Paragraph::new("Stock Data")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).title("Title"));

            let mut text = Vec::new();
            let mut news_text = Vec::new();

            for (symbol, data) in &stock_data {
                if let Some(values) = data["values"].as_array() {
                    if !values.is_empty() {
                        let latest = &values[0];
                        let latest_price: f64 = latest["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                        let mut line = vec![Span::styled(
                            format!("{}: {:.2}", symbol, latest_price),
                            Style::default().fg(Color::Green),
                        )];

                        if values.len() > 1 {
                            let previous = &values[1];
                            let previous_price: f64 = previous["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                            let percent_change = ((latest_price - previous_price) / previous_price) * 100.0;

                            let change_span = if percent_change < -1.0 {
                                Span::styled(
                                    format!(" dropped {:.2}%", percent_change),
                                    Style::default().fg(Color::Red),
                                )
                            } else {
                                Span::styled(
                                    format!(" increased {:.2}%", percent_change),
                                    Style::default().fg(Color::Green),
                                )
                            };
                            line.push(change_span);
                        }
                        text.push(Spans::from(line));
                    }
                }
            }

            for (symbol, news) in &news_data {
                for article in news["data"].as_array().unwrap_or(&vec![]) {
                    let title = article["title"].as_str().unwrap_or("No title");
                    news_text.push(Spans::from(vec![Span::styled(
                        format!("{}: {}", symbol, title),
                        Style::default().fg(Color::Blue),
                    )]));
                }
            }

            let stock_paragraph = Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title("Stocks"));

            let news_paragraph = Paragraph::new(news_text)
                .block(Block::default().borders(Borders::ALL).title("News"));

            f.render_widget(title, chunks[0]);
            f.render_widget(stock_paragraph, chunks[1]);
            f.render_widget(news_paragraph, chunks[2]);
        })?;

        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

async fn fetch_top_movers() -> Vec<String> {
    // Use predefined stock symbols
    let symbols: Vec<String> = STOCK_SYMBOLS.iter().map(|&s| s.to_string()).collect();
    symbols
}

async fn fetch_stock_data(client: &Client, symbol: &str) -> Result<Value, Box<dyn Error>> {
    let url = format!("{}?symbol={}&interval=1h&apikey={}", STOCK_API_URL, symbol, STOCK_API_KEY);
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    Ok(json)
}

async fn fetch_all_stock_data(client: &Client, symbols: &[String]) -> Vec<(String, Value)> {
    let mut stock_data = Vec::new();
    for symbol in symbols {
        match fetch_stock_data(client, symbol).await {
            Ok(data) => stock_data.push((symbol.clone(), data)),
            Err(e) => eprintln!("Error fetching stock data: {}", e),
        }
    }
    stock_data
}

async fn fetch_stock_news(client: &Client, symbol: &str) -> Result<Value, Box<dyn Error>> {
    let url = format!("{}?symbols={}&api_token={}", NEWS_API_URL, symbol, NEWS_API_KEY);
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    Ok(json)
}
