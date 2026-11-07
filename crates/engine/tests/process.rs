//! Process-level tests driving a scripted fake engine.
#![cfg(unix)]

use std::{path::PathBuf, time::Duration};

use engine::{EngineEvent, EngineHandle, Limits, Score, SearchRequest};
use smol::{channel::Receiver, future};

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_uci.sh")
}

fn next(events: &Receiver<EngineEvent>) -> EngineEvent {
    future::block_on(future::or(
        async { events.recv().await.expect("event stream closed") },
        async {
            smol::Timer::after(Duration::from_secs(10)).await;
            panic!("timed out waiting for an engine event");
        },
    ))
}

fn request(search_id: u64, limits: Option<Limits>) -> SearchRequest {
    SearchRequest {
        search_moves: Vec::new(),
        fen: START.to_owned(),
        moves: Vec::new(),
        limits,
        search_id,
        game: 1,
    }
}

/// What the app wrote to the engine so far.
fn sent(engine: &EngineHandle) -> Vec<String> {
    engine
        .log()
        .snapshot(true)
        .into_iter()
        .filter(|line| line.kind == engine::LogKind::Sent)
        .map(|line| line.text)
        .collect()
}

/// Waits until the app has written `text`, returning everything written by then.
fn wait_for_sent(engine: &EngineHandle, text: &str) -> Vec<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let lines = sent(engine);
        if lines.iter().any(|line| line == text) {
            return lines;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never sent {text}: {lines:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn root_move_restrictions_reach_the_engine_for_finite_and_infinite_searches() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let mut restricted = request(
        1,
        Some(Limits {
            depth: Some(8),
            ..Default::default()
        }),
    );
    restricted.search_moves = vec!["d2d4".into(), "e2e4".into()];
    engine.search(restricted);
    wait_for_sent(&engine, "go depth 8 searchmoves d2d4 e2e4");
    let mut infinite = request(2, None);
    infinite.search_moves = vec!["d2d4".into()];
    engine.search(infinite);
    wait_for_sent(&engine, "go infinite searchmoves d2d4");
    engine.search(request(
        3,
        Some(Limits {
            depth: Some(4),
            ..Default::default()
        }),
    ));
    wait_for_sent(&engine, "go depth 4");
    engine.quit();
}

#[test]
fn first_search_waits_for_readyok_and_uses_startpos() {
    let engine = EngineHandle::spawn(
        fixture(),
        Vec::new(),
        vec![("Hash".to_owned(), "64".to_owned())],
    );
    // Asked for before the handshake: must still come after uci, options and isready.
    engine.search(request(1, None));
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    assert!(matches!(next(&events), EngineEvent::Analysis { .. }));
    let lines = wait_for_sent(&engine, "go infinite");
    let at = |text: &str| {
        lines
            .iter()
            .position(|line| line == text)
            .unwrap_or_else(|| panic!("{text} missing from {lines:?}"))
    };
    assert!(at("uci") < at("setoption name Hash value 64"));
    assert!(at("setoption name Hash value 64") < at("ucinewgame"));
    assert!(at("ucinewgame") < at("isready"));
    assert!(at("isready") < at("position startpos"));
    assert!(at("position startpos") < at("go infinite"));
    engine.quit();
}

#[test]
fn a_new_game_sends_ucinewgame_and_synchronises_again() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    let depth = Some(Limits {
        depth: Some(1),
        ..Default::default()
    });
    engine.search(request(1, depth));
    assert!(matches!(next(&events), EngineEvent::Analysis { .. }));
    assert!(matches!(next(&events), EngineEvent::BestMove { .. }));
    // The same game again: no further ucinewgame or isready.
    engine.search(request(2, depth));
    assert!(matches!(next(&events), EngineEvent::Analysis { .. }));
    assert!(matches!(next(&events), EngineEvent::BestMove { .. }));
    let count = |lines: &[String], text: &str| lines.iter().filter(|line| *line == text).count();
    let lines = sent(&engine);
    assert_eq!(count(&lines, "ucinewgame"), 1, "{lines:?}");
    assert_eq!(count(&lines, "isready"), 1, "{lines:?}");
    let mut other = request(3, depth);
    other.game = 2;
    other.fen = "8/8/8/8/8/8/k7/K7 w - - 0 1".to_owned();
    engine.search(other);
    assert!(matches!(next(&events), EngineEvent::Analysis { .. }));
    let lines = wait_for_sent(&engine, "position fen 8/8/8/8/8/8/k7/K7 w - - 0 1");
    assert_eq!(count(&lines, "ucinewgame"), 2, "{lines:?}");
    assert_eq!(count(&lines, "isready"), 2, "{lines:?}");
    engine.quit();
}

#[test]
fn handshake_reports_identity_and_option_schema() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let EngineEvent::Ready(info) = next(&engine.events()) else {
        panic!("expected Ready");
    };
    assert_eq!(info.name, "FakeFish 1.0");
    assert_eq!(info.options.len(), 2);
    engine.quit();
}

