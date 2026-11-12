//! Engine tournaments through fastchess, which is downloaded on first use rather than bundled.
//!
//! Fast builds a fastchess command line (or takes a pasted one, or a saved `config.json` to
//! continue), runs it in a folder of its own and reads its human-readable output. See
//! `docs/fastchess-integration.md` for what fastchess could offer to make this less fragile.
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context as _, Result, bail, ensure};

use crate::engine_catalog::{Download, Entry, Format};

mod command;
mod files;
mod output;
mod process;
mod setup;

pub use self::{command::*, files::*, output::*, process::*, setup::*};

pub const VERSION: &str = "1.8.3-alpha";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_tournament_folders_but_the_kept_one() {
        let data = tempfile::tempdir().unwrap();
        let root = tournaments_dir(data.path());
        for name in ["a", "b"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("games.pgn"), "1234").unwrap();
        }
        assert_eq!(dir_size(&root), 8);
        let kept = root.join("b");
        assert_eq!(clear_tournaments(data.path(), Some(&kept)).unwrap(), (1, 4));
        assert!(!root.join("a").exists());
        assert!(kept.join("games.pgn").exists());
    }

    fn setup(participants: Vec<Participant>) -> Setup {
        Setup {
            participants,
            limit: Limit::TimeControl,
            limit_value: "10+0.1".into(),
            hash: "16".into(),
            threads: "1".into(),
            rounds: "100".into(),
            games_per_round: 2,
            concurrency: "4".into(),
            gauntlet: false,
            chess960: false,
            noswap: false,
            reverse: false,
            openings: String::new(),
            random_order: true,
            plies: String::new(),
            sprt: true,
            elo0: "0".into(),
            elo1: "2".into(),
            alpha: "0.05".into(),
            beta: "0.05".into(),
            sprt_model: "normalized",
            draw_adjudication: false,
            draw_movenumber: "32".into(),
            draw_movecount: "6".into(),
            draw_score: "10".into(),
            resign_adjudication: false,
            resign_movecount: "3".into(),
            resign_score: "600".into(),
            resign_twosided: true,
            maxmoves: String::new(),
            tb_paths: String::new(),
            tb_pieces: String::new(),
            tb_ignore50: false,
            tb_adjudicate: "BOTH",
            recover: true,
            seed: String::new(),
            wait: String::new(),
            time_margin: String::new(),
            restart: false,
            affinity: false,
            engine_args: String::new(),
            pgn: PgnOut::default(),
            extra: "-srand 42".into(),
        }
    }

    #[test]
    fn variant_tablebases_and_pgn_options() {
        let mut setup = setup(vec![engine("A"), engine("B")]);
        setup.chess960 = true;
        setup.noswap = true;
        setup.engine_args = "--uci --quiet".into();
        setup.tb_paths = "/tb/345".into();
        setup.tb_pieces = "5".into();
        setup.tb_ignore50 = true;
        setup.tb_adjudicate = "DRAW";
        setup.pgn.notation = "uci";
        setup.pgn.nodes = true;
        setup.pgn.pv = true;
        let args = setup.args(Path::new("/out")).unwrap();
        let after = |flag: &str| {
            let at = args.iter().position(|arg| arg == flag).unwrap();
            args[at + 1].clone()
        };
        assert_eq!(after("-variant"), "fischerandom");
        assert!(args.contains(&"-noswap".to_owned()));
        assert!(!args.contains(&"-reverse".to_owned()));
        assert!(args.contains(&"args=--uci --quiet".to_owned()));
        assert_eq!(after("-tb"), "/tb/345");
        assert_eq!(after("-tbpieces"), "5");
        assert!(args.contains(&"-tbignore50".to_owned()));
        assert_eq!(after("-tbadjudicate"), "DRAW");
        assert!(args.contains(&"model=normalized".to_owned()));
        let pgnout = args.iter().position(|arg| arg == "-pgnout").unwrap();
        assert_eq!(
            args[pgnout + 2..pgnout + 5],
            ["notation=uci", "nodes=true", "pv=true"]
        );
    }

    #[test]
    fn advanced_engine_and_scheduling_options() {
        let mut setup = setup(vec![engine("A"), engine("B")]);
        setup.extra.clear();
        setup.seed = "42".into();
        setup.wait = "100".into();
        setup.time_margin = "50".into();
        setup.restart = true;
        setup.affinity = true;
        let args = setup.args(Path::new("/tmp/run")).unwrap();
        for expected in ["timemargin=50", "restart=on", "-use-affinity"] {
            assert!(args.iter().any(|arg| arg == expected));
        }
        for pair in [["-srand", "42"], ["-wait", "100"]] {
            assert!(args.windows(2).any(|args| args == pair));
        }
        let each = args.iter().position(|arg| arg == "-each").unwrap();
        let next_flag = args[each + 1..]
            .iter()
            .position(|arg| arg.starts_with('-'))
            .unwrap()
            + each
            + 1;
        assert!(args[each..next_flag].iter().any(|arg| arg == "restart=on"));
        setup.wait = "-1".into();
        assert!(setup.args(Path::new("/tmp/run")).is_err());
        setup.wait.clear();
        setup.seed = "invalid".into();
        assert!(setup.args(Path::new("/tmp/run")).is_err());
    }

    fn engine(name: &str) -> Participant {
        Participant {
            name: name.into(),
            cmd: PathBuf::from(format!("/engines/{name}")),
            options: [
                ("Hash".to_owned(), "512".to_owned()),
                ("EvalFile".to_owned(), "nn.nnue".to_owned()),
            ]
            .into(),
        }
    }

    #[test]
    fn release_has_a_build_for_every_platform_fast_ships() {
        let entry = release();
        for (os, arch) in [
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("windows", "x86_64"),
            ("windows", "aarch64"),
        ] {
            let asset = entry
                .downloads
                .iter()
                .find(|asset| asset.os == os && asset.arch == arch)
                .unwrap();
            assert!(
                asset
                    .url
                    .starts_with("https://github.com/Disservin/fastchess/")
            );
            assert_eq!(asset.sha256.len(), 64);
            assert!(asset.executable.starts_with("fastchess-"));
        }
    }

    #[test]
    fn registered_engine_omits_empty_options_and_preserves_configured_values() {
        let mut registered = engine("sf");
        registered.options.extend([
            ("Debug Log File".into(), "".into()),
            ("SyzygyPath".into(), "   ".into()),
            ("Skill Level".into(), "0".into()),
            ("UCI_ShowWDL".into(), "false".into()),
            ("Some File".into(), "/books/path with spaces/file".into()),
        ]);
        let args = setup(vec![registered, engine("other")])
            .args(Path::new("/tmp/run"))
            .unwrap();
        assert!(
            !args
                .iter()
                .any(|arg| arg.starts_with("option.Debug Log File=")
                    || arg.starts_with("option.SyzygyPath="))
        );
        assert!(args.iter().any(|arg| arg == "option.Skill Level=0"));
        assert!(args.iter().any(|arg| arg == "option.UCI_ShowWDL=false"));
        assert!(
            args.iter()
                .any(|arg| arg == "option.Some File=/books/path with spaces/file")
        );
        assert!(args.iter().any(|arg| arg == "option.EvalFile=nn.nnue"));
    }

    #[test]
    fn setup_builds_a_complete_command_with_unique_names() {
        let dir = Path::new("/data/t1");
        let args = setup(vec![engine("sf"), engine("sf")]).args(dir).unwrap();
        let text = args.join(" ");
        assert!(
            text.contains("-engine cmd=/engines/sf name=sf dir=/engines option.EvalFile=nn.nnue")
        );
        assert!(text.contains("name=sf (2)"));
        assert!(!text.contains("option.Hash=512"), "{text}");
        assert!(text.contains("-each tc=10+0.1 option.Hash=16 option.Threads=1"));
        assert!(text.contains("-rounds 100 -repeat -concurrency 4"));
        assert!(text.contains("-sprt elo0=0 elo1=2 alpha=0.05 beta=0.05"));
        assert!(text.contains("-recover"));
        assert!(text.contains("-srand 42"));
        assert!(text.ends_with(&format!(
            "-pgnout file={} -config outname={}",
            dir.join("games.pgn").display(),
            dir.join("config.json").display()
        )));
    }

    #[test]
    fn setup_rejects_bad_input() {
        let dir = Path::new("/data/t1");
        assert!(setup(vec![engine("a")]).args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.rounds = "many".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.limit_value = "10 minutes".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.openings = "/no/such/book.epd".into();
        assert!(bad.args(dir).is_err());
        let mut bad = setup(vec![engine("a"), engine("b")]);
        bad.draw_adjudication = true;
        bad.draw_score = "-5".into();
        assert!(bad.args(dir).is_err());
    }

    #[test]
    fn setup_passes_the_adjudication_rules_given() {
        let dir = Path::new("/data/t1");
        let mut setup = setup(vec![engine("a"), engine("b")]);
        setup.draw_adjudication = true;
        (
            setup.draw_movenumber,
            setup.draw_movecount,
            setup.draw_score,
        ) = ("40".into(), "8".into(), "5".into());
        setup.resign_adjudication = true;
        (setup.resign_movecount, setup.resign_score) = ("4".into(), "800".into());
        setup.resign_twosided = false;
        let args = setup.args(dir).unwrap();
        let after = |flag: &str| {
            let start = args.iter().position(|arg| arg == flag).unwrap() + 1;
            args[start..start + 3].join(" ")
        };
        assert_eq!(after("-draw"), "movenumber=40 movecount=8 score=5");
        assert_eq!(after("-resign"), "movecount=4 score=800 twosided=false");
    }

    #[test]
    fn splits_pasted_command_lines() {
        assert_eq!(
            split_args("./fastchess -engine cmd=\"my engine\" name='a b' \\\n  -each tc=8+0.08")
                .unwrap(),
            [
                "./fastchess",
                "-engine",
                "cmd=my engine",
                "name=a b",
                "-each",
                "tc=8+0.08"
            ]
        );
        assert_eq!(
            split_args(r"-engine cmd=C:\engines\sf.exe ^").unwrap(),
            ["-engine", r"cmd=C:\engines\sf.exe", "^"]
        );
        assert!(split_args("-engine cmd=\"open").is_err());
    }

    #[test]
    fn pasted_commands_and_configs_are_recognised() {
        let Pasted::Args(args) =
            parse_paste("fastchess.exe -engine cmd=a -engine cmd=b -rounds 2").unwrap()
        else {
            panic!("not a command");
        };
        assert_eq!(args[0], "-engine");
        let dir = tempfile::tempdir().unwrap();
        let (launch, config) = Pasted::Args(args.clone())
            .launch(dir.path(), dir.path())
            .unwrap();
        assert_eq!(config, dir.path().join("config.json"));
        assert_eq!(launch[launch.len() - 2], "-config");
        let own = [
            args.clone(),
            vec!["-config".into(), "outname=mine.json".into()],
        ]
        .concat();
        let (launch, config) = Pasted::Args(own.clone())
            .launch(dir.path(), Path::new("/work"))
            .unwrap();
        assert_eq!(launch, own);
        assert_eq!(config, Path::new("/work/mine.json"));

        let json = r#"{"engines": [{"name": "a"}, {"name": "b"}], "stats": {}}"#;
        let pasted = parse_paste(json).unwrap();
        let (launch, config) = pasted.launch(dir.path(), dir.path()).unwrap();
        assert!(config.is_file());
        assert_eq!(launch, resume_args(&config));

        assert!(parse_paste("{\"rounds\": 3}").is_err());
        assert!(parse_paste("hello world").is_err());
    }

    #[test]
    fn parses_fastchess_output() {
        assert_eq!(
            parse_line("Started game 4 of 300 (c vs a)"),
            Line::Started {
                game: 4,
                total: 300
            }
        );
        assert_eq!(
            parse_line("Finished game 3 (a vs c): 1-0 {White mates}"),
            Line::Finished {
                game: 3,
                white: "a",
                black: "c",
                result: "1-0",
                reason: "White mates"
            }
        );
        assert_eq!(
            parse_line("--------------------------------------------------"),
            Line::Rule
        );
        assert_eq!(parse_line("Finished match"), Line::Text("Finished match"));
        let block: Vec<String> = [
            "Results of a vs b (1+0.01, 1t, 8MB):",
            "Elo: -58.45 +/- 98.05, nElo: -173.72 +/- 278.00",
            "LOS: 11.03 %, DrawRatio: 66.67 %, PairsRatio: 0.00",
            "Games: 6, Wins: 0, Losses: 1, Draws: 5, Points: 2.5 (41.67 %)",
            "Ptnml(0-2): [0, 1, 2, 0, 0], WL/DD Ratio: 0.00",
            "LLR: -0.03 (-0.9%) (-2.94, 2.94) [0.00, 5.00]",
        ]
        .map(String::from)
        .into();

        let numbers = results(&block).expect("a head-to-head block");
        assert_eq!(numbers.engines, Some(("a".into(), "b".into())));
        assert_eq!((numbers.elo, numbers.elo_err), (-58.45, 98.05));
        assert_eq!((numbers.nelo, numbers.nelo_err), (-173.72, 278.0));
        assert_eq!((numbers.los, numbers.draw_ratio), (11.03, 66.67));
        assert_eq!(
            (numbers.games, numbers.wins, numbers.losses, numbers.draws),
            (6, 0, 1, 5)
        );
        assert_eq!((numbers.points, numbers.score_pct), (2.5, 41.67));
        assert_eq!(numbers.ptnml, [0, 1, 2, 0, 0]);
        assert_eq!(
            numbers.llr,
            Some(Llr {
                value: -0.03,
                lower: -2.94,
                upper: 2.94
            })
        );
        // A table for more engines is not a head-to-head block.
        assert!(results(&["Results of 3 engines:".to_owned()]).is_none());
    }

    #[test]
    fn finds_games_in_a_pgn_file() {
        let pgn = "[Event \"x\"]\n[Round \"2\"]\n\n1. e4 1-0\n\n[Event \"x\"]\n[Round \"1\"]\n\n1. d4 0-1\n";
        assert_eq!(count_games(pgn), 2);
        assert!(nth_game(pgn, 1).unwrap().ends_with("1. d4 0-1"));
        assert!(nth_game(pgn, 2).is_none());
    }

    #[test]
    fn saved_runs_report_engines_and_games() {
        let dir = tempfile::tempdir().unwrap();
        let run = tournaments_dir(dir.path()).join("t1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(
            run.join("config.json"),
            r#"{"engines": [{"name": "a"}, {"name": "b"}],
                "stats": {"a vs b": {"wins": 3, "losses": 1, "draws": 4}}}"#,
        )
        .unwrap();
        let runs = saved_runs(dir.path());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].engines, ["a", "b"]);
        assert_eq!(runs[0].games, 8);
    }

    /// Downloads the pinned release, plays a short match, stops it and continues it.
    #[test]
    #[ignore = "downloads fastchess; set FAST_TEST_ENGINE to a UCI engine"]
    fn download_play_stop_and_continue() {
        let engine = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        let data = tempfile::tempdir().unwrap();
        let program = install(data.path(), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!(installed(data.path()), Some(program.clone()));
        let dir = new_run_dir(data.path()).unwrap();
        let participant = |name: &str| Participant {
            name: name.into(),
            cmd: engine.clone(),
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant("a"), participant("b")]);
        setup.limit_value = "1+0.01".into();
        setup.rounds = "200".into();
        setup.concurrency = "2".into();
        setup.extra = String::new();
        let args = setup.args(&dir).unwrap();
        let collect = |process: &Process, stop_after: Option<u64>| {
            let mut finished = 0;
            let mut total = 0;
            let mut results = false;
            while let Ok(line) = process.lines.recv_blocking() {
                match parse_line(&line) {
                    Line::Started { total: all, .. } => total = all,
                    Line::Finished { .. } => {
                        finished += 1;
                        if Some(finished) == stop_after {
                            process.stop_handle().stop();
                        }
                    }
                    Line::Text(text) if text.starts_with("Results of") => results = true,
                    _ => {}
                }
            }
            (finished, total, results, process.wait())
        };
        let process = spawn(&program, &args, &dir, None).unwrap();
        let (finished, total, results, exit) = collect(&process, Some(4));
        assert!(
            finished >= 4 && total == 400 && results,
            "{finished} {total}"
        );
        assert!(exit.stopped && !exit.success);
        let config = dir.join("config.json");
        let saved = &saved_runs(data.path())[0];
        // With two games per round, only complete rounds are saved.
        assert!(
            saved.games.is_multiple_of(2) && saved.games <= finished,
            "{}",
            saved.games
        );
        let process = spawn(&program, &resume_args(&config), &dir, None).unwrap();
        let (finished, _, _, exit) = collect(&process, Some(2));
        assert!(finished >= 2 && exit.stopped);
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        assert!(count_games(&pgn) >= 6);
    }

    /// Verifies selfplay against any local fastchess build, including builds without events.
    #[test]
    #[ignore = "set FAST_TEST_FASTCHESS and FAST_TEST_ENGINE to local executables"]
    fn selfplay_runs_two_processes_of_the_same_engine_and_saves_legal_games() {
        let program = PathBuf::from(std::env::var("FAST_TEST_FASTCHESS").expect("fastchess"));
        let engine_path = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        let data = tempfile::tempdir().unwrap();
        let dir = new_run_dir(data.path()).unwrap();
        let participant = Participant {
            name: "Selfplay".into(),
            cmd: engine_path,
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant.clone(), participant]);
        setup.rounds = "1".into();
        setup.concurrency = "1".into();
        setup.limit = Limit::Nodes;
        setup.limit_value = "100".into();
        setup.maxmoves = "8".into();
        setup.sprt = false;
        setup.extra.clear();
        let process = spawn(&program, &setup.args(&dir).unwrap(), &dir, None).unwrap();
        let mut finished = 0;
        let mut output = Vec::new();
        while let Ok(line) = process.lines.recv_blocking() {
            if matches!(parse_line(&line), Line::Finished { .. }) {
                finished += 1;
            }
            output.push(line);
        }
        assert!(process.wait().success, "{output:?}");
        assert_eq!(finished, 2, "{output:?}");
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        let games = chess_core::parse_pgn(&pgn).unwrap();
        assert_eq!(games.len(), 2);
        for game in games {
            assert!(!game.mainline().is_empty());
            let names = [game.header("White").unwrap(), game.header("Black").unwrap()];
            assert!(names.contains(&"Selfplay"));
            assert!(names.contains(&"Selfplay (2)"));
            assert_ne!(game.header("Result"), Some("*"));
        }
    }

    /// Runs a fastchess build with `-events` and follows it as the Tournament tab does.
    #[test]
    #[ignore = "set FAST_TEST_FASTCHESS to a fastchess build with -events and FAST_TEST_ENGINE"]
    fn event_stream_drives_live_games() {
        use crate::tournament_live::{LiveGames, events_args, has_events};
        let program = PathBuf::from(std::env::var("FAST_TEST_FASTCHESS").expect("fastchess"));
        let engine = PathBuf::from(std::env::var("FAST_TEST_ENGINE").expect("engine path"));
        assert!(has_events(&program));
        let data = tempfile::tempdir().unwrap();
        let dir = new_run_dir(data.path()).unwrap();
        let participant = |name: &str| Participant {
            name: name.into(),
            cmd: engine.clone(),
            options: BTreeMap::new(),
        };
        let mut setup = setup(vec![participant("a"), participant("b")]);
        setup.limit_value = "2+0.02".into();
        setup.rounds = "50".into();
        setup.concurrency = "2".into();
        setup.extra = String::new();
        let mut args = setup.args(&dir).unwrap();
        args.extend(events_args());
        let live = Arc::new(Mutex::new(LiveGames::default()));
        let process = spawn(&program, &args, &dir, Some(live.clone())).unwrap();
        let mut finished = 0;
        let mut seen_moves = 0;
        while let Ok(line) = process.lines.recv_blocking() {
            assert!(
                !line.starts_with("{\"event\""),
                "events must not reach the log"
            );
            if let Line::Finished { .. } = parse_line(&line) {
                finished += 1;
                let games = live.lock().unwrap().snapshot();
                seen_moves = seen_moves.max(games.iter().map(|g| g.moves.len()).max().unwrap_or(0));
                assert!(
                    games
                        .iter()
                        .all(|game| game.number > 0 && game.white.is_some())
                );
                if finished == 3 {
                    process.stop_handle().stop();
                }
            }
        }
        assert!(process.wait().stopped);
        assert!(seen_moves > 10, "{seen_moves}");
        // Interrupted games are aborted, so no board is left behind.
        assert!(live.lock().unwrap().snapshot().is_empty());
        let pgn = std::fs::read_to_string(dir.join("games.pgn")).unwrap();
        let first = game_by_id(&pgn, 1).or_else(|| game_by_id(&pgn, 2)).unwrap();
        assert!(first.contains("[GameId "));
        // The event options are not saved for a later resume.
        let config = std::fs::read_to_string(dir.join("config.json")).unwrap();
        assert!(!config.contains("events"));
    }
}
