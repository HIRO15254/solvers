//! Everything a viewer reads about a solved tree besides its strategies:
//! the compiled tree plus [`GameIndex`] (action labels, dealt cards, pots,
//! streets, parents, hand supports and root ranges). A live solve derives
//! the index from its builder metadata; a `.sol` artifact stores the tree
//! and the index, so reading one never rebuilds the tree.
//!
//! The encoded index (one stream inside the artifact's index frame, all
//! integers little-endian):
//!
//! ```text
//! counts      u64 nodes, u64 deals, u32 masks, u32 transitions, u32 labels,
//!             u32 root dimension per player
//! kinds       u8 per node: 0 = P0 action, 1 = P1 action, 2 = chance, 3 = terminal
//! children    u16 per non-terminal node
//! terminals   u32 per terminal: its id minus the previous terminal's (wrapping)
//! deals       f32 weight per deal, then u32 reach map per deal and player
//!             (bits 30-31: 0 identity, 1 mask, 2 transition; bits 0-29: index)
//! deal cards  u8 per deal: card index, or 255 where no card is recorded
//! masks       u32 length + f32 entries, per mask
//! transitions u32 in_dim, u32 out_dim, u32 entries + (u32, u32, f32) entries
//! labels      u32 byte length + UTF-8 display label (`check`, `bet 2.5`)
//! edges       u32 label per child of an action node, action nodes by id
//! pots        u32 pot in chips per action node, by storage ref
//! ```

use std::collections::HashMap;
use std::io::{self, Read, Write};

use hu_engine::{
    CompiledGame, Deal, NodeId, NodeKind, PublicTree, ReachMap, ShapeNode, SparseTransition,
    TreeShape, parent_array,
};
use nlh::{Card, PerPlayer, Player, Street};

use crate::queries::history_token_matches;
use crate::sol::SolError;
use crate::{PostflopConfig, PostflopEvaluator, PostflopHands, PostflopNodeInfo, node_streets};

const NO_LABEL: u32 = u32::MAX;
const NO_CARD: u8 = u8::MAX;
/// Bytes buffered per column write.
const WRITE_BUFFER: usize = 1 << 16;
const MAP_INDEX_BITS: u32 = 30;

/// Viewer annotations of a compiled postflop tree, indexed by its node ids,
/// deals and storage refs.
pub struct GameIndex {
    /// Display action labels, interned.
    labels: Vec<String>,
    /// History token of each label (`x`, `r2.5`).
    tokens: Vec<String>,
    /// Table scale used to compare equivalent selector amounts.
    scale: nlh::ChipScale,
    /// Table rounding mode for BB selector amounts.
    rounding: nlh::Rounding,
    /// Label of the action leading into each node; [`NO_LABEL`] at the root
    /// and below chance nodes.
    edges: Vec<u32>,
    /// Card each chance branch deals, in [`PublicTree::deals`] order. An
    /// iso-merged branch records its representative card when an action node
    /// follows it; a branch into an all-in runout records [`NO_CARD`].
    deal_cards: Vec<u8>,
    /// Pot in chips at each action node, by storage ref.
    pots: Vec<u32>,
    pub streets: Vec<Street>,
    pub parents: Vec<NodeId>,
    pub hands: PostflopHands,
    pub root_ranges: PerPlayer<Vec<f32>>,
}

/// Interns display labels by their builder spelling.
#[derive(Default)]
struct Labels<'a> {
    ids: HashMap<&'a str, u32>,
    labels: Vec<String>,
}

impl<'a> Labels<'a> {
    fn intern(&mut self, raw: &'a str, scale: nlh::ChipScale) -> u32 {
        if let Some(&id) = self.ids.get(raw) {
            return id;
        }
        let id = self.labels.len() as u32;
        self.labels.push(display_label(raw, scale));
        self.ids.insert(raw, id);
        id
    }
}

/// Builder labels name chip amounts; displays use the table's canonical spelling.
fn display_label(raw: &str, scale: nlh::ChipScale) -> String {
    for prefix in ["bet ", "raise to "] {
        if let Some(amount) = raw.strip_prefix(prefix) {
            return format!(
                "{prefix}{}",
                crate::prepare::bb(amount.parse().expect("builder amount"), scale)
            );
        }
    }
    raw.to_string()
}

/// The card inside the last `[Xy]` token of a builder history.
fn last_bracketed_card(history: &str) -> Option<Card> {
    let start = history.rfind('[')?;
    let end = history[start..].find(']')? + start;
    history[start + 1..end].parse().ok()
}

impl GameIndex {
    /// Heap bytes allocated by the index, by capacity.
    pub fn heap_bytes(&self) -> u64 {
        use crate::postflop::vec_bytes;
        let strings =
            |v: &Vec<String>| vec_bytes(v) + v.iter().map(|s| s.capacity() as u64).sum::<u64>();
        strings(&self.labels)
            + strings(&self.tokens)
            + vec_bytes(&self.edges)
            + vec_bytes(&self.deal_cards)
            + vec_bytes(&self.pots)
            + vec_bytes(&self.streets)
            + vec_bytes(&self.parents)
            + self.hands.heap_bytes()
            + vec_bytes(&self.root_ranges[Player::P0])
            + vec_bytes(&self.root_ranges[Player::P1])
    }

