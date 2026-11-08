//! Measures optional index storage and search latency on repeated local sample games.
//! Run with a source database and a new output path; the source is opened read-only.
use chess_core::{GameTree, PositionPattern};
use db::{Database, Filter, ImportControl};
use rusqlite::{Connection, OpenFlags, params};
use std::{path::PathBuf, time::Instant};

/// Creates a one-million-game scale fixture without changing the source database.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err(
            "usage: structure_bench SOURCE OUTPUT, or structure_bench --query DATABASE".into(),
        );
    }
    if args[1] == "--query" {
        return searches(&Database::open(&args[2])?);
    }
    let source = Connection::open_with_flags(&args[1], OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let path = PathBuf::from(&args[2]);
    if path.exists() {
        return Err("output must be a new file".into());
    }
    drop(Database::open(&path)?);
    let mut output = Connection::open(&path)?;
    let mut statement = source.prepare("SELECT start_fen, moves FROM games LIMIT 1000")?;
    let sample = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if sample.is_empty() {
        return Err("source has no games".into());
    }
    println!(
        "Repeated sample: {} games; fixture: 1,000,000 games",
        sample.len()
    );
    let tx = output.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO games (white, black, result, date, eco, event, round, start_fen, source, time_class, plies, moves) VALUES ('A','B','*','2026-01-01','','','',?2, '', '', 0, ?1)")?;
        for i in 0..1_000_000 {
            insert.execute(params![
                sample[i % sample.len()].1,
                sample[i % sample.len()].0
            ])?;
        }
    }
    tx.commit()?;
    drop(output);
    let before = std::fs::metadata(&path)?.len();
    let mut database = Database::open(&path)?;
    let start = Instant::now();
    database.build_structure_index(&ImportControl::default(), |done, _| {
        if done % 100_000 == 0 {
            println!("Indexed {done}");
        }
    })?;
    println!("Build: {:.2}s", start.elapsed().as_secs_f64());
    drop(database);
    let bytes = std::fs::metadata(&path)?.len() - before;
    println!(
        "Index: {bytes} bytes ({:.1} bytes/game)",
        bytes as f64 / 1_000_000.
    );
    let database = Database::open(&path)?;
    searches(&database)
}

/// Times both the result list and the summaries the Database screen requests.
fn searches(database: &Database) -> Result<(), Box<dyn std::error::Error>> {
    let tree = GameTree::from_fen("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2")?;
    let pattern = PositionPattern::of(&tree.node(GameTree::ROOT).pos);
    for (name, filter) in [
        (
            "Pawns",
            Filter {
                pawn_structure: Some(pattern.pawn_hash),
                ..Default::default()
            },
        ),
        (
            "Material",
            Filter {
                material: Some(pattern.material),
                ..Default::default()
            },
        ),
        (
            "Combined",
            Filter {
                pawn_structure: Some(pattern.pawn_hash),
                material: Some(pattern.material),
                ..Default::default()
            },
        ),
    ] {
        let start = Instant::now();
        let ids = database.filtered_ids(&filter)?;
        let ids_time = start.elapsed().as_secs_f64();
        let start = Instant::now();
        database.stats(&filter)?;
        println!(
            "{name}: {} games, ids {ids_time:.3}s, stats {:.3}s",
            ids.len(),
            start.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
