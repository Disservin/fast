use shakmaty::{
    CastlingMode, Chess, EnPassantMode, Move, Position,
    fen::Fen,
    san::{San, SanPlus},
    zobrist::Zobrist64,
};

pub type NodeId = usize;

/// Hash identifying a position for repertoire, note and database lookups.
pub fn position_hash(pos: &Chess) -> u64 {
    let hash: Zobrist64 = pos.zobrist_hash(EnPassantMode::Legal);
    hash.0
}

/// One position in the game tree. `children[0]` is the main continuation.
#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Move that led here (`None` only for the root).
    pub mv: Option<Move>,
    pub san: String,
    pub pos: Chess,
    pub ply: u32,
    pub comment: String,
    pub nags: Vec<u8>,
}

/// A game: headers plus an arena-allocated tree of positions.
#[derive(Clone, Debug)]
pub struct GameTree {
    nodes: Vec<Node>,
    pub headers: Vec<(String, String)>,
    /// Standard chess or Chess960; every position follows its castling rules.
    pub variant: crate::Variant,
}

impl Default for GameTree {
    fn default() -> Self {
        Self::new()
    }
}

impl GameTree {
    pub const ROOT: NodeId = 0;

    pub fn new() -> Self {
        Self::from_position(Chess::default())
    }

    pub fn from_position(pos: Chess) -> Self {
        let variant = if pos.castles().mode() == CastlingMode::Chess960 {
            crate::Variant::Chess960
        } else {
            crate::Variant::Standard
        };
        let root = Node {
            parent: None,
            children: Vec::new(),
            mv: None,
            san: String::new(),
            ply: (pos.fullmoves().get() - 1) * 2 + u32::from(pos.turn().is_black()),
            pos,
            comment: String::new(),
            nags: Vec::new(),
        };
        let mut tree = Self {
            nodes: vec![root],
            headers: Vec::new(),
            variant,
        };
        if variant == crate::Variant::Chess960 {
            tree.set_header("Variant", "Chess960");
        }
        if variant == crate::Variant::Chess960 || tree.node(Self::ROOT).pos != Chess::default() {
            tree.set_header("SetUp", "1");
            let fen = tree.fen(Self::ROOT);
            tree.set_header("FEN", &fen);
        }
        tree
    }