    /// Index of a freshly built game. `node_info` is the builder's metadata
    /// (chip amounts, untranslated); `start` is the subgame's first street.
    pub fn from_game(
        game: &CompiledGame<PostflopEvaluator>,
        node_info: &[PostflopNodeInfo],
        start: Street,
        scale: nlh::ChipScale,
        rounding: nlh::Rounding,
    ) -> Self {
        let tree = &game.tree;
        let mut labels = Labels::default();
        let mut edges = vec![NO_LABEL; tree.nodes.len()];
        let mut pots = vec![0; tree.storage_refs.len()];
        let mut deal_cards = vec![NO_CARD; tree.deals.len()];
        for (id, node) in tree.nodes.iter().enumerate() {
            let id = id as NodeId;
            match node.kind {
                NodeKind::Action => {
                    let info = &node_info[tree.tags[id as usize] as usize];
                    assert_eq!(
                        info.actions.len(),
                        node.num_children as usize,
                        "indexing a game built without node info"
                    );
                    pots[node.aux as usize] =
                        (info.contrib[Player::P0] + info.contrib[Player::P1]).0;
                    for (child, label) in tree.children(id).zip(&info.actions) {
                        edges[child as usize] = labels.intern(label, scale);
                    }
                }
                NodeKind::Chance => {
                    for (pos, child) in tree.children(id).enumerate() {
                        let card = match tree.deal(node, pos).maps[Player::P0] {
                            ReachMap::Mask(m) => Some(game.evaluator.mask_cards[m as usize]),
                            ReachMap::Transition(_)
                                if tree.node(child).kind == NodeKind::Action =>
                            {
                                let info = &node_info[tree.tags[child as usize] as usize];
                                last_bracketed_card(&info.history)
                            }
                            _ => None,
                        };
                        deal_cards[node.aux as usize + pos] =
                            card.map_or(NO_CARD, |c| c.index() as u8);
                    }
                }
                NodeKind::Terminal => {}
            }
        }
        Self::assemble(
            tree,
            labels.labels,
            edges,
            deal_cards,
            pots,
            start,
            game.evaluator.hands.clone(),
            game.root_ranges.clone(),
            scale,
            rounding,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        tree: &PublicTree,
        labels: Vec<String>,
        edges: Vec<u32>,
        deal_cards: Vec<u8>,
        pots: Vec<u32>,
        start: Street,
        hands: PostflopHands,
        root_ranges: PerPlayer<Vec<f32>>,
        scale: nlh::ChipScale,
        rounding: nlh::Rounding,
    ) -> Self {
        GameIndex {
            scale,
            rounding,
            tokens: labels
                .iter()
                .map(|l| crate::queries::history_token(l))
                .collect(),
            labels,
            edges,
            deal_cards,
            pots,
            streets: node_streets(tree, start),
            parents: parent_array(tree),
            hands,
            root_ranges,
        }
    }
}

/// A compiled tree with its index, as a loaded artifact holds them.
pub struct IndexedTree {
    pub tree: PublicTree,
    pub index: GameIndex,
}

impl IndexedTree {
    pub fn view(&self) -> TreeView<'_> {
        TreeView {
            tree: &self.tree,
            index: &self.index,
        }
    }
}

/// Read-only navigation of a tree and its index.
#[derive(Clone, Copy)]
pub struct TreeView<'a> {
    pub tree: &'a PublicTree,
    pub index: &'a GameIndex,
}

