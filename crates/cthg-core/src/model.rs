//! Data model of the CTHG (Causal-Temporal HyperGraph).
//!
//! Axes of the 3D memory:
//! - X: space — the repository path the memory applies to (`src/core/collider.rs`), hierarchical,
//!   optionally narrowed to a symbol. An empty path means "applies everywhere".
//! - Y: time — Unix seconds. Immutable once written and forward-only.
//! - Z: confidence — the χ score in [-1, 1], computed (never stored) by `score.rs`.
//!   +1: verified and explained. −1: contradicted, superseded or out of the current view.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// Something to always / never do.
    Rule,
    /// A design choice and its reason.
    Decision,
    /// Something true about the code (checkable).
    Fact,
    /// Something broke; usually the cause of a rule.
    Incident,
    /// Style or naming convention.
    Convention,
    /// Intended future work. Born `POTENTIAL`.
    Plan,
    /// How to build, test, run or deploy.
    Command,
    /// Summary of one agent session.
    Session,
}

impl NodeKind {
    pub const ALL: [NodeKind; 8] = [
        NodeKind::Rule,
        NodeKind::Decision,
        NodeKind::Fact,
        NodeKind::Incident,
        NodeKind::Convention,
        NodeKind::Plan,
        NodeKind::Command,
        NodeKind::Session,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Rule => "regla",
            NodeKind::Decision => "decision",
            NodeKind::Fact => "hecho",
            NodeKind::Incident => "incidente",
            NodeKind::Convention => "convencion",
            NodeKind::Plan => "plan",
            NodeKind::Command => "comando",
            NodeKind::Session => "sesion",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == s)
    }

    /// Kinds that a new statement can restate, reinforce or weaken.
    pub fn is_statement(self) -> bool {
        !matches!(self, NodeKind::Session)
    }
}

/// Where a memory came from. Decides its prior and whether it is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Proven by the repository itself (file exists, dependency present, command works).
    /// Anchored: cannot be overturned by talk, only by `verify` failing.
    Repo,
    /// Imported from an existing CLAUDE.md / AGENTS.md. Strong prior, not anchored.
    Imported,
    /// Deduced by the agent on its own. Even prior: does not count as proof.
    Agent,
    /// Said or corrected by the user.
    User,
    /// Raw session summary.
    Session,
}

impl Origin {
    pub const ALL: [Origin; 5] = [Origin::Repo, Origin::Imported, Origin::Agent, Origin::User, Origin::Session];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Origin::Repo => "repo",
            Origin::Imported => "importado",
            Origin::Agent => "agente",
            Origin::User => "usuario",
            Origin::Session => "sesion",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|o| o.name() == s)
    }

    /// Beta prior (α, β) for the evidence view.
    pub fn prior(self) -> (f32, f32) {
        match self {
            Origin::Repo => (20.0, 0.5),
            Origin::Imported => (5.0, 1.5),
            Origin::Agent => (1.5, 1.5),
            Origin::User => (4.0, 1.0),
            Origin::Session => (3.0, 1.0),
        }
    }
}

pub mod flags {
    /// Proven by the repo: the structural view treats it as a deterministic root.
    pub const ANCHORED: u8 = 1 << 0;
    /// A learned memory that gathered enough agreeing evidence.
    pub const CONSOLIDATED: u8 = 1 << 1;
    /// Always injected (cross-cutting rules with no place in the tree).
    pub const CORE: u8 = 1 << 2;
    /// A plan: a possible future, not a fact. It never becomes true by time passing,
    /// only when something later shows it happened.
    pub const POTENTIAL: u8 = 1 << 3;
    /// `verify` found that its path, symbol, dependency or command no longer holds.
    /// Structural, not evidence: it sinks χ while set and lifts when `verify` passes again.
    pub const STALE: u8 = 1 << 4;
    /// A correction captured from a transcript by heuristics: kept, but never injected
    /// (except when its own files are read) until `cthg confirm` clears it.
    pub const PROPOSED: u8 = 1 << 5;
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub kind: NodeKind,
    pub origin: Origin,
    pub flags: u8,
    /// X: repository path, `/`-separated, relative to the root. Empty = everywhere.
    pub path: String,
    /// X, finer: function / type inside `path`. Empty = the whole path.
    pub symbol: String,
    /// Git branch it was learned on. Empty = trunk (visible from every branch).
    pub branch: String,
    /// Short commit hash at the time it was learned (may be empty outside git).
    pub commit: String,
    /// Y: Unix seconds.
    pub y: f64,
    /// Evidence view: Beta evidence for / against.
    pub alpha: f32,
    pub beta: f32,
    /// Recency of recall in [0, 1]; decays every session (forgetting).
    pub activation: f32,
    pub recalls: u32,
    pub created_session: u32,
    pub key: String,
    pub label: String,
    pub text: String,
    pub aliases: Vec<String>,
}

