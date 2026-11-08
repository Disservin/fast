//! Downloading a player's games from Lichess or Chess.com into a database. This only
//! runs after the user asks for it, sends nothing but the username and how far back
//! to look, and streams the games through the ordinary PGN import.

use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};

use db::{ImportControl, ImportOptions, ImportProgress, import_games};
use serde::{Deserialize, Serialize};

/// The client name sent to the sites, without anything about the user.
const USER_AGENT: &str = "Fast chess app (github.com/Disservin/fast)";

/// A site games can be downloaded from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    #[default]
    Lichess,
    ChessCom,
}

impl Site {
    pub fn name(self) -> &'static str {
        match self {
            Self::Lichess => "Lichess",
            Self::ChessCom => "Chess.com",
        }
    }

    /// The host the dialog says Fast connects to.
    pub fn host(self) -> &'static str {
        match self {
            Self::Lichess => "lichess.org",
            Self::ChessCom => "chess.com",
        }
    }
}

/// Where the sites are reached. Tests point these at a local server.
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub lichess: String,
    pub chess_com: String,
    pub https_only: bool,
    /// How long to wait when a site asks Fast to slow down, before trying once more.
    pub retry_wait: Duration,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            lichess: "https://lichess.org".to_owned(),
            chess_com: "https://api.chess.com".to_owned(),
            https_only: true,
            retry_wait: Duration::from_secs(60),
        }
    }
}

/// What to download and where to put it.
#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub site: Site,
    pub user: String,
    /// Only games started after this time, in milliseconds since 1970 (UTC).
    pub since_ms: Option<i64>,
    /// Ask Lichess for clock times and engine evaluations as comments.
    pub clocks_and_evals: bool,
    pub database: PathBuf,
    pub keep_annotations: bool,
}

/// The current step, including work before any games have arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DownloadPhase {
    #[default]
    Connecting,
    FetchingArchives,
    ReceivingGames,
    SavingGames,
}

impl DownloadPhase {
    /// Explains the current step without suggesting that a stalled request has finished.
    pub fn label(self, site: Site) -> String {
        match self {
            Self::Connecting => ui::i18n::tf("Connecting to {}…", &[site.name()]),
            Self::FetchingArchives => {
                ui::i18n::tf("Fetching archive list from {}…", &[site.name()])
            }
            Self::ReceivingGames => ui::i18n::tf("Receiving games from {}…", &[site.name()]),
            Self::SavingGames => ui::i18n::t("Saving downloaded games…").to_owned(),
        }
    }
}

/// News from a running download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadEvent {
    /// A new step started, before its network or database work runs.
    Phase(DownloadPhase),
    /// Games stored so far, those already in the database, and for Chess.com the
    /// months fetched out of all months asked for.
    Progress {
        games: u64,
        duplicates: u64,
        months: Option<(usize, usize)>,
        /// The newest game committed so far, safe to save as the account's last game.
        latest_game_ms: Option<i64>,
    },
    /// The site asked Fast to wait; it tries again in this many seconds.
    Waiting(u64),
}

/// The outcome of a finished or cancelled download.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DownloadSummary {
    pub games: u64,
    pub duplicates: u64,
    pub skipped: u64,
    pub cancelled: bool,
    pub latest_game_ms: Option<i64>,
}

/// Why a download stopped, worded for the dialog with what to do next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadError {
    UnknownUser(Site, String),
    Offline,
    RateLimited(Site),
    /// The connection broke after some games were stored.
    Interrupted(u64),
    Http(u16),
    Database(String),
    Response(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ui::i18n::{t, tf};
        let text = match self {
            Self::UnknownUser(site, user) => tf("No {} user named '{}'", &[site.name(), user]),
            Self::Offline => t("You're offline. Try again later.").to_owned(),
            Self::RateLimited(site) => tf(
                "{} asks us to wait. Try again in a few minutes.",
                &[site.name()],
            ),
            Self::Interrupted(games) => tf(
                "The connection broke after {} games. Update to fetch the rest.",
                &[&games.to_string()],
            ),
            Self::Http(status) => tf(
                "The site answered with error {}. Try again later.",
                &[&status.to_string()],
            ),
            Self::Database(err) => tf("Could not store the games: {}", &[err]),
            Self::Response(err) => tf("Could not read the site's answer: {}", &[err]),
        };
        f.write_str(&text)
    }
}