impl<'a> TreeView<'a> {
    pub fn hands(&self) -> &'a PostflopHands {
        &self.index.hands
    }

    pub fn root_ranges(&self) -> &'a PerPlayer<Vec<f32>> {
        &self.index.root_ranges
    }

    pub fn street(&self, node: NodeId) -> Street {
        self.index.streets[node as usize]
    }

    /// Display labels of an action node's actions, in child order.
    pub fn actions(&self, node: NodeId) -> Vec<&'a str> {
        self.action_labels(node).collect()
    }

    pub(crate) fn resolve_action(&self, node: NodeId, label: &str) -> Result<usize, String> {
        crate::queries::resolve_scaled_action(
            &self.actions(node),
            label,
            self.index.scale,
            self.index.rounding,
        )
    }

    pub(crate) fn action_labels(&self, node: NodeId) -> impl Iterator<Item = &'a str> + '_ {
        self.tree
            .children(node)
            .map(|child| self.index.labels[self.index.edges[child as usize] as usize].as_str())
    }

    /// Pot in chips at an action node, the starting pot included.
    pub fn pot(&self, node: NodeId) -> u32 {
        self.index.pots[self.tree.node(node).aux as usize]
    }

    /// Card a chance node's `pos`-th branch deals; `None` for a branch into
    /// an all-in runout that merges several cards.
    pub(crate) fn deal_card(&self, node: NodeId, pos: usize) -> Option<Card> {
        let card = self.index.deal_cards[self.tree.node(node).aux as usize + pos];
        (card != NO_CARD).then(|| Card::from_index(card))
    }

    /// Betting-line history using the table's canonical amount spelling; the
    /// root is the empty string.
    pub fn history(&self, node: NodeId) -> String {
        let mut steps = Vec::new();
        let mut current = node;
        loop {
            let parent = self.index.parents[current as usize];
            if parent == NodeId::MAX {
                break;
            }
            steps.push((parent, current));
            current = parent;
        }
        let mut history = String::new();
        for &(parent, child) in steps.iter().rev() {
            let p = self.tree.node(parent);
            if p.kind == NodeKind::Action {
                history.push_str(&self.index.tokens[self.index.edges[child as usize] as usize]);
            } else {
                let pos = (child - p.first_child) as usize;
                match self.deal_card(parent, pos) {
                    Some(card) => history.push_str(&format!("[{card}]")),
                    None => history.push_str(&format!("[deal{pos}]")),
                }
            }
        }
        history
    }

    /// The action node a history names, walking from the root.
    pub fn node_by_history(&self, history: &str) -> Option<NodeId> {
        self.node_by_history_any(history)
            .filter(|&id| self.tree.node(id).kind == NodeKind::Action)
    }

    /// A node of any kind named by history, including chance and terminal nodes.
    /// Cardless runout branches use `[dealN]`, where N is the child position.
    pub fn node_by_history_any(&self, history: &str) -> Option<NodeId> {
        crate::queries::selector_history_steps(history, self.index.scale)
            .iter()
            .find_map(|steps| self.walk_history(steps))
    }

    fn walk_history(&self, steps: &[&str]) -> Option<NodeId> {
        let mut current: NodeId = 0;
        for &token in steps {
            let node = self.tree.node(current);
            current = match node.kind {
                NodeKind::Action => self.tree.children(current).find(|&child| {
                    history_token_matches(
                        &self.index.tokens[self.index.edges[child as usize] as usize],
                        token,
                        self.index.scale,
                        self.index.rounding,
                    )
                })?,
                NodeKind::Chance => {
                    let dealt = token.strip_prefix('[')?.strip_suffix(']')?;
                    let pos = if let Some(position) = dealt.strip_prefix("deal") {
                        let pos: usize = position.parse().ok()?;
                        if pos >= node.num_children as usize
                            || self.deal_card(current, pos).is_some()
                        {
                            return None;
                        }
                        pos
                    } else {
                        let card: Card = dealt.parse().ok()?;
                        (0..node.num_children as usize)
                            .find(|&pos| self.deal_card(current, pos) == Some(card))?
                    };
                    node.first_child + pos as NodeId
                }
                NodeKind::Terminal => return None,
            };
        }
        Some(current)
    }

    /// Resolve a profile selector to action nodes. `[?]` in a history, or
    /// `?` in a slash-separated label path, expands every chance branch.
    pub fn profile_nodes(&self, selector: &str) -> Result<Vec<NodeId>, spot::SpotError> {
        let error = || {
            spot::SpotError::new(
                spot::Code::NLH003,
                "profile.node",
                format!("unknown or non-action node selector {selector:?}"),
            )
        };
        if selector.is_empty() || selector == "root" {
            return (self.tree.node(0).kind == NodeKind::Action)
                .then_some(vec![0])
                .ok_or_else(error);
        }
        let history = selector.contains("[?]") || self.node_by_history_any(selector).is_some();
        if history {
            return crate::queries::selector_history_steps(selector, self.index.scale)
                .iter()
                .find_map(|steps| self.walk_profile(steps, true))
                .ok_or_else(error);
        }
        let steps: Vec<_> = selector
            .split('/')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        self.walk_profile(&steps, false).ok_or_else(error)
    }

    fn walk_profile(&self, steps: &[&str], history: bool) -> Option<Vec<NodeId>> {
        let mut current = vec![0];
        for &step in steps {
            let mut next = Vec::new();
            for id in current {
                let node = self.tree.node(id);
                match node.kind {
                    NodeKind::Action => {
                        if history {
                            next.extend(self.tree.children(id).filter(|&child| {
                                history_token_matches(
                                    &self.index.tokens[self.index.edges[child as usize] as usize],
                                    step,
                                    self.index.scale,
                                    self.index.rounding,
                                )
                            }));
                        } else if let Ok(pos) = self.resolve_action(id, step) {
                            next.push(node.first_child + pos as NodeId);
                        }
                    }
                    NodeKind::Chance => {
                        for pos in 0..node.num_children as usize {
                            let label = self.chance_label(id, pos);
                            let wanted = if history {
                                step.strip_prefix('[')
                                    .and_then(|s| s.strip_suffix(']'))
                                    .unwrap_or(step)
                            } else {
                                step
                            };
                            if wanted == "?"
                                || label
                                    .trim_end_matches('*')
                                    .eq_ignore_ascii_case(wanted.trim_end_matches('*'))
                            {
                                next.push(node.first_child + pos as NodeId);
                            }
                        }
                    }
                    NodeKind::Terminal => {}
                }
            }
            if next.is_empty() {
                return None;
            }
            current = next;
        }
        if current.is_empty()
            || current
                .iter()
                .any(|&id| self.tree.node(id).kind != NodeKind::Action)
        {
            return None;
        }
        Some(current)
    }

    /// The board represented by this node, including representative runout
    /// cards on iso-merged branches.
    pub fn profile_board(&self, node: NodeId, starting_board: &[Card]) -> Vec<Card> {
        let mut board = starting_board.to_vec();
        let mut current = node;
        let mut dealt = Vec::new();
        loop {
            let parent = self.index.parents[current as usize];
            if parent == NodeId::MAX {
                break;
            }
            let p = self.tree.node(parent);
            if p.kind == NodeKind::Chance
                && let Some(card) = self.deal_card(parent, (current - p.first_child) as usize)
            {
                dealt.push(card);
            }
            current = parent;
        }
        board.extend(dealt.into_iter().rev());
        board
    }

    /// Select board-live hands in the acting player's compact support.
    pub fn profile_hand_mask(
        &self,
        node: NodeId,
        starting_board: &[Card],
        selector: &spot::HandSelector,
    ) -> Result<Vec<bool>, spot::SpotError> {
        let error =
            |message: String| spot::SpotError::new(spot::Code::NLH003, "profile.hands", message);
        let board = self.profile_board(node, starting_board);
        let range = match selector {
            spot::HandSelector::Range(text) => Some(
                text.parse::<nlh::Range>()
                    .map_err(|e| error(e.to_string()))?,
            ),
            _ => None,
        };
        let category = match selector {
            spot::HandSelector::Category(text) => {
                Some(nlh::category::CategoryExpr::parse(text).map_err(|e| error(e.to_string()))?)
            }
            _ => None,
        };
        let actor = self.tree.node(node).player;
        Ok(self
            .hands()
            .combos(actor)
            .iter()
            .map(|&combo| {
                let (a, b) = nlh::combo_cards(combo as usize);
                !board.contains(&a)
                    && !board.contains(&b)
                    && range
                        .as_ref()
                        .is_none_or(|r| r.weight(combo as usize) > 0.0)
                    && category.as_ref().is_none_or(|c| c.matches(&board, [a, b]))
            })
            .collect())
    }

    /// Label of a chance node's `pos`-th branch: its card, `*`-suffixed for
    /// an iso-merged class, or `dealN` when no card is recorded.
    pub fn chance_label(&self, node: NodeId, pos: usize) -> String {
        let n = self.tree.node(node);
        match (
            self.tree.deal(n, pos).maps[Player::P0],
            self.deal_card(node, pos),
        ) {
            (ReachMap::Mask(_), Some(card)) => card.to_string(),
            (ReachMap::Transition(_), Some(card)) => format!("{card}*"),
            _ => format!("deal{pos}"),
        }
    }
}

