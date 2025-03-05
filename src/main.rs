use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use std::{io, time::Duration};
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
use futures::future::join_all;
use tokio::time;

const STOCK_API_URL: &str = "https://api.twelvedata.com/time_series";
const STOCK_API_KEY: &str = "ab9e27fedd3d4c4bb83c314a03ce4cd1";
const STOCK_SYMBOLS: &[&str] = &[
    "AAPL",
    "EUR/USD",
    "ETH/BTC:Huobi",
    "TRP:TSX",
    "RHM.DE",
    "GOOG",
    "MSFT",
    "AMZN",
    "FB",
    "TSLA",
];
const NEWS_API_URL: &str = "https://api.marketaux.com/v1/news/all";
const NEWS_API_KEY: &str = "UIg3lYafKnwqxNHmYPc2h282hN9zmhdLrmkz7PJK";
const TECH_UPDATE_INTERVAL: usize = 10;

#[derive(Debug)]
struct TechnicalIndicators {
    sma50: Option<f64>,
    sma200: Option<f64>,
    rsi: Option<f64>,
    macd: Option<f64>,
    bb_upper: Option<f64>,
    bb_middle: Option<f64>,
    bb_lower: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = Client::new();
    let symbols: Vec<String> = STOCK_SYMBOLS.iter().map(|&s| s.to_string()).collect();
    let mut interval = time::interval(Duration::from_secs(30));
    let mut cycle_count = 0;
    let mut technical_data: Vec<(String, TechnicalIndicators)> = Vec::new();

    loop {
        interval.tick().await;
        cycle_count += 1;

        let stock_data = fetch_all_stock_data(&client, &symbols).await;
        let news_data = fetch_relevant_news(&client, &stock_data).await;

        if cycle_count % TECH_UPDATE_INTERVAL == 0 {
            technical_data = fetch_all_technical_data(&client, &symbols).await;
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Percentage(10),
                        Constraint::Percentage(40),
                        Constraint::Percentage(30),
                        Constraint::Percentage(20),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let header = Paragraph::new("Trading Data HUD")
                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).title("Header"));

            let stock_paragraph = Paragraph::new(format_stock_data(&stock_data))
                .block(Block::default().borders(Borders::ALL).title("Stocks"));

            let indicator_paragraph = Paragraph::new(format_indicator_data(&technical_data))
                .block(Block::default().borders(Borders::ALL).title("Technical Indicators"));

            let news_paragraph = Paragraph::new(format_news_data(&news_data))
                .block(Block::default().borders(Borders::ALL).title("News"));