fn agent(endpoints: &Endpoints) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(endpoints.https_only)
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .timeout_connect(Some(Duration::from_secs(15)))
        // Long histories stream for minutes; only a stalled connection is an error.
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Whether an error means the site could not be reached at all.
fn is_offline(err: &ureq::Error) -> bool {
    matches!(
        err,
        ureq::Error::Io(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Timeout(_)
    )
}

/// What every request of one download shares.
struct Fetch<'a> {
    agent: ureq::Agent,
    site: Site,
    user: &'a str,
    endpoints: &'a Endpoints,
    control: &'a ImportControl,
}

impl Fetch<'_> {
    /// Sends a GET, waiting and retrying once when the site answers 429. `None` means
    /// the user cancelled while waiting.
    fn get(
        &self,
        url: &str,
        query: &[(&str, String)],
        accept: &str,
        on_event: &mut impl FnMut(DownloadEvent),
    ) -> Result<Option<ureq::http::Response<ureq::Body>>, DownloadError> {
        let (agent, site, user, endpoints, control) = (
            &self.agent,
            self.site,
            self.user,
            self.endpoints,
            self.control,
        );
        for attempt in 0..2 {
            let mut request = agent.get(url).header("Accept", accept);
            for (key, value) in query {
                request = request.query(*key, value);
            }
            let response = request.call().map_err(|err| {
                if is_offline(&err) {
                    DownloadError::Offline
                } else {
                    DownloadError::Response(err.to_string())
                }
            })?;
            match response.status().as_u16() {
                200 => return Ok(Some(response)),
                404 => return Err(DownloadError::UnknownUser(site, user.to_owned())),
                429 if attempt == 0 => {
                    let until = Instant::now() + endpoints.retry_wait;
                    loop {
                        if control.is_cancelled() {
                            return Ok(None);
                        }
                        let left = until.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        on_event(DownloadEvent::Waiting(left.as_secs_f64().ceil() as u64));
                        std::thread::sleep(left.min(Duration::from_millis(250)));
                    }
                }
                429 => return Err(DownloadError::RateLimited(site)),
                status => return Err(DownloadError::Http(status)),
            }
        }
        Err(DownloadError::RateLimited(site))
    }
}

/// Downloads the request's games into its database. Cancelling keeps every game
/// already committed.
pub fn download(
    request: &DownloadRequest,
    endpoints: &Endpoints,
    control: &ImportControl,
    mut on_event: impl FnMut(DownloadEvent),
) -> Result<DownloadSummary, DownloadError> {
    let user = request.user.trim();
    if user.is_empty()
        || !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(DownloadError::UnknownUser(request.site, user.to_owned()));
    }
    on_event(DownloadEvent::Phase(DownloadPhase::Connecting));
    let fetch = Fetch {
        agent: agent(endpoints),
        site: request.site,
        user,
        endpoints,
        control,
    };
    let options = ImportOptions {
        keep_annotations: request.keep_annotations,
        skip_known_urls: true,
        ..Default::default()
    };
    match request.site {
        Site::Lichess => lichess(request, &fetch, &options, on_event),
        Site::ChessCom => chess_com(request, &fetch, &options, on_event),
    }
}