#[test]
fn analysis_then_cancellation_on_new_position() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let events = engine.events();
    // Requested before the handshake completes: must be replayed once ready.
    engine.search(request(1, None));
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    let EngineEvent::Analysis { search_id, update } = next(&events) else {
        panic!("expected analysis");
    };
    assert_eq!(search_id, 1);
    assert_eq!(update.score, Some(Score::Cp(30)));
    assert_eq!(update.pv, vec!["e2e4", "e7e5"]);

    // The new position cancels the running search; its stale "cp 99" output and
    // bestmove are dropped, and the next event belongs to search 2.
    engine.search(request(
        2,
        Some(Limits {
            depth: Some(1),
            ..Default::default()
        }),
    ));
    let EngineEvent::Analysis { search_id, .. } = next(&events) else {
        panic!("expected analysis from the second search");
    };
    assert_eq!(search_id, 2);
    let EngineEvent::BestMove { search_id, best } = next(&events) else {
        panic!("expected best move");
    };
    assert_eq!((search_id, best.as_deref()), (2, Some("d2d4")));
    engine.quit();
}

#[test]
fn options_are_applied_after_handshake() {
    let options = vec![("Marker".to_owned(), "77".to_owned())];
    let engine = EngineHandle::spawn(fixture(), Vec::new(), options);
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    engine.search(request(
        1,
        Some(Limits {
            depth: Some(1),
            ..Default::default()
        }),
    ));
    let EngineEvent::Analysis { update, .. } = next(&events) else {
        panic!("expected analysis");
    };
    assert_eq!(update.score, Some(Score::Cp(77)));
    engine.quit();
}

#[test]
fn option_change_during_a_search_applies_and_resumes_it() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    engine.search(request(4, None));
    let EngineEvent::Analysis { update, .. } = next(&events) else {
        panic!("expected analysis");
    };
    assert_eq!(update.score, Some(Score::Cp(30)));
    engine.set_option("Marker", Some("55".to_owned()));
    // Output from before the stop is dropped; the search resumes with the new value.
    let EngineEvent::Analysis { search_id, update } = next(&events) else {
        panic!("expected resumed analysis");
    };
    assert_eq!(search_id, 4);
    assert_eq!(update.score, Some(Score::Cp(55)));
    let sent: Vec<String> = engine
        .log()
        .snapshot(true)
        .into_iter()
        .filter(|line| line.kind == engine::LogKind::Sent)
        .map(|line| line.text)
        .collect();
    let stop = sent.iter().position(|line| line == "stop").unwrap();
    let option = sent
        .iter()
        .position(|line| line.contains("Marker"))
        .unwrap();
    assert!(stop < option, "{sent:?}");
    engine.quit();
}

#[test]
fn log_records_traffic_and_stderr() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    engine.search(request(1, None));
    assert!(matches!(next(&events), EngineEvent::Analysis { .. }));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let lines = engine.log().snapshot(true);
        let has = |kind, text: &str| {
            lines
                .iter()
                .any(|line| line.kind == kind && line.text.contains(text))
        };
        if has(engine::LogKind::Sent, "uci")
            && has(engine::LogKind::Received, "uciok")
            && has(engine::LogKind::Stderr, "fake engine warning")
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{lines:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.quit();
}

#[test]
fn debug_mode_is_sent_after_the_handshake() {
    let engine = EngineHandle::spawn(fixture(), Vec::new(), Vec::new());
    // Requested before `uciok`, so it must wait for the handshake.
    engine.set_debug(true);
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    engine.set_debug(false);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let sent: Vec<String> = engine
            .log()
            .snapshot(true)
            .into_iter()
            .filter(|line| line.kind == engine::LogKind::Sent)
            .map(|line| line.text)
            .collect();
        let on = sent.iter().position(|text| text == "debug on");
        let off = sent.iter().position(|text| text == "debug off");
        if let (Some(on), Some(off)) = (on, off) {
            assert!(on < off);
            assert!(sent.iter().position(|text| text == "uci") < Some(on));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{sent:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    engine.quit();
}

#[test]
fn crash_is_reported_not_fatal() {
    let engine = EngineHandle::spawn(fixture(), vec!["crash".to_owned()], Vec::new());
    let events = engine.events();
    assert!(matches!(next(&events), EngineEvent::Ready(_)));
    engine.search(request(1, None));
    assert!(matches!(next(&events), EngineEvent::Crashed(_)));
}

#[test]
fn missing_binary_is_reported() {
    let engine = EngineHandle::spawn(PathBuf::from("/nonexistent/engine"), Vec::new(), Vec::new());
    let EngineEvent::Crashed(reason) = next(&engine.events()) else {
        panic!("expected Crashed");
    };
    assert!(reason.contains("could not start"), "{reason}");
}