// --- codec -------------------------------------------------------------------

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Writes `items` through a bounded buffer.
fn put_column<T>(
    out: &mut dyn Write,
    items: impl IntoIterator<Item = T>,
    mut put: impl FnMut(&mut Vec<u8>, T) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = Vec::with_capacity(WRITE_BUFFER + 16);
    for item in items {
        put(&mut buf, item)?;
        if buf.len() >= WRITE_BUFFER {
            out.write_all(&buf)?;
            buf.clear();
        }
    }
    out.write_all(&buf)
}

fn encode_map(map: ReachMap) -> io::Result<u32> {
    let (kind, index) = match map {
        ReachMap::Identity => (0, 0),
        ReachMap::Mask(m) => (1, m),
        ReachMap::Transition(t) => (2, t),
    };
    if index >> MAP_INDEX_BITS != 0 {
        return Err(invalid("reach map index exceeds the index format"));
    }
    Ok(kind << MAP_INDEX_BITS | index)
}

/// Encodes `tree` and `index` as the artifact's index stream.
pub(crate) fn write_index(
    out: &mut dyn Write,
    tree: &PublicTree,
    index: &GameIndex,
) -> io::Result<()> {
    let u32_of = |n: usize| u32::try_from(n).map_err(|_| invalid("count exceeds u32"));
    let mut head = Vec::new();
    head.extend_from_slice(&(tree.nodes.len() as u64).to_le_bytes());
    head.extend_from_slice(&(tree.deals.len() as u64).to_le_bytes());
    head.extend_from_slice(&u32_of(tree.masks.len())?.to_le_bytes());
    head.extend_from_slice(&u32_of(tree.transitions.len())?.to_le_bytes());
    head.extend_from_slice(&u32_of(index.labels.len())?.to_le_bytes());
    head.extend_from_slice(&tree.root_dims[Player::P0].to_le_bytes());
    head.extend_from_slice(&tree.root_dims[Player::P1].to_le_bytes());
    out.write_all(&head)?;

    put_column(out, &tree.nodes, |buf, node| {
        buf.push(match node.kind {
            NodeKind::Action => node.player.index() as u8,
            NodeKind::Chance => 2,
            NodeKind::Terminal => 3,
        });
        Ok(())
    })?;
    put_column(
        out,
        tree.nodes.iter().filter(|n| n.kind != NodeKind::Terminal),
        |buf, node| {
            buf.extend_from_slice(&node.num_children.to_le_bytes());
            Ok(())
        },
    )?;
    let mut previous = 0u32;
    put_column(
        out,
        tree.nodes.iter().filter(|n| n.kind == NodeKind::Terminal),
        |buf, node| {
            buf.extend_from_slice(&node.aux.wrapping_sub(previous).to_le_bytes());
            previous = node.aux;
            Ok(())
        },
    )?;
    put_column(out, &tree.deals, |buf, deal| {
        buf.extend_from_slice(&deal.weight.to_le_bytes());
        Ok(())
    })?;
    put_column(out, &tree.deals, |buf, deal| {
        for player in Player::BOTH {
            buf.extend_from_slice(&encode_map(deal.maps[player])?.to_le_bytes());
        }
        Ok(())
    })?;
    out.write_all(&index.deal_cards)?;
    for mask in &tree.masks {
        out.write_all(&u32_of(mask.len())?.to_le_bytes())?;
        put_column(out, mask, |buf, v| {
            buf.extend_from_slice(&v.to_le_bytes());
            Ok(())
        })?;
    }
    for tr in &tree.transitions {
        out.write_all(&tr.in_dim.to_le_bytes())?;
        out.write_all(&tr.out_dim.to_le_bytes())?;
        out.write_all(&u32_of(tr.entries.len())?.to_le_bytes())?;
        put_column(out, &tr.entries, |buf, &(i, j, w)| {
            buf.extend_from_slice(&i.to_le_bytes());
            buf.extend_from_slice(&j.to_le_bytes());
            buf.extend_from_slice(&w.to_le_bytes());
            Ok(())
        })?;
    }
    for label in &index.labels {
        out.write_all(&u32_of(label.len())?.to_le_bytes())?;
        out.write_all(label.as_bytes())?;
    }
    put_column(
        out,
        tree.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == NodeKind::Action)
            .flat_map(|(id, _)| tree.children(id as NodeId)),
        |buf, child| {
            buf.extend_from_slice(&index.edges[child as usize].to_le_bytes());
            Ok(())
        },
    )?;
    put_column(out, &index.pots, |buf, pot| {
        buf.extend_from_slice(&pot.to_le_bytes());
        Ok(())
    })
}