fn lichess(
    request: &DownloadRequest,
    fetch: &Fetch<'_>,
    options: &ImportOptions,
    mut on_event: impl FnMut(DownloadEvent),
) -> Result<DownloadSummary, DownloadError> {
    let (user, endpoints, control) = (fetch.user, fetch.endpoints, fetch.control);
    let flag = |on: bool| if on { "true" } else { "false" }.to_owned();
    // Oldest first, so the newest game committed is always safe to resume from.
    let mut query = vec![
        ("sort", "dateAsc".to_owned()),
        ("clocks", flag(request.clocks_and_evals)),
        ("evals", flag(request.clocks_and_evals)),
        ("opening", "true".to_owned()),
    ];
    if let Some(since) = request.since_ms {
        query.push(("since", (since + 1).to_string()));
    }
    let url = format!("{}/api/games/user/{user}", endpoints.lichess);
    let Some(response) = fetch.get(&url, &query, "application/x-chess-pgn", &mut on_event)? else {
        return Ok(DownloadSummary {
            cancelled: true,
            ..Default::default()
        });
    };
    on_event(DownloadEvent::Phase(DownloadPhase::ReceivingGames));
    let mut committed = ImportProgress::default();
    let reader = response.into_body().into_reader();
    let result = import_games(&request.database, reader, options, control, |progress| {
        committed = progress;
        on_event(DownloadEvent::Progress {
            games: progress.games,
            duplicates: progress.duplicates,
            months: None,
            latest_game_ms: progress.latest_game_ms,
        });
    });
    match result {
        Ok(summary) => Ok(DownloadSummary {
            games: summary.games,
            duplicates: summary.duplicates,
            skipped: summary.skipped,
            cancelled: summary.cancelled,
            latest_game_ms: summary.latest_game_ms,
        }),
        Err(db::DbError::Io(_)) => Err(DownloadError::Interrupted(committed.games)),
        Err(err) => Err(DownloadError::Database(err.to_string())),
    }
}

/// The monthly archive list Chess.com returns for a player.
#[derive(Deserialize)]
struct Archives {
    archives: Vec<String>,
}

/// The archive URLs for months on or after the month of `since_ms`, oldest first.
fn archives_since(archives: Vec<String>, since_ms: Option<i64>) -> Vec<String> {
    let Some(since) = since_ms else {
        return archives;
    };
    let days = since.div_euclid(86_400_000);
    let (year, month) = civil_month(days);
    archives
        .into_iter()
        .filter(|url| {
            let mut parts = url.rsplit('/');
            let month_part = parts.next().and_then(|m| m.parse::<i64>().ok());
            let year_part = parts.next().and_then(|y| y.parse::<i64>().ok());
            match (year_part, month_part) {
                (Some(y), Some(m)) => (y, m) >= (year, month),
                _ => true,
            }
        })
        .collect()
}

/// The year and month of a day counted from 1970-01-01.
fn civil_month(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month)
}

