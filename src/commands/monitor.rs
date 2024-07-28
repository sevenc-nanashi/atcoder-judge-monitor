use crate::{info, store};
use std::{io::Write, str::FromStr};
use termion::{raw::IntoRawMode, screen::IntoAlternateScreen};
use tokio::{io::AsyncReadExt, sync::Mutex};

static SUBMISSIONS: std::sync::LazyLock<Mutex<indexmap::IndexMap<u64, Submission>>> =
    std::sync::LazyLock::new(|| Mutex::new(indexmap::IndexMap::new()));
static STOPPED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Message {
    time: std::time::SystemTime,
    kind: MessageKind,
    message: String,
}

enum MessageKind {
    Error,
    Info,
    Warning,
}

static MESSAGE: std::sync::LazyLock<Mutex<Option<Message>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

async fn message(kind: MessageKind, message: String) {
    let time = std::time::SystemTime::now();
    let log = Message {
        time,
        kind,
        message,
    };
    let mut locked = MESSAGE.lock().await;
    *locked = Some(log);
}

pub async fn main(contest_url: String) -> anyhow::Result<()> {
    let client = store::create_http_client()
        .ok_or_else(|| anyhow::anyhow!("Failed to create agent, have you logged in?"))?;

    let title = get_title(&client, &contest_url).await?;

    info!("Monitoring contest {}", contest_url);

    let polling_thread = {
        let contest_url = contest_url.clone();
        tokio::spawn(async move { poll(client, &contest_url).await })
    };

    let screen_thread = tokio::spawn(async move { screen_loop(title).await });

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
    });

    let quit_thread = {
        tokio::spawn(async move {
            let mut stdin = tokio::io::stdin();
            while STOPPED.get().is_none() {
                let Ok(k) =
                    tokio::time::timeout(std::time::Duration::from_millis(100), stdin.read_u8())
                        .await
                else {
                    continue;
                };
                let k = k?;
                if k == b'q' || k == 3 {
                    break;
                }
                if k == b'p' {
                    PAUSED.store(
                        !PAUSED.load(std::sync::atomic::Ordering::Relaxed),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                if k.is_ascii_digit() {
                    let index = k - b'0';
                    let submissions = {
                        let locked = SUBMISSIONS.lock().await;
                        locked.clone()
                    };
                    let index =
                        submissions.len() as i32 - (if index == 0 { 10 } else { index as _ });
                    if index < 0 {
                        message(MessageKind::Warning, "Invalid index".to_string()).await;
                    }
                    if let Some((_id, submission)) = submissions.get_index(index as usize) {
                        let url = &submission.detail;
                        if let Err(err) = open::that_detached(url) {
                            message(MessageKind::Error, format!("Failed to open URL: {}", err))
                                .await;
                        } else {
                            message(
                                MessageKind::Info,
                                format!("Opening submission detail: {}", url),
                            )
                            .await;
                        }
                    }
                }
            }

            Ok(()) as anyhow::Result<()>
        })
    };

    print!("{}", termion::cursor::Hide);
    let result = {
        let threads = vec![polling_thread, screen_thread, quit_thread];
        let (finished, _, remaining) = futures::future::select_all(threads).await;

        STOPPED.get_or_init(|| ());

        futures::future::join_all(remaining).await;

        finished?
    };
    print!("{}", termion::cursor::Show);

    std::io::stdout().flush()?;

    if result.is_ok() {
        info!("Goodbye!");
    }

    result
}

#[cfg(feature = "dummy-submissions")]
async fn poll(client: reqwest::Client, contest_url: &str) -> anyhow::Result<()> {
    let mut counter = 0;
    let first_time = chrono::Utc::now();
    while STOPPED.get().is_none() {
        counter += 1;
        let mut submissions: indexmap::IndexMap<u64, Submission> = indexmap::IndexMap::new();
        for i in 0..100 {
            let time = first_time + chrono::Duration::seconds(i as _);
            let problem = format!("Problem {}", i);
            let language = "Rust".to_string();
            let score = (i * 100) as usize;
            let code_size = "1 KB".to_string();
            let status = match (i + counter) % 11 {
                0 => SubmissionStatus::Accepted,
                1 => SubmissionStatus::WaitingJudge,
                2 => SubmissionStatus::Judging,
                3 => SubmissionStatus::WaitingRejudge,
                4 => SubmissionStatus::WrongAnswer,
                5 => SubmissionStatus::TimeLimitExceeded,
                6 => SubmissionStatus::MemoryLimitExceeded,
                7 => SubmissionStatus::RuntimeError,
                8 => SubmissionStatus::CompileError,
                9 => SubmissionStatus::OutputLimitExceeded,
                10 => SubmissionStatus::InternalError,
                _ => unreachable!(),
            };
            let execution_time = if i % 2 == 0 {
                Some("100ms".to_string())
            } else {
                None
            };
            let memory = if i % 2 == 0 {
                Some("100MB".to_string())
            } else {
                None
            };
            let detail = "https://example.com".to_string();
            let submission = Submission {
                time,
                problem,
                language,
                score,
                code_size,
                status,
                execution_time,
                memory,
                detail,
            };
            submissions.insert(i as _, submission);
        }

        {
            let mut locked = SUBMISSIONS.lock().await;
            for (id, submission) in submissions {
                locked.insert(id, submission);
            }
        }

        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if STOPPED.get().is_some() {
                break;
            }
        }
        while PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if STOPPED.get().is_some() {
                break;
            }
        }
    }

    Ok(())
}
#[cfg(not(feature = "dummy-submissions"))]
async fn poll(client: reqwest::Client, contest_url: &str) -> anyhow::Result<()> {
    while STOPPED.get().is_none() {
        let submissions_html = client
            .get(format!("{}/submissions/me", contest_url))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let mut submissions = indexmap::IndexMap::new();

        {
            let html = scraper::Html::parse_document(&submissions_html);
            let rows_selector = scraper::Selector::parse("tbody tr").unwrap();
            let rows = html.select(&rows_selector).collect::<Vec<_>>();
            let rows = rows.iter().rev();
            let td_selector = scraper::Selector::parse("td").unwrap();
            for row in rows {
                let mut cells = row.select(&td_selector);

                let time = cells.next().unwrap();
                let problem = cells.next().unwrap();
                let _user = cells.next().unwrap();
                let lang = cells.next().unwrap();
                let score = cells.next().unwrap();
                let code_size = cells.next().unwrap();

                let status_elem = cells.next().unwrap();
                let status_text = status_elem.text().collect::<String>();
                let status_text = status_text.trim().split(' ').last().unwrap();
                let status = if status_text.contains('/') {
                    SubmissionStatus::Judging
                } else {
                    SubmissionStatus::from_str(status_text).unwrap()
                };
                let (execution_time, memory) = if status_elem.attr("colspan") == Some("3") {
                    (None, None)
                } else {
                    let execution_time = cells.next().unwrap();
                    let memory = cells.next().unwrap();
                    (
                        Some(execution_time.text().collect::<String>()),
                        Some(memory.text().collect::<String>()),
                    )
                };
                let detail = cells.next().unwrap();
                let detail = detail.child_elements().next().unwrap();
                let detail = format!("https://atcoder.jp{}", detail.value().attr("href").unwrap());

                let id: u64 = score.value().attr("data-id").unwrap().parse().unwrap();
                let time = chrono::DateTime::parse_from_str(
                    &time.text().collect::<String>(),
                    "%Y-%m-%d %H:%M:%S%z",
                )
                .unwrap()
                .with_timezone(&chrono::Utc);

                let submission = Submission {
                    time,
                    problem: problem.text().collect(),
                    language: lang.text().collect(),
                    score: score.text().collect::<String>().parse().unwrap(),
                    code_size: code_size.text().collect(),
                    status,
                    execution_time,
                    memory,
                    detail,
                };

                submissions.insert(id, submission);
            }
        }

        {
            let mut locked = SUBMISSIONS.lock().await;
            for (id, submission) in submissions {
                locked.insert(id, submission);
            }
        }
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if STOPPED.get().is_some() {
                break;
            }
        }
        while PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if STOPPED.get().is_some() {
                break;
            }
        }
    }

    Ok(())
}

