use std::ops::ControlFlow;

use pgn_reader::{Nag, Outcome, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
use shakmaty::Position;
use thiserror::Error;

use crate::{GameTree, NodeId};

#[derive(Debug, Error)]
pub enum PgnError {
    #[error("illegal or unreadable move {san} in game {game}: {reason}")]
    Move {
        game: usize,
        san: String,
        reason: String,
    },
    #[error("invalid FEN header in game {game}: {reason}")]
    Fen { game: usize, reason: String },
    #[error("could not read PGN: {0}")]
    Io(#[from] std::io::Error),
}

struct TreeVisitor {
    game: usize,
}

struct Movetext {
    tree: GameTree,
    current: NodeId,
    stack: Vec<NodeId>,
}

impl Visitor for TreeVisitor {
    type Tags = Vec<(String, String)>;
    type Movetext = Movetext;
    type Output = Result<GameTree, PgnError>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        self.game += 1;
        ControlFlow::Continue(Vec::new())
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        tags.push((
            String::from_utf8_lossy(name).into_owned(),
            value.decode_utf8_lossy().into_owned(),
        ));
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        let fen = tags
            .iter()
            .find(|(key, _)| key == "FEN")
            .map(|(_, value)| value.clone());
        let tagged_960 = tags
            .iter()
            .find(|(key, _)| key == "Variant")
            .is_some_and(|(_, value)| crate::Variant::from_tag(value) == crate::Variant::Chess960);
        let parsed = fen.map(|fen| {
            if tagged_960 {
                GameTree::from_fen_variant(&fen, crate::Variant::Chess960)
            } else {
                GameTree::from_fen(&fen)
            }
        });
        let mut tree = match parsed {
            Some(parsed) => match parsed {
                Ok(tree) => tree,
                Err(reason) => {
                    return ControlFlow::Break(Err(PgnError::Fen {
                        game: self.game,
                        reason,
                    }));
                }
            },
            None if tagged_960 => GameTree::chess960(518),
            None => GameTree::new(),
        };
        let variant_header = tree.header("Variant").map(str::to_owned);
        tree.headers = tags;
        // Chess960 games always say so, whichever name the file used.
        if let Some(variant) = variant_header {
            tree.set_header("Variant", &variant);
        }
        if tree.variant == crate::Variant::Chess960 {
            tree.set_header("SetUp", "1");
            let fen = tree.fen(GameTree::ROOT);
            tree.set_header("FEN", &fen);
        }
        ControlFlow::Continue(Movetext {
            tree,
            current: GameTree::ROOT,
            stack: Vec::new(),
        })
    }

    fn san(
        &mut self,
        movetext: &mut Self::Movetext,
        san_plus: SanPlus,
    ) -> ControlFlow<Self::Output> {
        let pos = &movetext.tree.node(movetext.current).pos;
        match san_plus.san.to_move(pos) {
            Ok(mv) => {
                movetext.current = movetext.tree.add_move(movetext.current, mv);
                ControlFlow::Continue(())
            }
            Err(err) => ControlFlow::Break(Err(PgnError::Move {
                game: self.game,
                san: san_plus.to_string(),
                reason: err.to_string(),
            })),
        }
    }

    fn nag(&mut self, movetext: &mut Self::Movetext, nag: Nag) -> ControlFlow<Self::Output> {
        movetext.tree.node_mut(movetext.current).nags.push(nag.0);
        ControlFlow::Continue(())
    }

    fn comment(
        &mut self,
        movetext: &mut Self::Movetext,
        comment: RawComment<'_>,
    ) -> ControlFlow<Self::Output> {
        let text = String::from_utf8_lossy(comment.as_bytes());
        let text = text.trim();
        let node = movetext.tree.node_mut(movetext.current);
        if !node.comment.is_empty() {
            node.comment.push(' ');
        }
        node.comment.push_str(text);
        ControlFlow::Continue(())
    }

    fn begin_variation(
        &mut self,
        movetext: &mut Self::Movetext,
    ) -> ControlFlow<Self::Output, Skip> {
        // A variation replaces the last move played, so it starts at that move's parent.
        let Some(parent) = movetext.tree.node(movetext.current).parent else {
            return ControlFlow::Continue(Skip(true));
        };
        movetext.stack.push(movetext.current);
        movetext.current = parent;
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, movetext: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        if let Some(previous) = movetext.stack.pop() {
            movetext.current = previous;
        }
        ControlFlow::Continue(())
    }

    fn outcome(
        &mut self,
        movetext: &mut Self::Movetext,
        outcome: Outcome,
    ) -> ControlFlow<Self::Output> {
        if movetext.tree.header("Result").is_none() {
            movetext.tree.set_header("Result", &outcome.to_string());
        }
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, movetext: Self::Movetext) -> Self::Output {
        Ok(movetext.tree)
    }
}

/// Parses every game of a PGN document into trees (including variations).
pub fn parse_pgn(text: &str) -> Result<Vec<GameTree>, PgnError> {
    let mut reader = Reader::new(std::io::Cursor::new(text));
    let mut visitor = TreeVisitor { game: 0 };
    let mut games = Vec::new();
    while let Some(game) = reader.read_game(&mut visitor)? {
        games.push(game?);
    }
    Ok(games)
}

fn write_moves(tree: &GameTree, start: NodeId, out: &mut Vec<String>, mut force_number: bool) {
    let mut current = start;
    loop {
        let children = &tree.node(current).children;
        let Some(&main) = children.first() else {
            return;
        };
        write_move(tree, main, out, force_number);
        force_number = false;
        let has_comment = !tree.node(main).comment.is_empty();
        for &variation in &children[1..] {
            let mut inner = Vec::new();
            write_move(tree, variation, &mut inner, true);
            let continuation_number = !tree.node(variation).comment.is_empty();
            write_moves(tree, variation, &mut inner, continuation_number);
            out.push(format!("({})", inner.join(" ")));
            force_number = true;
        }
        if has_comment {
            force_number = true;
        }
        current = main;
    }
}

fn write_move(tree: &GameTree, id: NodeId, out: &mut Vec<String>, force_number: bool) {
    let node = tree.node(id);
    let white = node.ply % 2 == 1;
    let text = if white {
        format!("{}. {}", node.ply.div_ceil(2), node.san)
    } else if force_number {
        format!("{}... {}", node.ply / 2, node.san)
    } else {
        node.san.clone()
    };
    out.push(text);
    for nag in &node.nags {
        out.push(format!("${nag}"));
    }
    if !node.comment.is_empty() {
        out.push(format!("{{ {} }}", node.comment.replace('}', ")")));
    }
}

/// Writes the whole game, seven-tag roster first.
pub fn write_pgn(tree: &GameTree) -> String {
    let mut out = String::new();
    for name in ["Event", "Site", "Date", "Round", "White", "Black", "Result"] {
        let default = match name {
            "Date" => "????.??.??",
            "Result" => "*",
            _ => "?",
        };
        let value = tree.header(name).unwrap_or(default);
        out.push_str(&format!("[{name} \"{}\"]\n", escape(value)));
    }
    for (key, value) in &tree.headers {
        if !["Event", "Site", "Date", "Round", "White", "Black", "Result"].contains(&key.as_str()) {
            out.push_str(&format!("[{key} \"{}\"]\n", escape(value)));
        }
    }
    out.push('\n');
    let mut tokens = Vec::new();
    let root = tree.node(GameTree::ROOT);
    if !root.comment.is_empty() {
        tokens.push(format!("{{ {} }}", root.comment));
    }
    write_moves(
        tree,
        GameTree::ROOT,
        &mut tokens,
        !root.pos.turn().is_white(),
    );
    tokens.push(tree.header("Result").unwrap_or("*").to_owned());
    out.push_str(&wrap(&tokens));
    out.push('\n');
    out
}

/// Movetext from `id` onward including variations, for "copy PGN from here".
pub fn write_pgn_from(tree: &GameTree, id: NodeId) -> String {
    let mut tokens = Vec::new();
    write_move(tree, id, &mut tokens, true);
    write_moves(tree, id, &mut tokens, false);
    wrap(&tokens)
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn wrap(tokens: &[String]) -> String {
    let mut out = String::new();
    let mut width = 0;
    for token in tokens {
        if width > 0 && width + token.len() + 1 > 80 {
            out.push('\n');
            width = 0;
        } else if width > 0 {
            out.push(' ');
            width += 1;
        }
        out.push_str(token);
        width += token.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANNOTATED: &str = r#"[Event "Club league"]
[White "A"]
[Black "B"]
[Result "1-0"]

{ Opening note } 1. e4 c5 $1 2. Nf3 (2. c3 { Alapin } 2... d5 (2... Nf6 3. e5) 3. exd5) 2... d6 $5
{ Main } 3. d4 1-0
"#;

    #[test]
    fn parses_variations_comments_and_nags() {
        let games = parse_pgn(ANNOTATED).unwrap();
        assert_eq!(games.len(), 1);
        let game = &games[0];
        assert_eq!(game.header("White"), Some("A"));
        assert_eq!(game.node(GameTree::ROOT).comment, "Opening note");
        let main = game.mainline();
        assert_eq!(main.len(), 5);
        assert_eq!(game.node(main[1]).nags, vec![1]);
        assert_eq!(game.node(main[3]).comment, "Main");
        let black_reply = main[1];
        assert_eq!(game.node(black_reply).children.len(), 2);
        let alapin = game.node(black_reply).children[1];
        assert_eq!(game.node(alapin).san, "c3");
        assert_eq!(game.node(alapin).comment, "Alapin");
        let d5 = game.node(alapin).children[0];
        assert_eq!(game.node(d5).san, "d5");
        assert_eq!(game.node(alapin).children.len(), 2);
    }

    #[test]
    fn round_trip_preserves_structure() {
        let first = parse_pgn(ANNOTATED).unwrap().remove(0);
        let written = write_pgn(&first);
        let second = parse_pgn(&written).unwrap().remove(0);
        assert_eq!(write_pgn(&second), written);
        assert_eq!(first.mainline().len(), second.mainline().len());
        assert!(written.contains("(2. c3 { Alapin } 2... d5 (2... Nf6 3. e5) 3. exd5)"));
    }

    #[test]
    fn round_trip_of_multiple_games_and_custom_start() {
        let text = "[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 Kd7 *\n\n1. d4 d5 *\n";
        let games = parse_pgn(text).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].node(games[0].mainline()[1]).san, "Kd7");
        let again = parse_pgn(&write_pgn(&games[0])).unwrap().remove(0);
        assert_eq!(again.header("FEN"), Some("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1"));
    }

    #[test]
    fn black_to_move_start_writes_ellipsis() {
        let mut game = GameTree::from_fen("4k3/8/8/8/8/8/4P3/4K3 b - - 0 1").unwrap();
        game.add_san(GameTree::ROOT, "Kd7").unwrap();
        assert!(write_pgn(&game).contains("1... Kd7"));
    }

    #[test]
    fn illegal_move_reports_game_number() {
        let err = parse_pgn("1. e4 *\n\n1. e5 *\n").unwrap_err();
        assert!(matches!(err, PgnError::Move { game: 2, .. }), "{err}");
    }

    #[test]
    fn write_from_includes_following_variations() {
        let game = parse_pgn("1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *")
            .unwrap()
            .remove(0);
        let e4 = game.mainline()[0];
        assert_eq!(
            write_pgn_from(&game, e4),
            "1. e4 e5 (1... c5 2. Nf3) 2. Nf3"
        );
    }

    #[test]
    fn chess960_round_trips_with_its_variant_tag() {
        let text = "[Variant \"Fischerandom\"]\n[FEN \"bqnbrkrn/pppppppp/8/8/8/8/PPPPPPPP/BQNBRKRN w KQkq - 0 1\"]\n\n1. Ng3 Ng6 2. O-O O-O *\n";
        let game = parse_pgn(text).unwrap().remove(0);
        assert_eq!(game.variant, crate::Variant::Chess960);
        let written = write_pgn(&game);
        assert!(written.contains("[Variant \"Chess960\"]"), "{written}");
        assert!(written.contains("[SetUp \"1\"]"));
        let again = parse_pgn(&written).unwrap().remove(0);
        assert_eq!(again.variant, crate::Variant::Chess960);
        assert_eq!(again.mainline().len(), 4);
        assert_eq!(write_pgn(&again), written);
        // A new Chess960 game carries its tags from the start.
        let fresh = GameTree::chess960(734);
        assert!(write_pgn(&fresh).contains("[Variant \"Chess960\"]"));
    }

    #[test]
    fn sample_game_is_legal() {
        let game = crate::sample::najdorf();
        assert_eq!(game.mainline().len(), 26);
        assert_eq!(game.move_label(*game.mainline().last().unwrap()), "13…Ne8");
    }
}
