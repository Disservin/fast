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
        fen: START.to_owned(),
        moves: Vec::new(),
        limits,
        search_id,
    }
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