/// Reads exactly `len` bytes without trusting `len` for the allocation.
fn take(input: &mut dyn Read, len: u64) -> Result<Vec<u8>, SolError> {
    let mut bytes = Vec::new();
    input.take(len).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(SolError::Malformed("index stream is truncated"));
    }
    Ok(bytes)
}

fn words(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&w| u32::from_le_bytes(w))
}

fn read_u32(input: &mut dyn Read) -> Result<u32, SolError> {
    let mut word = [0u8; 4];
    input.read_exact(&mut word).map_err(truncated)?;
    Ok(u32::from_le_bytes(word))
}

fn read_u64(input: &mut dyn Read) -> Result<u64, SolError> {
    let mut word = [0u8; 8];
    input.read_exact(&mut word).map_err(truncated)?;
    Ok(u64::from_le_bytes(word))
}

fn truncated(error: io::Error) -> SolError {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        SolError::Malformed("index stream is truncated")
    } else {
        SolError::Io(error)
    }
}

fn decode_map(word: u32) -> Result<ReachMap, SolError> {
    let index = word & ((1 << MAP_INDEX_BITS) - 1);
    match word >> MAP_INDEX_BITS {
        0 if index == 0 => Ok(ReachMap::Identity),
        1 => Ok(ReachMap::Mask(index)),
        2 => Ok(ReachMap::Transition(index)),
        _ => Err(SolError::Malformed("index holds an unknown reach map")),
    }
}