            f.render_widget(header, chunks[0]);
            f.render_widget(stock_paragraph, chunks[1]);
            f.render_widget(indicator_paragraph, chunks[2]);
            f.render_widget(news_paragraph, chunks[3]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn fetch_all_stock_data(client: &Client, symbols: &[String]) -> Vec<(String, Value)> {
    let futures = symbols.iter().map(|symbol| {
        let symbol = symbol.clone();
        async move {
            match fetch_stock_data(client, &symbol).await {
                Ok(data) => Some((symbol, data)),
                Err(e) => {
                    eprintln!("Error fetching data for {}: {}", symbol, e);
                    None
                }
            }
        }
    });
    join_all(futures)
        .await
        .into_iter()
        .filter_map(|data| data)
        .collect()
}

async fn fetch_stock_data(client: &Client, symbol: &str) -> Result<Value> {
    let url = format!(
        "{}?symbol={}&interval=1h&apikey={}",
        STOCK_API_URL, symbol, STOCK_API_KEY
    );
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    Ok(json)
}

async fn fetch_relevant_news(client: &Client, stock_data: &[(String, Value)]) -> Vec<(String, Value)> {
    let mut news_results = Vec::new();

    for (symbol, data) in stock_data {
        if let Some(values) = data["values"].as_array() {
            if values.len() > 1 {
                let latest = &values[0];
                let previous = &values[1];
                let latest_price: f64 = latest["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                let previous_price: f64 = previous["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                if previous_price > 0.0 {
                    let percent_change = ((latest_price - previous_price) / previous_price) * 100.0;
                    if percent_change > 7.0 {
                        if let Ok(news) = fetch_stock_news(client, symbol).await {
                            news_results.push((symbol.clone(), news));
                        }
                    }
                }
            }
        }
    }
    news_results
}

async fn fetch_stock_news(client: &Client, symbol: &str) -> Result<Value> {
    let url = format!(
        "{}?symbols={}&api_token={}",
        NEWS_API_URL, symbol, NEWS_API_KEY
    );
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    Ok(json)
}

fn format_stock_data(stock_data: &[(String, Value)]) -> Vec<Spans> {
    let mut lines = Vec::new();
    for (symbol, data) in stock_data {
        if let Some(values) = data["values"].as_array() {
            if !values.is_empty() {
                let latest = &values[0];
                let latest_price: f64 = latest["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                let mut spans = vec![Span::styled(
                    format!("{}: {:.2}", symbol, latest_price),
                    Style::default().fg(Color::Green),
                )];

                if values.len() > 1 {
                    let previous = &values[1];
                    let previous_price: f64 = previous["close"].as_str().unwrap_or("0").parse().unwrap_or(0.0);
                    if previous_price > 0.0 {
                        let percent_change = ((latest_price - previous_price) / previous_price) * 100.0;
                        let change_text = if percent_change < -1.0 {
                            format!(" dropped {:.2}%", percent_change)
                        } else {
                            format!(" increased {:.2}%", percent_change)
                        };
                        let change_span = Span::styled(
                            change_text,
                            if percent_change < -1.0 {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default().fg(Color::Green)
                            },
                        );
                        spans.push(change_span);
                    }
                }
                lines.push(Spans::from(spans));
            }
        }
    }
    lines
}

fn format_news_data(news_data: &[(String, Value)]) -> Vec<Spans> {
    let mut lines = Vec::new();
    for (symbol, news) in news_data {
        if let Some(articles) = news["data"].as_array() {
            for article in articles {
                let title = article["title"].as_str().unwrap_or("No title");
                lines.push(Spans::from(vec![Span::styled(
                    format!("{}: {}", symbol, title),
                    Style::default().fg(Color::Blue),
                )]));
            }
        }
    }
    lines
}

async fn fetch_all_technical_data(client: &Client, symbols: &[String]) -> Vec<(String, TechnicalIndicators)> {
    let futures = symbols.iter().map(|symbol| {
        let symbol = symbol.clone();
        async move {
            match fetch_technical_indicators(client, &symbol).await {
                Ok(indicators) => Some((symbol, indicators)),
                Err(e) => {
                    eprintln!("Error fetching technical data for {}: {}", symbol, e);
                    None
                }
            }
        }
    });
    join_all(futures)
        .await
        .into_iter()
        .filter_map(|data| data)
        .collect()
}

async fn fetch_technical_indicators(client: &Client, symbol: &str) -> Result<TechnicalIndicators> {
    let sma50 = fetch_indicator_value(client, symbol, "sma", "daily", 50).await?;
    let sma200 = fetch_indicator_value(client, symbol, "sma", "daily", 200).await?;
    let rsi = fetch_indicator_value(client, symbol, "rsi", "daily", 14).await?;
    let macd = fetch_indicator_value(client, symbol, "macd", "daily", 12).await?;
    let (bb_upper, bb_middle, bb_lower) = fetch_bbands(client, symbol, "daily", 20).await?;
    Ok(TechnicalIndicators {
        sma50,
        sma200,
        rsi,
        macd,
        bb_upper,
        bb_middle,
        bb_lower,
    })
}

async fn fetch_indicator_value(
    client: &Client,
    symbol: &str,
    indicator: &str,
    interval: &str,
    time_period: i32,
) -> Result<Option<f64>> {
    let url = format!(
        "https://api.twelvedata.com/technical_indicator?symbol={}&interval={}&indicator={}&time_period={}&apikey={}",
        symbol, interval, indicator, time_period, STOCK_API_KEY
    );
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    if let Some(values) = json["values"].as_array() {
        if let Some(latest) = values.first() {
            if let Some(value_str) = latest[indicator].as_str() {
                return Ok(Some(value_str.parse().unwrap_or(0.0)));
            }
        }
    }
    Ok(None)
}

async fn fetch_bbands(
    client: &Client,
    symbol: &str,
    interval: &str,
    time_period: i32,
) -> Result<(Option<f64>, Option<f64>, Option<f64>)> {
    let url = format!(
        "https://api.twelvedata.com/technical_indicator?symbol={}&interval={}&indicator=bbands&time_period={}&apikey={}",
        symbol, interval, time_period, STOCK_API_KEY
    );
    let response = client.get(&url).send().await?;
    let json: Value = response.json().await?;
    if let Some(values) = json["values"].as_array() {
        if let Some(latest) = values.first() {
            let upper = latest["real_upper_band"].as_str().and_then(|s| s.parse().ok());
            let middle = latest["real_middle_band"].as_str().and_then(|s| s.parse().ok());
            let lower = latest["real_lower_band"].as_str().and_then(|s| s.parse().ok());
            return Ok((upper, middle, lower));
        }
    }
    Ok((None, None, None))
}

fn format_indicator_data(technical_data: &[(String, TechnicalIndicators)]) -> Vec<Spans> {
    let mut lines = Vec::new();
    for (symbol, indicators) in technical_data {
        let line = format!(
            "{} | SMA50: {:.2?} | SMA200: {:.2?} | RSI: {:.2?} | MACD: {:.2?} | BB: [{:.2?}, {:.2?}, {:.2?}]",
            symbol,
            indicators.sma50.unwrap_or(0.0),
            indicators.sma200.unwrap_or(0.0),
            indicators.rsi.unwrap_or(0.0),
            indicators.macd.unwrap_or(0.0),
            indicators.bb_upper.unwrap_or(0.0),
            indicators.bb_middle.unwrap_or(0.0),
            indicators.bb_lower.unwrap_or(0.0)
        );
        lines.push(Spans::from(Span::styled(line, Style::default().fg(Color::Magenta))));
    }
    lines
}