async fn screen_loop(title: String) -> anyhow::Result<()> {
    let mut i = 0;

    let mut screen = std::io::stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    let mut last_size = 0;
    let mut exit_warned = false;
    let mut last_update = std::time::Instant::now();
    let mut prev_status = std::collections::HashMap::new();
    let mut update_time = std::collections::HashMap::new();
    while STOPPED.get().is_none() {
        let (terminal_width, terminal_height) = termion::terminal_size()?;
        i += 1;
        print!("{}", termion::clear::All);

        let title = format!("{}{}{}", termion::style::Bold, title, termion::style::Reset);

        print!(
            "{}{}",
            termion::cursor::Goto(1, 1),
            crate::log::strip_ansi_codes(&console::truncate_str(
                &title,
                terminal_width as usize - 1,
                "..."
            )),
        );
        print!("{}", termion::cursor::Goto(1, terminal_height));

        let error_message = {
            let locked = MESSAGE.lock().await;
            match &*locked {
                Some(log) => {
                    if log.time.elapsed().unwrap() < std::time::Duration::from_secs(1) {
                        Some(format!(
                            "{}{}: {}{}",
                            match log.kind {
                                MessageKind::Error =>
                                    termion::color::Fg(termion::color::Red).to_string(),
                                MessageKind::Info =>
                                    termion::color::Fg(termion::color::Blue).to_string(),
                                MessageKind::Warning =>
                                    termion::color::Fg(termion::color::Yellow).to_string(),
                            },
                            match log.kind {
                                MessageKind::Error => "Error",
                                MessageKind::Info => "Info",
                                MessageKind::Warning => "Warning",
                            },
                            log.message,
                            termion::color::Fg(termion::color::Reset)
                        ))
                    } else {
                        None
                    }
                }
                None => None,
            }
        };

        let footer_message = if let Some(message) = error_message {
            message
        } else if PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
            "  Paused | {{p}} to resume, {{q}} to quit, {{0-9}} to open submission detail"
                .to_string()
        } else {
            format!(
                "{} Running | {{p}} to pause, {{q}} to quit, {{0-9}} to open submission detail",
                match i % 4 {
                    0 => "|",
                    1 => "/",
                    2 => "-",
                    3 => "\\",
                    _ => unreachable!(),
                },
            )
        }
        .replace("{", &format!("{}", termion::style::Bold))
        .replace(
            "}",
            &format!(
                "{}{}",
                termion::style::Reset,
                termion::color::Fg(termion::color::LightBlack)
            ),
        );
        print!(
            "{}",
            crate::log::strip_ansi_codes(&format!(
                "{}{}",
                termion::color::Fg(termion::color::LightBlack),
                console::truncate_str(&footer_message, terminal_width as usize - 1, "...")
            ))
        );
        let submissions = {
            let locked = SUBMISSIONS.lock().await;
            locked.clone()
        };
        if last_size != submissions.len() {
            last_size = submissions.len();
            exit_warned = false;
            last_update = std::time::Instant::now();
        }

        for i in 0..(terminal_height - 2) {
            let index = submissions.len() as i32 - i as i32 - 1;
            if index < 0 {
                break;
            }
            let Some((id, submission)) = submissions.get_index(index as usize) else {
                break;
            };

            print!(
                "{}{}",
                termion::cursor::Goto(1, terminal_height - i - 1),
                crate::log::strip_ansi_codes(termion::style::Reset.as_ref())
            );

            let mut sections = vec![];

            if i < 10 {
                let key = ((i + 1) % 10).to_string();

                sections.push(format!(
                    "[{}{}{}] ",
                    termion::style::Bold,
                    key,
                    termion::style::Reset
                ));
            } else {
                sections.push("    ".to_string());
            }

            match submission.status {
                SubmissionStatus::Accepted => {
                    sections.push(termion::color::Fg(termion::color::Green).to_string());
                }
                SubmissionStatus::WaitingJudge
                | SubmissionStatus::Judging
                | SubmissionStatus::WaitingRejudge => {
                    sections.push(termion::color::Fg(termion::color::LightBlack).to_string());
                }
                SubmissionStatus::WrongAnswer
                | SubmissionStatus::TimeLimitExceeded
                | SubmissionStatus::MemoryLimitExceeded
                | SubmissionStatus::RuntimeError
                | SubmissionStatus::CompileError
                | SubmissionStatus::OutputLimitExceeded => {
                    sections.push(termion::color::Fg(termion::color::Yellow).to_string());
                }
                SubmissionStatus::InternalError => {
                    sections.push(termion::color::Fg(termion::color::Red).to_string());
                }
            };
            match prev_status.get(id) {
                Some(prev_status) if prev_status != &submission.status => {
                    update_time.insert(*id, std::time::Instant::now());
                }
                _ => {}
            }
            prev_status.insert(*id, submission.status);

            let is_updated_recently = match update_time.get(id) {
                Some(update_time) => update_time.elapsed() < std::time::Duration::from_secs(5),
                None => false,
            };
            let global_style = if matches!(
                submission.status,
                SubmissionStatus::WaitingJudge
                    | SubmissionStatus::Judging
                    | SubmissionStatus::WaitingRejudge
            ) {
                termion::color::Fg(termion::color::LightBlack).to_string()
            } else if is_updated_recently {
                termion::style::Bold.to_string()
            } else {
                "".to_string()
            };

            // print!("{:>3}", submission.status.to_string());
            // print!("{}", termion::color::Fg(termion::color::Reset));
            // print!(": ");
            sections.push(format!(
                "{:>3}: {}{}",
                submission.status.to_string(),
                termion::style::Reset,
                global_style
            ));

            let local_time = submission.time.with_timezone(&chrono::Local);
            sections.push(local_time.format("%Y-%m-%d %H:%M:%S").to_string());

            sections.push(" | ".to_string());
            sections.push(format!(
                "{:<30}",
                console::truncate_str(&submission.problem, 30, "...")
            ));
            sections.push(" | ".to_string());
            sections.push(format!("{:>4}pts", submission.score));
            if let Some(execution_time) = &submission.execution_time {
                sections.push(" | ".to_string());
                sections.push(format!("{:>10}", execution_time));
            }

            print!(
                "{}",
                console::truncate_str(
                    &crate::log::strip_ansi_codes(&sections.join("")),
                    terminal_width as usize - 1,
                    "..."
                )
            );
        }

        std::io::stdout().flush()?;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !PAUSED.load(std::sync::atomic::Ordering::Relaxed) {
            if last_update.elapsed() > std::time::Duration::from_secs(60 * 59) && !exit_warned {
                exit_warned = true;
                message(
                    MessageKind::Warning,
                    "No new submissions for 59 minutes, will pause polling.".to_string(),
                )
                .await;
            }
            if last_update.elapsed() > std::time::Duration::from_secs(60 * 60) {
                PAUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                last_update = std::time::Instant::now();
            }
        }
    }
    let (_terminal_width, terminal_height) = termion::terminal_size()?;
    print!("{}", termion::cursor::Goto(1, terminal_height));
    print!("{}", termion::clear::CurrentLine);
    print!("Stopping...");

    screen.flush()?;

    Ok(())
}