/// Decodes an index stream written by [`write_index`] for `config`. The
/// stream must describe `nodes` nodes and end where the index ends.
pub(crate) fn read_index(
    input: &mut dyn Read,
    config: &PostflopConfig,
    nodes: u64,
) -> Result<IndexedTree, SolError> {
    let node_count = read_u64(input)?;
    if node_count != nodes {
        return Err(SolError::Malformed(
            "index and metadata disagree on the node count",
        ));
    }
    let deal_count = read_u64(input)?;
    let mask_count = read_u32(input)?;
    let transition_count = read_u32(input)?;
    let label_count = read_u32(input)?;
    let root_dims = PerPlayer::new(read_u32(input)?, read_u32(input)?);
    let hands = PostflopHands::new(&config.board, &config.ranges);
    if root_dims != PerPlayer::new(hands.len(Player::P0) as u32, hands.len(Player::P1) as u32) {
        return Err(SolError::Malformed(
            "index root dimensions differ from the embedded config's ranges",
        ));
    }

    let kinds = take(input, node_count)?;
    let mut shape_nodes = Vec::with_capacity(kinds.len());
    let mut terminals = 0u64;
    for &kind in &kinds {
        let (kind, player) = match kind {
            0 => (NodeKind::Action, Player::P0),
            1 => (NodeKind::Action, Player::P1),
            2 => (NodeKind::Chance, Player::P0),
            3 => (NodeKind::Terminal, Player::P0),
            _ => return Err(SolError::Malformed("index holds an unknown node kind")),
        };
        terminals += u64::from(kind == NodeKind::Terminal);
        shape_nodes.push(ShapeNode {
            kind,
            player,
            num_children: 0,
        });
    }
    drop(kinds);
    let counts = take(input, (node_count - terminals) * 2)?;
    let mut counts = counts.as_chunks::<2>().0.iter();
    for node in shape_nodes
        .iter_mut()
        .filter(|n| n.kind != NodeKind::Terminal)
    {
        node.num_children = u16::from_le_bytes(*counts.next().expect("sized above"));
    }
    let mut previous = 0u32;
    let terminal_ids = words(&take(input, terminals * 4)?)
        .map(|delta| {
            previous = previous.wrapping_add(delta);
            previous
        })
        .collect();

    let weights = take(input, deal_count * 4)?;
    let maps = take(input, deal_count * 8)?;
    let mut map_words = words(&maps);
    let deals = words(&weights)
        .map(|weight| {
            let mut next = || decode_map(map_words.next().expect("sized above"));
            Ok(Deal {
                weight: f32::from_bits(weight),
                maps: PerPlayer::new(next()?, next()?),
            })
        })
        .collect::<Result<Vec<_>, SolError>>()?;
    let deal_cards = take(input, deal_count)?;
    if deal_cards.iter().any(|&c| c >= 52 && c != NO_CARD) {
        return Err(SolError::Malformed("index deals an unknown card"));
    }

    let mut masks = Vec::new();
    for _ in 0..mask_count {
        let len = read_u32(input)?;
        masks.push(
            words(&take(input, u64::from(len) * 4)?)
                .map(f32::from_bits)
                .collect(),
        );
    }
    let mut transitions = Vec::new();
    for _ in 0..transition_count {
        let in_dim = read_u32(input)?;
        let out_dim = read_u32(input)?;
        let len = read_u32(input)?;
        let bytes = take(input, u64::from(len) * 12)?;
        let entries = bytes
            .as_chunks::<12>()
            .0
            .iter()
            .map(|e| {
                let word = |at: usize| u32::from_le_bytes(e[at..at + 4].try_into().expect("4"));
                (word(0), word(4), f32::from_bits(word(8)))
            })
            .collect();
        transitions.push(SparseTransition {
            in_dim,
            out_dim,
            entries,
        });
    }
    let mut labels = Vec::new();
    for _ in 0..label_count {
        let len = read_u32(input)?;
        let label = String::from_utf8(take(input, u64::from(len))?)
            .map_err(|_| SolError::Malformed("index label is not UTF-8"))?;
        labels.push(label);
    }

    let tree = PublicTree::from_shape(TreeShape {
        nodes: shape_nodes,
        terminal_ids,
        deals,
        masks,
        transitions,
        root_dims,
    })
    .map_err(|e| SolError::Malformed(e.0))?;

    let action_children: u64 = tree
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Action)
        .map(|n| u64::from(n.num_children))
        .sum();
    let edge_words = take(input, action_children * 4)?;
    let mut edge_words = words(&edge_words);
    let mut edges = vec![NO_LABEL; tree.nodes.len()];
    for (id, node) in tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action {
            for child in tree.children(id as NodeId) {
                let label = edge_words.next().expect("sized above");
                if label >= label_count {
                    return Err(SolError::Malformed("index names a missing action label"));
                }
                edges[child as usize] = label;
            }
        }
    }
    let pots = words(&take(input, tree.storage_refs.len() as u64 * 4)?).collect();
    let mut rest = [0u8; 1];
    if input.read(&mut rest)? != 0 {
        return Err(SolError::Malformed("index stream has trailing bytes"));
    }

    let start = crate::artifact::start_street_from_board_len(config.board.len());
    let root_ranges = crate::postflop::root_ranges(config, &hands);
    let index = GameIndex::assemble(
        &tree,
        labels,
        edges,
        deal_cards,
        pots,
        start,
        hands,
        root_ranges,
        config.scale,
        config.chip_rounding,
    );
    Ok(IndexedTree { tree, index })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare::internal_history;

    /// Monotone flop with suit-symmetric ranges, so turn and river deals
    /// merge into iso classes; an all-in flop bet adds runouts without
    /// action nodes.
    const ISO_FLOP: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack = 21
[spot]
line = "BTN c, BB x"
board = "Ks 7s 2s"
[ranges]
BB = "AA,QQ"
BTN = "JJ,TT"
[tree]
script = "flop { replace bet [a] } turn { replace bet [50%] } river { checkdown }"
[run]
memory = "1GiB"
"#;

    const TURN: &str = r#"