impl Node {
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }

    pub fn anchored(&self) -> bool {
        self.has(flags::ANCHORED)
    }

    /// Evidence gathered beyond the uniform prior.
    pub fn evidence(&self) -> f32 {
        (self.alpha + self.beta - 2.0).max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// Directed cause → effect. Its α/β measure how deterministic the link is.
    Causes,
    /// Pure temporal succession (session chain).
    Precedes,
    /// Associative link, Hebbian weight.
    Relates,
    /// The source contradicts the target.
    Contradicts,
    /// A session or memory mentions another.
    Mentions,
    /// The source (newer) replaces the target (older).
    Supersedes,
}

impl EdgeKind {
    pub const ALL: [EdgeKind; 6] = [
        EdgeKind::Causes,
        EdgeKind::Precedes,
        EdgeKind::Relates,
        EdgeKind::Contradicts,
        EdgeKind::Mentions,
        EdgeKind::Supersedes,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(c as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::Causes => "causa",
            EdgeKind::Precedes => "precede",
            EdgeKind::Relates => "relacion",
            EdgeKind::Contradicts => "contradice",
            EdgeKind::Mentions => "menciona",
            EdgeKind::Supersedes => "sustituye",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == s)
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
    pub alpha: f32,
    pub beta: f32,
    /// Hebbian association strength in [0, 1].
    pub hebb: f32,
    pub created_session: u32,
}

impl Edge {
    /// Determinism of a causal link in [0, 1]: 2p - 1 clamped.
    pub fn determinism(&self) -> f32 {
        let p = self.alpha / (self.alpha + self.beta).max(1e-6);
        (2.0 * p - 1.0).clamp(0.0, 1.0)
    }
}

/// One hyperedge binding many nodes: an incident, a PR, a decision that touched several places.
#[derive(Clone, Debug)]
pub struct Hyperedge {
    pub id: u32,
    pub label: String,
    pub y: f64,
    pub members: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub hyperedges: Vec<Hyperedge>,
    /// Session counter (one per agent session that wrote something).
    pub session: u32,
    pub meta: Vec<(String, String)>,
}

impl Memory {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.get_mut(id as usize)
    }

    pub fn by_key(&self, key: &str) -> Option<u32> {
        self.nodes.iter().find(|n| n.key == key).map(|n| n.id)
    }

    pub fn meta(&self, k: &str) -> Option<&str> {
        self.meta.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub fn set_meta(&mut self, k: &str, v: &str) {
        match self.meta.iter_mut().find(|(a, _)| a == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.meta.push((k.to_string(), v.to_string())),
        }
    }

    pub fn add_node(&mut self, mut n: Node) -> u32 {
        let id = self.nodes.len() as u32;
        n.id = id;
        self.nodes.push(n);
        id
    }

    pub fn find_edge(&self, from: u32, to: u32, kind: EdgeKind) -> Option<usize> {
        self.edges
            .iter()
            .position(|e| e.from == from && e.to == to && e.kind == kind)
    }

    /// Adds the edge, or returns the index of the existing one with the same endpoints and kind.
    pub fn upsert_edge(&mut self, from: u32, to: u32, kind: EdgeKind, alpha: f32, beta: f32) -> usize {
        if let Some(i) = self.find_edge(from, to, kind) {
            return i;
        }
        self.edges.push(Edge {
            from,
            to,
            kind,
            alpha,
            beta,
            hebb: 0.0,
            created_session: self.session,
        });
        self.edges.len() - 1
    }

    /// The memory's present: the latest instant written. Time only advances, so this never
    /// moves back. Plans are excluded: they are not part of the past yet.
    pub fn horizon(&self) -> f64 {
        self.nodes
            .iter()
            .filter(|n| !n.has(flags::POTENTIAL))
            .map(|n| n.y)
            .fold(0.0, f64::max)
    }
}