    /// Builds a tree rooted at the given FEN, read as Chess960 when its castling
    /// rights only make sense there (X-FEN or Shredder-FEN). A FEN not starting from the
    /// initial position is preserved in the `SetUp`/`FEN` headers when written back to PGN.
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let variant = crate::fen_variant(fen).unwrap_or_default();
        Self::from_fen_variant(fen, variant)
    }

    /// Builds a tree rooted at the given FEN with the castling rules of `variant`.
    pub fn from_fen_variant(fen: &str, variant: crate::Variant) -> Result<Self, String> {
        let parsed: Fen = fen.parse().map_err(|err| format!("{err}"))?;
        let pos: Chess = parsed
            .into_position(variant.castling_mode())
            .map_err(|err| format!("{err}"))?;
        let mut tree = Self::from_position(pos);
        if fen != Self::fen_of(&Chess::default()) || variant == crate::Variant::Chess960 {
            tree.set_header("SetUp", "1");
            tree.set_header("FEN", fen);
        }
        Ok(tree)
    }

    /// A new game from Chess960 position `number` (0–959).
    pub fn chess960(number: u16) -> Self {
        let pos = crate::chess960_position(number);
        let fen = Self::fen_of(&pos);
        let mut tree = Self::from_position(pos);
        tree.set_header("SetUp", "1");
        tree.set_header("FEN", &fen);
        tree
    }

    /// A move in UCI notation for this game: castling is king-takes-rook in Chess960
    /// and the king's two-square step otherwise.
    pub fn uci(&self, mv: Move) -> String {
        shakmaty::uci::UciMove::from_move(mv, self.variant.castling_mode()).to_string()
    }

    pub fn fen_of(pos: &Chess) -> String {
        Fen::from_position(pos, EnPassantMode::Legal).to_string()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn set_header(&mut self, name: &str, value: &str) {
        if let Some(entry) = self.headers.iter_mut().find(|(key, _)| key == name) {
            entry.1 = value.to_owned();
        } else {
            self.headers.push((name.to_owned(), value.to_owned()));
        }
    }

    /// Plays `mv` from `parent`, reusing an existing child with the same move.
    /// The caller guarantees the move is legal in the parent's position.
    pub fn add_move(&mut self, parent: NodeId, mv: Move) -> NodeId {
        if let Some(&existing) = self.nodes[parent]
            .children
            .iter()
            .find(|&&child| self.nodes[child].mv.as_ref() == Some(&mv))
        {
            return existing;
        }
        let mut pos = self.nodes[parent].pos.clone();
        let san = SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string();
        let id = self.nodes.len();
        let ply = self.nodes[parent].ply + 1;
        self.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            mv: Some(mv),
            san,
            pos,
            ply,
            comment: String::new(),
            nags: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        id
    }

    /// Plays a SAN string from `parent`.
    pub fn add_san(&mut self, parent: NodeId, san: &str) -> Result<NodeId, String> {
        let parsed: San = san.parse().map_err(|err| format!("{san}: {err}"))?;
        let mv = parsed
            .to_move(&self.nodes[parent].pos)
            .map_err(|err| format!("{san}: {err}"))?;
        Ok(self.add_move(parent, mv))
    }

    /// Detaches `id` and everything after it. Returns the node to select next.
    pub fn delete_from(&mut self, id: NodeId) -> NodeId {
        let Some(parent) = self.nodes[id].parent else {
            return id;
        };
        self.nodes[parent].children.retain(|&child| child != id);
        parent
    }

    /// Removes every sideline, preserving node ids and mainline annotations.
    /// Returns the closest surviving ancestor of the selected position.
    pub fn delete_all_sidelines(&mut self, selected: NodeId) -> NodeId {
        let mut surviving = Self::ROOT;
        for id in self.path_to(selected) {
            if self.nodes[surviving].children.first() != Some(&id) {
                break;
            }
            surviving = id;
        }
        let mut current = Self::ROOT;
        loop {
            self.nodes[current].children.truncate(1);
            let Some(&next) = self.nodes[current].children.first() else {
                break;
            };
            current = next;
        }
        surviving
    }

    /// Moves the variation containing `id` one rank closer to the main line.
    pub fn promote(&mut self, id: NodeId) -> bool {
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            let siblings = &mut self.nodes[parent].children;
            if let Some(index) = siblings.iter().position(|&sibling| sibling == current)
                && index > 0
            {
                siblings.swap(index, index - 1);
                return true;
            }
            current = parent;
        }
        false
    }

    /// Nodes from the root (exclusive) to `id` (inclusive).
    pub fn path_to(&self, id: NodeId) -> Vec<NodeId> {
        let mut path = Vec::new();
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            path.push(current);
            current = parent;
        }
        path.reverse();
        path
    }

    /// The main line after the root, as node ids.
    pub fn mainline(&self) -> Vec<NodeId> {
        self.line_from(Self::ROOT)
    }

    /// Follows first children from `id` (exclusive).
    pub fn line_from(&self, id: NodeId) -> Vec<NodeId> {
        let mut line = Vec::new();
        let mut current = id;
        while let Some(&next) = self.nodes[current].children.first() {
            line.push(next);
            current = next;
        }
        line
    }

    /// The full line through `id`: its ancestors, then first children to the end.
    pub fn line_through(&self, id: NodeId) -> Vec<NodeId> {
        let mut line = self.path_to(id);
        line.extend(self.line_from(id));
        line
    }

    pub fn is_on_mainline(&self, id: NodeId) -> bool {
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            if self.nodes[parent].children.first() != Some(&current) {
                return false;
            }
            current = parent;
        }
        true
    }

    /// Toggles a NAG. Move-quality NAGs (1–6) are mutually exclusive.
    pub fn toggle_nag(&mut self, id: NodeId, nag: u8) {
        let nags = &mut self.nodes[id].nags;
        if let Some(index) = nags.iter().position(|&existing| existing == nag) {
            nags.remove(index);
            return;
        }
        if (1..=6).contains(&nag) {
            nags.retain(|existing| !(1..=6).contains(existing));
        }
        nags.push(nag);
        nags.sort_unstable();
    }

    pub fn fen(&self, id: NodeId) -> String {
        Self::fen_of(&self.nodes[id].pos)
    }

    /// "13…Ne8" / "14.f4" label for a node, ignoring annotations.
    pub fn move_label(&self, id: NodeId) -> String {
        let node = &self.nodes[id];
        crate::notation::numbered(node.ply, &node.san)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(moves: &[&str]) -> (GameTree, Vec<NodeId>) {
        let mut tree = GameTree::new();
        let mut ids = Vec::new();
        let mut current = GameTree::ROOT;
        for san in moves {
            current = tree.add_san(current, san).unwrap();
            ids.push(current);
        }
        (tree, ids)
    }

    #[test]
    fn chess960_castling_with_king_and_rook_side_by_side() {
        // King on f1 next to its rook on g1 (and on e1 for the long side).
        let fen = "bqnbrkrn/pppppppp/8/8/8/8/PPPPPPPP/BQNBRKRN w GEge - 0 1";
        let mut game = GameTree::from_fen(fen).unwrap();
        assert_eq!(game.variant, crate::Variant::Chess960);
        assert_eq!(game.header("Variant"), Some("Chess960"));
        // Clear the squares between: knight away, then short castling.
        let n = game.add_san(GameTree::ROOT, "Ng3").unwrap();
        let reply = game.add_san(n, "Ng6").unwrap();
        let castle = game.add_san(reply, "O-O").unwrap();
        let mv = game.node(castle).mv.unwrap();
        assert_eq!(game.uci(mv), "f1g1");
        let pos = &game.node(castle).pos;
        assert_eq!(
            pos.board().piece_at(shakmaty::Square::G1).map(|p| p.role),
            Some(shakmaty::Role::King)
        );
        assert_eq!(
            pos.board().piece_at(shakmaty::Square::F1).map(|p| p.role),
            Some(shakmaty::Role::Rook)
        );
        // Engines answer castling as king-takes-rook.
        let parsed = "f1g1"
            .parse::<shakmaty::uci::UciMove>()
            .unwrap()
            .to_move(&game.node(reply).pos)
            .unwrap();
        assert_eq!(parsed, mv);
    }

    #[test]
    fn chess960_castling_with_the_king_already_on_its_castled_square() {
        // King on g1 with the rook on h1: castling short moves only the rook.
        let fen = "4k3/8/8/8/8/8/8/R5KR w HA - 0 1";
        let mut game = GameTree::from_fen(fen).unwrap();
        assert_eq!(game.variant, crate::Variant::Chess960);
        let castle = game.add_san(GameTree::ROOT, "O-O").unwrap();
        let pos = &game.node(castle).pos;
        assert_eq!(
            pos.board().piece_at(shakmaty::Square::G1).map(|p| p.role),
            Some(shakmaty::Role::King)
        );
        assert_eq!(
            pos.board().piece_at(shakmaty::Square::F1).map(|p| p.role),
            Some(shakmaty::Role::Rook)
        );
        assert_eq!(game.uci(game.node(castle).mv.unwrap()), "g1h1");
    }

    #[test]
    fn standard_games_keep_the_two_square_castling_step() {
        let (game, ids) = tree(&["e4", "e5", "Nf3", "Nc6", "Bc4", "Bc5", "O-O"]);
        assert_eq!(game.variant, crate::Variant::Standard);
        assert_eq!(game.uci(game.node(ids[6]).mv.unwrap()), "e1g1");
        assert!(game.header("Variant").is_none());
    }

    #[test]
    fn add_move_reuses_existing_child() {
        let (mut game, ids) = tree(&["e4", "c5"]);
        assert_eq!(game.add_san(ids[0], "c5").unwrap(), ids[1]);
        assert_eq!(game.node(ids[0]).children.len(), 1);
    }

    #[test]
    fn variations_promote_and_delete() {
        let (mut game, ids) = tree(&["e4", "c5", "Nf3"]);
        let var = game.add_san(ids[0], "e5").unwrap();
        assert!(!game.is_on_mainline(var));
        assert!(game.promote(var));
        assert!(game.is_on_mainline(var));
        assert!(!game.is_on_mainline(ids[1]));
        assert_eq!(game.mainline(), vec![ids[0], var]);
        assert_eq!(game.delete_from(var), ids[0]);
        assert_eq!(game.mainline(), vec![ids[0], ids[1], ids[2]]);
    }

    #[test]
    fn promote_walks_up_from_inside_a_variation() {
        let (mut game, ids) = tree(&["e4", "c5"]);
        let var = game.add_san(ids[0], "e5").unwrap();
        let deep = game.add_san(var, "Nf3").unwrap();
        assert!(game.promote(deep));
        assert!(game.is_on_mainline(deep));
        assert!(!game.promote(deep));
    }

    #[test]
    fn delete_all_sidelines_preserves_mainline_and_selects_surviving_ancestor() {
        let (mut game, ids) = tree(&["e4", "e5", "Nf3"]);
        let alternative = game.add_san(GameTree::ROOT, "d4").unwrap();
        let variation = game.add_san(ids[0], "c5").unwrap();
        let nested = game.add_san(variation, "Nf3").unwrap();
        game.node_mut(ids[2]).comment = "keep".into();
        assert_eq!(game.delete_all_sidelines(nested), ids[0]);
        assert_eq!(game.mainline(), ids);
        assert_eq!(game.node(GameTree::ROOT).children, vec![ids[0]]);
        assert_eq!(game.node(ids[0]).children, vec![ids[1]]);
        assert_eq!(game.node(ids[2]).comment, "keep");
        assert!(!game.node(GameTree::ROOT).children.contains(&alternative));
        assert_eq!(game.delete_all_sidelines(ids[2]), ids[2]);
    }

    #[test]
    fn quality_nags_are_exclusive_but_evaluation_nags_stack() {
        let (mut game, ids) = tree(&["e4"]);
        game.toggle_nag(ids[0], 1);
        game.toggle_nag(ids[0], 5);
        game.toggle_nag(ids[0], 10);
        assert_eq!(game.node(ids[0]).nags, vec![5, 10]);
        game.toggle_nag(ids[0], 5);
        assert_eq!(game.node(ids[0]).nags, vec![10]);
    }

    #[test]
    fn transpositions_share_a_hash() {
        let (first, a) = tree(&["Nf3", "Nf6", "Nc3"]);
        let (second, b) = tree(&["Nc3", "Nf6", "Nf3"]);
        assert_eq!(
            position_hash(&first.node(a[2]).pos),
            position_hash(&second.node(b[2]).pos)
        );
    }

    #[test]
    fn labels_use_move_numbers() {
        let (game, ids) = tree(&["e4", "c5"]);
        assert_eq!(game.move_label(ids[0]), "1.e4");
        assert_eq!(game.move_label(ids[1]), "1…c5");
    }

    #[test]
    fn from_fen_keeps_setup_headers() {
        let fen = "8/8/8/8/8/4k3/4p3/4K3 w - - 0 60";
        let game = GameTree::from_fen(fen).unwrap();
        assert_eq!(game.header("FEN"), Some(fen));
        assert!(GameTree::from_fen("not a fen").is_err());
    }
}