schema = "solvers.nlh/v1"
[table]
players = 2
stack = 21
[spot]
line = "BTN c, BB x / BB x, BTN x"
board = "2s 7s Ks 2h"
[ranges]
BB = "44,55"
BTN = "33,66"
[tree]
script = "turn { replace bet [2bb] } river { replace bet [100%] } turn when aggressions >= 1 { remove raise } river when aggressions >= 1 { remove raise }"
[solver]
iso_merging = false
[run]
memory = "1GiB"
"#;

    #[test]
    fn second_street_wager_history_uses_street_target() {
        for (bb_chips, bet, token) in [(1000, "2", "r2"), (6, "12c", "r12c")] {
            let raw = TURN
                .replace(
                    "players = 2",
                    &format!("players = 2\nbb_chips = {bb_chips}"),
                )
                .replace(
                    "river { replace bet [100%] }",
                    "river { replace bet [2bb] }",
                );
            let p = crate::prepare::prepare(&raw, std::path::Path::new("test.toml")).unwrap();
            let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
            let start = crate::artifact::start_street_from_board_len(p.config.board.len());
            let index = GameIndex::from_game(
                &built.game,
                &built.node_info,
                start,
                p.config.scale,
                p.config.chip_rounding,
            );
            let view = TreeView {
                tree: &built.game.tree,
                index: &index,
            };
            let chance = view.node_by_history_any(&format!("{token}c")).unwrap();
            let river = view.tree.children(chance).next().unwrap();
            let river_history = view.history(river);
            let labels = view.actions(river);
            let pos = labels
                .iter()
                .position(|label| *label == format!("bet {bet}"))
                .unwrap();
            let child = view.tree.children(river).nth(pos).unwrap();
            let history = format!("{river_history}{token}");
            assert_eq!(view.history(child), history);
            assert_eq!(view.node_by_history(&history), Some(child));
            assert_eq!(view.profile_nodes(&history).unwrap(), [child]);
        }
    }

    #[test]
    fn equivalent_histories_resolve_after_call_and_across_wildcards() {
        for (scale, bb, chips) in [(1000, "r2", "r2000c"), (6, "r2", "r12c")] {
            let raw = TURN.replace("players = 2", &format!("players = 2\nbb_chips = {scale}"));
            let p = crate::prepare::prepare(&raw, std::path::Path::new("test.toml")).unwrap();
            let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
            let index = GameIndex::from_game(
                &built.game,
                &built.node_info,
                Street::Turn,
                p.config.scale,
                p.config.chip_rounding,
            );
            let view = TreeView {
                tree: &built.game.tree,
                index: &index,
            };
            assert!(view.node_by_history(bb).is_some());
            assert_eq!(view.node_by_history(bb), view.node_by_history(chips));
            let bb_call = format!("{bb}c");
            let chip_call = format!("{chips}c");
            assert!(view.node_by_history_any(&bb_call).is_some());
            assert_eq!(
                view.node_by_history_any(&bb_call),
                view.node_by_history_any(&chip_call)
            );
            let bb_wild = format!("{bb_call}[?]");
            let chip_wild = format!("{chip_call}[?]");
            assert_eq!(
                view.profile_nodes(&bb_wild).unwrap(),
                view.profile_nodes(&chip_wild).unwrap()
            );
        }
    }

    #[test]
    fn selector_bb_amounts_use_table_rounding() {
        for (rounding, chips) in [("down", "r6c"), ("nearest", "r7c"), ("up", "r7c")] {
            let raw = TURN
                .replace(
                    "players = 2",
                    &format!("players = 2\nbb_chips = 3\nchip_rounding = \"{rounding}\""),
                )
                .replace(
                    "turn { replace bet [2bb] }",
                    "turn { replace bet [6c, 7c] }",
                );
            let p = crate::prepare::prepare(&raw, std::path::Path::new("test.toml")).unwrap();
            let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
            let index = GameIndex::from_game(
                &built.game,
                &built.node_info,
                Street::Turn,
                p.config.scale,
                p.config.chip_rounding,
            );
            let view = TreeView {
                tree: &built.game.tree,
                index: &index,
            };
            let expected = view.node_by_history(chips).unwrap();
            assert_eq!(view.node_by_history("r2.333"), Some(expected));
            assert_eq!(view.profile_nodes("r2.333").unwrap(), [expected]);
            assert_eq!(view.node_by_history("r2.333bb"), Some(expected));
            assert_eq!(view.profile_nodes("bet 2.333bb").unwrap(), [expected]);
            assert_eq!(
                view.profile_nodes(&format!("bet {}", &chips[1..])).unwrap(),
                [expected]
            );
            assert_eq!(
                view.resolve_action(0, "bet 2.333").unwrap(),
                view.resolve_action(0, &format!("bet {}", &chips[1..]))
                    .unwrap()
            );
            assert_eq!(
                view.profile_nodes("r2.333c[?]").unwrap(),
                view.profile_nodes(&format!("{chips}c[?]")).unwrap(),
            );
            let mut stream = Vec::new();
            write_index(&mut stream, &built.game.tree, &index).unwrap();
            let stored = read_index(
                &mut stream.as_slice(),
                &p.config,
                built.game.tree.nodes.len() as u64,
            )
            .unwrap();
            let stored_view = TreeView {
                tree: &stored.tree,
                index: &stored.index,
            };
            assert_eq!(stored_view.node_by_history("r2.333"), Some(expected));
        }
    }

    #[test]
    fn nondecimal_histories_round_trip_chip_suffix_and_call() {
        let raw = TURN.replace("players = 2", "players = 2\nbb_chips = 6");
        let p = crate::prepare::prepare(&raw, std::path::Path::new("test.toml")).unwrap();
        let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
        let start = crate::artifact::start_street_from_board_len(p.config.board.len());
        let index = GameIndex::from_game(
            &built.game,
            &built.node_info,
            start,
            p.config.scale,
            p.config.chip_rounding,
        );
        let view = TreeView {
            tree: &built.game.tree,
            index: &index,
        };
        let mut saw_chip_call = false;
        let mut saw_chip_wildcard = false;
        for id in 0..built.game.tree.nodes.len() as NodeId {
            let history = view.history(id);
            assert_eq!(view.node_by_history_any(&history), Some(id), "{history}");
            if history.contains("r12cc") {
                saw_chip_call = true;
                assert!(internal_history(&history, p.config.scale).contains("r12c"));
            }
            match view.tree.node(id).kind {
                // Profile selectors tokenize chip amounts the same way.
                NodeKind::Action => {
                    assert_eq!(view.profile_nodes(&history).unwrap(), [id], "{history}")
                }
                NodeKind::Chance if history.contains("12c") => {
                    saw_chip_wildcard = true;
                    let enumerated: Vec<_> = view.tree.children(id).collect();
                    let selector = format!("{history}[?]");
                    assert_eq!(
                        view.profile_nodes(&selector).unwrap(),
                        enumerated,
                        "{selector}"
                    );
                }
                _ => {}
            }
        }
        assert!(saw_chip_call);
        assert!(saw_chip_wildcard);
    }

    #[test]
    fn profile_wildcard_matches_enumerated_runouts() {
        let p = crate::prepare::prepare(TURN, std::path::Path::new("test.toml")).unwrap();
        let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
        let start = crate::artifact::start_street_from_board_len(p.config.board.len());
        let index = GameIndex::from_game(
            &built.game,
            &built.node_info,
            start,
            p.config.scale,
            p.config.chip_rounding,
        );
        let view = TreeView {
            tree: &built.game.tree,
            index: &index,
        };
        let selected = view.profile_nodes("xx[?]").unwrap();
        let chance = view.node_by_history_any("xx").unwrap();
        let enumerated: Vec<_> = view.tree.children(chance).collect();
        assert_eq!(selected, enumerated);
        assert_eq!(view.profile_nodes("check/check/?").unwrap(), enumerated);
        assert!(view.profile_nodes("xx").is_err());
        assert!(view.profile_nodes("xx[?]invalid").is_err());
    }

    /// The index a run stores reproduces, from the stream alone, the tree it
    /// solved and every label, history, pot, street and dealt card the
    /// builder recorded, including iso-merged branches and runouts.
    #[test]
    fn stored_index_reproduces_the_builder_tree_and_metadata() {
        for raw in [ISO_FLOP, TURN] {
            let p = crate::prepare::prepare(raw, std::path::Path::new("test.toml")).unwrap();
            let built = crate::build_postflop_game(&p.config, p.payoff.pipeline());
            let start = crate::artifact::start_street_from_board_len(p.config.board.len());
            let index = GameIndex::from_game(
                &built.game,
                &built.node_info,
                start,
                p.config.scale,
                p.config.chip_rounding,
            );
            let mut stream = Vec::new();
            write_index(&mut stream, &built.game.tree, &index).unwrap();
            let nodes = built.game.tree.nodes.len() as u64;
            let stored = read_index(&mut stream.as_slice(), &p.config, nodes).unwrap();
            assert!(read_index(&mut &stream[..stream.len() - 1], &p.config, nodes).is_err());
            assert!(read_index(&mut stream.as_slice(), &p.config, nodes + 1).is_err());

            let (a, b) = (&built.game.tree, &stored.tree);
            let node =
                |n: &hu_engine::Node| (n.kind, n.player, n.num_children, n.first_child, n.aux);
            assert!(a.nodes.iter().map(node).eq(b.nodes.iter().map(node)));
            let deal = |d: &Deal| (d.weight.to_bits(), d.maps);
            assert!(a.deals.iter().map(deal).eq(b.deals.iter().map(deal)));
            assert_eq!(a.masks, b.masks);
            assert_eq!(a.transitions.len(), b.transitions.len());
            for (x, y) in a.transitions.iter().zip(&b.transitions) {
                assert_eq!(
                    (x.in_dim, x.out_dim, &x.entries),
                    (y.in_dim, y.out_dim, &y.entries)
                );
            }
            assert_eq!(stored.index.root_ranges, built.game.root_ranges);
            for player in Player::BOTH {
                assert_eq!(
                    stored.index.hands.combos(player),
                    built.game.evaluator.hands.combos(player)
                );
            }

            let view = stored.view();
            let (mut merged, mut runouts) = (0, 0);
            for id in 0..a.nodes.len() as NodeId {
                let n = a.node(id);
                let history = view.history(id);
                assert_eq!(view.node_by_history_any(&history), Some(id), "{history}");
                if n.kind != NodeKind::Action {
                    assert_eq!(view.node_by_history(&history), None, "{history}");
                }
                for child in a.children(id) {
                    let history = view.history(child);
                    assert_eq!(view.node_by_history_any(&history), Some(child), "{history}");
                }
                match n.kind {
                    NodeKind::Action => {
                        let info = &built.node_info[a.tags[id as usize] as usize];
                        let history = view.history(id);
                        assert_eq!(internal_history(&history, p.config.scale), info.history);
                        assert_eq!(view.node_by_history(&history), Some(id));
                        let labels: Vec<String> = info
                            .actions
                            .iter()
                            .map(|l| display_label(l, p.config.scale))
                            .collect();
                        assert_eq!(view.actions(id), labels);
                        assert_eq!(
                            view.pot(id),
                            (info.contrib[Player::P0] + info.contrib[Player::P1]).0
                        );
                        assert_eq!(view.street(id), info.street);
                    }
                    NodeKind::Chance => {
                        for (pos, child) in a.children(id).enumerate() {
                            let expected = match a.deal(n, pos).maps[Player::P0] {
                                ReachMap::Mask(m) => {
                                    built.game.evaluator.mask_cards[m as usize].to_string()
                                }
                                ReachMap::Transition(_)
                                    if a.node(child).kind == NodeKind::Action =>
                                {
                                    merged += 1;
                                    let info = &built.node_info[a.tags[child as usize] as usize];
                                    format!("{}*", last_bracketed_card(&info.history).unwrap())
                                }
                                _ => {
                                    runouts += 1;
                                    format!("deal{pos}")
                                }
                            };
                            assert_eq!(view.chance_label(id, pos), expected);
                        }
                    }
                    NodeKind::Terminal => {}
                }
            }
            if raw == ISO_FLOP {
                assert!(
                    merged > 0 && runouts > 0,
                    "{merged} merged, {runouts} runouts"
                );
            }
            assert_eq!(view.node_by_history("q"), None);
            assert_eq!(view.node_by_history("x[2c]"), None);
            assert_eq!(view.node_by_history_any("q"), None);
            assert_eq!(view.node_by_history_any("x[2c]"), None);
            assert_eq!(view.node_by_history_any("xx[deal999999]"), None);
        }
    }
}