#[derive(Debug, Clone)]
struct Submission {
    time: chrono::DateTime<chrono::Utc>,
    problem: String,
    language: String,
    score: usize,
    code_size: String,
    status: SubmissionStatus,
    execution_time: Option<String>,
    memory: Option<String>,
    detail: String,
}

#[derive(Debug, Copy, Clone, strum::Display, strum::EnumString, PartialEq, Eq)]
enum SubmissionStatus {
    #[strum(serialize = "WJ")]
    WaitingJudge,
    #[strum(serialize = "WR")]
    WaitingRejudge,
    #[strum(serialize = "...")]
    Judging,
    #[strum(serialize = "AC")]
    Accepted,
    #[strum(serialize = "WA")]
    WrongAnswer,
    #[strum(serialize = "TLE")]
    TimeLimitExceeded,
    #[strum(serialize = "MLE")]
    MemoryLimitExceeded,
    #[strum(serialize = "RE")]
    RuntimeError,
    #[strum(serialize = "CE")]
    CompileError,
    #[strum(serialize = "OLE")]
    OutputLimitExceeded,
    #[strum(serialize = "IE")]
    InternalError,
}

#[cfg(not(feature = "dummy-submissions"))]
async fn get_title(client: &reqwest::Client, contest_url: &str) -> anyhow::Result<String> {
    let html = client
        .get(contest_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let html = scraper::Html::parse_document(&html);
    let title_selector = scraper::Selector::parse("title").unwrap();
    let title = html.select(&title_selector).next().unwrap();
    let title = title.text().collect::<String>();
    Ok(title.split(" - ").next().unwrap().to_string())
}

#[cfg(feature = "dummy-submissions")]
async fn get_title(_client: &reqwest::Client, _contest_url: &str) -> anyhow::Result<String> {
    Ok("Dummy Contest".to_string())
}