fn chess_com(
    request: &DownloadRequest,
    fetch: &Fetch<'_>,
    options: &ImportOptions,
    mut on_event: impl FnMut(DownloadEvent),
) -> Result<DownloadSummary, DownloadError> {
    let (user, endpoints, control) = (fetch.user, fetch.endpoints, fetch.control);
    let base = &endpoints.chess_com;
    let url = format!("{base}/pub/player/{}/games/archives", user.to_lowercase());
    on_event(DownloadEvent::Phase(DownloadPhase::FetchingArchives));
    let Some(response) = fetch.get(&url, &[], "application/json", &mut on_event)? else {
        return Ok(DownloadSummary {
            cancelled: true,
            ..Default::default()
        });
    };
    on_event(DownloadEvent::Phase(DownloadPhase::FetchingArchives));
    let text = response
        .into_body()
        .read_to_string()
        .map_err(|err| DownloadError::Response(err.to_string()))?;
    let archives: Archives =
        serde_json::from_str(&text).map_err(|err| DownloadError::Response(err.to_string()))?;
    let months = archives_since(archives.archives, request.since_ms);
    let mut total = DownloadSummary::default();
    on_event(DownloadEvent::Progress {
        games: 0,
        duplicates: 0,
        months: Some((0, months.len())),
        latest_game_ms: None,
    });
    for (index, archive) in months.iter().enumerate() {
        if control.is_cancelled() {
            total.cancelled = true;
            break;
        }
        // Only follow archive links on the configured host.
        let Some(path) = archive.strip_prefix(base.as_str()) else {
            continue;
        };
        on_event(DownloadEvent::Phase(DownloadPhase::ReceivingGames));
        let Some(response) = fetch.get(
            &format!("{base}{path}/pgn"),
            &[],
            "application/x-chess-pgn",
            &mut on_event,
        )?
        else {
            total.cancelled = true;
            break;
        };
        on_event(DownloadEvent::Phase(DownloadPhase::ReceivingGames));
        let mut pgn = Vec::new();
        response
            .into_body()
            .into_reader()
            .read_to_end(&mut pgn)
            .map_err(|_| DownloadError::Interrupted(total.games))?;
        on_event(DownloadEvent::Phase(DownloadPhase::SavingGames));
        let summary = import_games(&request.database, pgn.as_slice(), options, control, |_| {})
            .map_err(|err| DownloadError::Database(err.to_string()))?;
        total.games += summary.games;
        total.duplicates += summary.duplicates;
        total.skipped += summary.skipped;
        total.latest_game_ms = total.latest_game_ms.max(summary.latest_game_ms);
        on_event(DownloadEvent::Progress {
            games: total.games,
            duplicates: total.duplicates,
            months: Some((index + 1, months.len())),
            latest_game_ms: total.latest_game_ms,
        });
        if summary.cancelled {
            total.cancelled = true;
            break;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };

    /// One scripted answer: status, body, and whether to cut the connection halfway.
    struct Reply {
        status: u16,
        body: String,
        cut: bool,
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            body: body.to_owned(),
            cut: false,
        }
    }

    /// Serves the replies in order on a local port, with `{BASE}` in a body replaced
    /// by the server's own URL; returns that URL and the request lines it received.
    fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let address = base.clone();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                log.lock().unwrap().push(line.trim().to_owned());
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                }
                let full = reply.body.replace("{BASE}", &address);
                let body = if reply.cut {
                    &full[..full.len() / 2]
                } else {
                    &full[..]
                };
                let head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    full.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        (base, seen)
    }

    fn endpoints(base: &str) -> Endpoints {
        Endpoints {
            lichess: base.to_owned(),
            chess_com: base.to_owned(),
            https_only: false,
            retry_wait: Duration::from_millis(300),
        }
    }

    fn game(id: u32, day: u32) -> String {
        format!(
            "[Site \"https://lichess.org/g{id}\"]\n[White \"me\"]\n[Black \"x\"]\n[Result \"1-0\"]\n\
             [UTCDate \"2024.01.{day:02}\"]\n[UTCTime \"12:00:00\"]\n\n1. e4 e5 1-0\n\n"
        )
    }

    fn request(site: Site, dir: &tempfile::TempDir) -> DownloadRequest {
        DownloadRequest {
            site,
            user: "Me_1".to_owned(),
            since_ms: None,
            clocks_and_evals: true,
            database: dir.path().join("d.sqlite"),
            keep_annotations: true,
        }
    }

    #[test]
    fn lichess_streams_games_and_skips_them_the_second_time() {
        let pgn: String = (1..=3).map(|n| game(n, n)).collect();
        let (base, seen) = serve(vec![reply(200, &pgn), reply(200, &pgn)]);
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(Site::Lichess, &dir);
        let first = download(&req, &endpoints(&base), &ImportControl::default(), |_| {}).unwrap();
        assert_eq!((first.games, first.duplicates), (3, 0));
        assert!(first.latest_game_ms.is_some());
        req.since_ms = Some(1000);
        let second = download(&req, &endpoints(&base), &ImportControl::default(), |_| {}).unwrap();
        assert_eq!((second.games, second.duplicates), (0, 3));
        let lines = seen.lock().unwrap();
        assert!(
            lines[0].starts_with("GET /api/games/user/Me_1?"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("sort=dateAsc") && lines[0].contains("clocks=true"));
        assert!(lines[1].contains("since=1001"));
    }

    #[test]
    fn rate_limit_waits_and_retries_once() {
        let (base, _) = serve(vec![reply(429, ""), reply(200, &game(1, 1))]);
        let dir = tempfile::tempdir().unwrap();
        let mut waited = false;
        let summary = download(
            &request(Site::Lichess, &dir),
            &endpoints(&base),
            &ImportControl::default(),
            |event| waited |= matches!(event, DownloadEvent::Waiting(_)),
        )
        .unwrap();
        assert!(waited);
        assert_eq!(summary.games, 1);
        let (base, _) = serve(vec![reply(429, ""), reply(429, "")]);
        let err = download(
            &request(Site::Lichess, &dir),
            &endpoints(&base),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(err, DownloadError::RateLimited(Site::Lichess));
    }

    #[test]
    fn unknown_users_and_bad_names_are_reported() {
        let (base, _) = serve(vec![reply(404, "")]);
        let dir = tempfile::tempdir().unwrap();
        let err = download(
            &request(Site::ChessCom, &dir),
            &endpoints(&base),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(
            err,
            DownloadError::UnknownUser(Site::ChessCom, "Me_1".into())
        );
        assert_eq!(err.to_string(), "No Chess.com user named 'Me_1'");
        let mut bad = request(Site::Lichess, &dir);
        bad.user = "../x".into();
        assert!(matches!(
            download(&bad, &endpoints(&base), &ImportControl::default(), |_| {}),
            Err(DownloadError::UnknownUser(..))
        ));
    }

    #[test]
    fn a_dropped_connection_keeps_nothing_half_written() {
        let pgn: String = (1..=4).map(|n| game(n, n)).collect();
        let (base, _) = serve(vec![Reply {
            status: 200,
            body: pgn,
            cut: true,
        }]);
        let dir = tempfile::tempdir().unwrap();
        let err = download(
            &request(Site::Lichess, &dir),
            &endpoints(&base),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::Interrupted(_)), "{err:?}");
    }

    #[test]
    fn chess_com_reads_the_archive_list_month_by_month() {
        let archives = "{\"archives\":[\"{BASE}/pub/player/me_1/games/2023/12\",\
            \"{BASE}/pub/player/me_1/games/2024/01\",\"https://elsewhere.example/x/2024/02\"]}";
        let (base, seen) = serve(vec![
            reply(200, archives),
            reply(200, &game(1, 5)),
            reply(200, &game(2, 6)),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let mut months = Vec::new();
        let mut phases = Vec::new();
        let summary = download(
            &request(Site::ChessCom, &dir),
            &endpoints(&base),
            &ImportControl::default(),
            |event| {
                if let DownloadEvent::Phase(phase) = event {
                    phases.push(phase);
                }
                if let DownloadEvent::Progress {
                    months: Some(done), ..
                } = event
                {
                    months.push(done);
                }
            },
        )
        .unwrap();
        assert_eq!(summary.games, 2);
        assert_eq!(phases.first(), Some(&DownloadPhase::Connecting));
        assert!(phases.contains(&DownloadPhase::ReceivingGames));
        assert!(phases.contains(&DownloadPhase::SavingGames));
        assert_eq!(months, vec![(0, 3), (1, 3), (2, 3)]);
        let lines = seen.lock().unwrap();
        assert!(lines[0].contains("/pub/player/me_1/games/archives"));
        assert!(lines[2].contains("/games/2024/01/pgn"));
        // The link to another host is never followed.
        assert_eq!(lines.len(), 3);
    }

    /// Both sites report startup while a server is still withholding its response.
    #[test]
    fn startup_progress_arrives_before_a_slow_server_responds() {
        for site in [Site::Lichess, Site::ChessCom] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let (release, wait) = std::sync::mpsc::channel();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                }
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                let body = if site == Site::ChessCom {
                    "{\"archives\":[]}".to_owned()
                } else {
                    game(1, 1)
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let dir = tempfile::tempdir().unwrap();
            let request = request(site, &dir);
            let endpoints = endpoints(&base);
            let (send, events) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                download(&request, &endpoints, &ImportControl::default(), |event| {
                    let _ = send.send(event);
                })
            });
            assert_eq!(
                events.recv_timeout(Duration::from_secs(5)).unwrap(),
                DownloadEvent::Phase(DownloadPhase::Connecting)
            );
            if site == Site::ChessCom {
                assert_eq!(
                    events.recv_timeout(Duration::from_secs(5)).unwrap(),
                    DownloadEvent::Phase(DownloadPhase::FetchingArchives)
                );
            }
            release.send(()).unwrap();
            assert_eq!(
                worker.join().unwrap().unwrap().games,
                if site == Site::Lichess { 1 } else { 0 }
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn archives_from_the_month_of_the_last_game() {
        let list = vec![
            "https://api.chess.com/pub/player/a/games/2023/11".to_owned(),
            "https://api.chess.com/pub/player/a/games/2023/12".to_owned(),
            "https://api.chess.com/pub/player/a/games/2024/01".to_owned(),
        ];
        // 2023-12-15.
        let since = Some(1_702_598_400_000);
        let kept = archives_since(list.clone(), since);
        assert_eq!(kept, list[1..].to_vec());
        assert_eq!(archives_since(list.clone(), None), list);
        assert_eq!(civil_month(0), (1970, 1));
        assert_eq!(civil_month(19_782), (2024, 2));
    }
}
