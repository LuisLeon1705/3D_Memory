//! Learning. Three plasticity rules, one per axis:
//!
//! - Y (time) is immutable and forward-only: every memory is written after the present and
//!   nothing already written is moved or erased. Anchored facts cannot be talked out of the repo.
//! - X (space/association) is plastic: memories recalled together get Hebbian links, activation
//!   decays every session (forgetting) and rises on recall; paths follow renames (`spatial`).
//! - Z (confidence, χ) moves only through evidence: agreeing statements add α, denials add β,
//!   statements against anchored facts are pushed out (toward −1).

use super::model::*;
use super::score::{self, Score, View};
use super::spatial;
use super::text;

const ACTIVATION_DECAY: f32 = 0.92;
const HEBB_DECAY: f32 = 0.995;
/// Minimum step of the Y axis, in seconds.
const STEP: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Affirms,
    Denies,
}

/// One statement to learn: a correction, a rule, a plan, a fact.
#[derive(Clone, Debug)]
pub struct Claim {
    pub text: String,
    pub kind: NodeKind,
    pub origin: Origin,
    pub path: String,
    pub symbol: String,
    /// Branch it is learned on ("" = trunk).
    pub branch: String,
    pub commit: String,
    pub stance: Stance,
    /// Why, in words: matched against existing memories or written as a new one.
    pub cause: Option<String>,
    /// Why, as an existing node.
    pub cause_id: Option<u32>,
    pub contradicts: Option<u32>,
    pub supersedes: Option<u32>,
    pub core: bool,
}

impl Claim {
    /// A rule said by the user, anchored at `path` ("" = nowhere in particular).
    pub fn rule(text: &str, path: &str) -> Self {
        Claim {
            text: text.to_string(),
            kind: NodeKind::Rule,
            origin: Origin::User,
            path: spatial::normalize(path),
            symbol: String::new(),
            branch: String::new(),
            commit: String::new(),
            stance: Stance::Affirms,
            cause: None,
            cause_id: None,
            contradicts: None,
            supersedes: None,
            core: false,
        }
    }

    pub fn because(mut self, cause: &str) -> Self {
        self.cause = Some(cause.to_string());
        self
    }
}

#[derive(Clone, Debug)]
pub struct LearnEvent {
    pub node: u32,
    pub label: String,
    /// "nuevo", "nueva causa", "causa", "refuerza", "debilita", "rechazado", "consolidado", "se cumple", "sustituido"
    pub action: &'static str,
    pub chi_before: f32,
    pub chi_after: f32,
}

/// Next instant on the timeline: now, or just after everything already written.
pub fn next_y(m: &Memory, now: f64) -> f64 {
    now.max(m.horizon() + STEP)
}

fn view_for(branch: &str) -> View {
    View::on_branch(f64::INFINITY, branch)
}

fn new_node(m: &Memory, c: &Claim, kind: NodeKind, y: f64, body: &str) -> Node {
    let (alpha, beta) = c.origin.prior();
    Node {
        id: 0,
        kind,
        origin: c.origin,
        flags: if c.origin == Origin::Repo { flags::ANCHORED } else { 0 },
        path: c.path.clone(),
        symbol: c.symbol.clone(),
        branch: c.branch.clone(),
        commit: c.commit.clone(),
        y,
        alpha,
        beta,
        activation: 1.0,
        recalls: 0,
        created_session: m.session,
        key: String::new(),
        label: text::clip(body, 64),
        text: text::clip(body, 600),
        aliases: vec![],
    }
}

/// Paths overlap: one is the other or an ancestor of it. Unanchored nodes overlap everything.
fn paths_overlap(a: &str, b: &str) -> bool {
    spatial::is_ancestor_or_self(a, b) || spatial::is_ancestor_or_self(b, a)
}

fn doc_tokens(n: &Node) -> Vec<String> {
    text::tokens(&format!("{} {}", n.label, n.text))
}

fn find_similar(m: &Memory, scores: &[Score], toks: &[String], path: &str, min: f32) -> Option<u32> {
    m.nodes
        .iter()
        // Nodes created after `scores` was computed have no score yet: skip them.
        .filter(|n| scores.get(n.id as usize).is_some_and(|s| s.visible))
        .filter(|n| n.kind.is_statement() && paths_overlap(&n.path, path))
        .map(|n| (n.id, text::similarity(toks, &doc_tokens(n))))
        .filter(|&(_, j)| j >= min)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

/// A plan that this statement shows has happened.
fn find_plan(m: &Memory, toks: &[String], path: &str, min: f32) -> Option<u32> {
    m.nodes
        .iter()
        .filter(|n| n.has(flags::POTENTIAL) && paths_overlap(&n.path, path))
        .map(|n| (n.id, text::similarity(toks, &doc_tokens(n))))
        .filter(|&(_, j)| j >= min)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

fn rejected_by_anchor(m: &Memory, scores: &[Score], id: u32) -> bool {
    m.edges.iter().any(|e| {
        e.kind == EdgeKind::Contradicts
            && e.from == id
            && scores.get(e.to as usize).is_some_and(|s| s.visible)
            && m.nodes[e.to as usize].anchored()
    })
}

/// Evidence gathered on top of the origin's prior.
fn gathered(n: &Node) -> f32 {
    let (a0, b0) = n.origin.prior();
    (n.alpha + n.beta - a0 - b0).max(0.0)
}

fn consolidate(m: &mut Memory, id: u32) -> bool {
    let n = &mut m.nodes[id as usize];
    if !n.anchored() && !n.has(flags::CONSOLIDATED) && gathered(n) >= 3.0 && score::chance(n) >= 0.6 {
        n.flags |= flags::CONSOLIDATED;
        return true;
    }
    false
}

/// Learns one statement at wall-clock time `now` (Unix seconds).
pub fn learn(m: &mut Memory, c: &Claim, now: f64) -> Vec<LearnEvent> {
    let view = view_for(&c.branch);
    let before = score::score_all(m, &view);
    let toks = text::tokens(&c.text);
    if toks.len() < 2 {
        return vec![];
    }
    let mut touched: Vec<(u32, &'static str)> = Vec::new();

    // A plan only becomes part of the past when something shows it happened: then it is written now.
    if c.stance == Stance::Affirms && c.kind != NodeKind::Plan {
        if let Some(id) = find_plan(m, &toks, &c.path, 0.55) {
            let y = next_y(m, now);
            let n = &mut m.nodes[id as usize];
            n.flags &= !flags::POTENTIAL;
            n.y = y;
            n.alpha += 1.0;
            n.activation = 1.0;
            if !c.commit.is_empty() {
                n.commit = c.commit.clone();
            }
            touched.push((id, "se cumple"));
            return events(m, &view, &before, touched);
        }
    }

    let restated = if c.supersedes.is_none() && c.kind != NodeKind::Plan {
        find_similar(m, &before, &toks, &c.path, 0.5)
    } else {
        None
    };
    if let Some(id) = restated {
        let anchored = m.nodes[id as usize].anchored();
        let rejected = rejected_by_anchor(m, &before, id);
        let n = &mut m.nodes[id as usize];
        n.activation = 1.0;
        if c.core {
            n.flags |= flags::CORE;
        }
        match (c.stance, anchored) {
            (Stance::Affirms, true) => touched.push((id, "refuerza")),
            (Stance::Affirms, false) if rejected => {
                n.beta += 1.0;
                touched.push((id, "rechazado"));
            }
            (Stance::Affirms, false) => {
                n.alpha += 1.0;
                touched.push((id, "refuerza"));
            }
            (Stance::Denies, false) => {
                n.beta += 1.0;
                touched.push((id, "debilita"));
            }
            (Stance::Denies, true) => {
                // The repo says otherwise: the denial is kept, but outside what is trusted.
                let y = next_y(m, now);
                let label = format!("Niega: {}", m.nodes[id as usize].label);
                let mut f = new_node(m, c, NodeKind::Fact, y, &c.text);
                f.label = text::clip(&label, 64);
                f.flags &= !flags::ANCHORED;
                f.beta += 2.0;
                let f = m.add_node(f);
                m.upsert_edge(f, id, EdgeKind::Contradicts, 1.0, 1.0);
                touched.push((f, "rechazado"));
            }
        }
        return events(m, &view, &before, touched);
    }

    let y = next_y(m, now);
    let mut f = new_node(m, c, c.kind, y, &c.text);
    if c.core {
        f.flags |= flags::CORE;
    }
    if c.kind == NodeKind::Plan {
        f.flags |= flags::POTENTIAL;
        f.flags &= !flags::ANCHORED;
    }
    if c.stance == Stance::Denies {
        f.beta += 1.0;
    }
    let fid = m.add_node(f);
    let mut action = "nuevo";

    if let Some(cid) = c.cause_id.filter(|&i| before.get(i as usize).is_some_and(|s| s.visible)) {
        m.upsert_edge(cid, fid, EdgeKind::Causes, 3.0, 1.0);
        touched.push((cid, "causa"));
    }
    if let Some(t) = c.contradicts.filter(|&t| before.get(t as usize).is_some_and(|s| s.visible)) {
        m.upsert_edge(fid, t, EdgeKind::Contradicts, 1.0, 1.0);
        if m.nodes[t as usize].anchored() {
            m.nodes[fid as usize].beta += 2.0;
            action = "rechazado";
        } else {
            m.nodes[t as usize].beta += 1.0;
            touched.push((t, "debilita"));
        }
    }
    if let Some(old) = c.supersedes.filter(|&o| before.get(o as usize).is_some_and(|s| s.visible)) {
        // Anchored facts are only replaced when `verify` drops their anchor.
        m.upsert_edge(fid, old, EdgeKind::Supersedes, 1.0, 1.0);
        touched.push((old, "sustituido"));
    }
    if let Some(cause) = c.cause.as_deref().filter(|s| text::tokens(s).len() >= 2) {
        let ctoks = text::tokens(cause);
        let (cid, action) = match find_similar(m, &before, &ctoks, &c.path, 0.35) {
            Some(id) => (id, "causa"),
            None => {
                let mut cn = new_node(m, c, NodeKind::Incident, y - STEP * 0.5, cause);
                cn.flags &= !flags::ANCHORED;
                (m.add_node(cn), "nueva causa")
            }
        };
        let i = m.upsert_edge(cid, fid, EdgeKind::Causes, 1.0, 1.0);
        m.edges[i].alpha += 1.0;
        touched.push((cid, action));
    }
    touched.push((fid, action));
    events(m, &view, &before, touched)
}

fn events(m: &mut Memory, view: &View, before: &[Score], touched: Vec<(u32, &'static str)>) -> Vec<LearnEvent> {
    let mut consolidated = Vec::new();
    for &(id, _) in &touched {
        consolidated.push(consolidate(m, id));
    }
    let after = score::score_all(m, view);
    touched
        .into_iter()
        .zip(consolidated)
        .map(|((id, action), c)| LearnEvent {
            node: id,
            label: m.nodes[id as usize].label.clone(),
            action: if c { "consolidado" } else { action },
            chi_before: before.get(id as usize).map(|s| s.chi).unwrap_or(0.0),
            chi_after: after[id as usize].chi,
        })
        .collect()
}

/// Manual feedback (`cthg confirm` / `cthg deny`): +1 confirms, −1 denies. Anchored never moves.
pub fn feedback(m: &mut Memory, id: u32, vote: i32) -> bool {
    let Some(n) = m.node_mut(id) else { return false };
    if n.anchored() {
        return false;
    }
    if vote > 0 {
        n.alpha += 1.0;
    } else {
        n.beta += 1.0;
    }
    n.activation = 1.0;
    consolidate(m, id);
    true
}

/// Start of an agent session: forgetting runs once per session, not per message.
pub fn begin_session(m: &mut Memory) {
    m.session += 1;
    for n in m.nodes.iter_mut() {
        n.activation *= ACTIVATION_DECAY;
    }
    for e in m.edges.iter_mut() {
        if e.kind == EdgeKind::Relates {
            e.hebb *= HEBB_DECAY;
        }
    }
}

/// Memories injected together: they become associated and fresh.
pub fn co_recalled(m: &mut Memory, ids: &[u32]) {
    // Injected together is not associated: linking every pair turned each injection into a
    // clique, and one matching word then pulled the whole clique back (a feedback loop).
    for &id in ids {
        if let Some(n) = m.node_mut(id) {
            n.activation = 1.0;
            n.recalls += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chi(m: &Memory, id: u32) -> f32 {
        score::score_all(m, &View::all())[id as usize].chi
    }

    #[test]
    fn time_moves_forward_and_repetition_consolidates() {
        let mut m = Memory::default();
        let c = Claim::rule("Los tests de integración necesitan la base de datos local", "tests");
        let id = learn(&mut m, &c, 1000.0)[0].node;
        let chi0 = chi(&m, id);
        let ev = learn(&mut m, &c, 500.0); // a clock that went back cannot move Y back
        assert_eq!(ev[0].node, id);
        for _ in 0..3 {
            learn(&mut m, &c, 1001.0);
        }
        assert!(chi(&m, id) > chi0);
        assert!(m.nodes[id as usize].has(flags::CONSOLIDATED));

        let other = learn(&mut m, &Claim::rule("Compila siempre en modo release", "tests"), 10.0)[0].node;
        assert!(m.nodes[other as usize].y > m.nodes[id as usize].y, "Y only moves forward");
    }

    #[test]
    fn denying_an_anchored_fact_stays_outside() {
        let mut m = Memory::default();
        let mut fact = Claim::rule("El proyecto usa tokio como runtime asíncrono", "Cargo.toml");
        fact.kind = NodeKind::Fact;
        fact.origin = Origin::Repo;
        let f = learn(&mut m, &fact, 100.0)[0].node;
        let mut deny = Claim::rule("El proyecto usa tokio como runtime asíncrono", "Cargo.toml");
        deny.origin = Origin::Agent;
        deny.stance = Stance::Denies;
        let ev = learn(&mut m, &deny, 101.0);
        assert_eq!(ev[0].action, "rechazado");
        assert!(chi(&m, ev[0].node) < -0.5);
        assert!(chi(&m, f) > 0.9, "the anchored fact is untouched");
    }

    #[test]
    fn plans_are_invisible_until_they_happen() {
        let mut m = Memory::default();
        let mut plan = Claim::rule("Migrar el renderizado de OpenGL a wgpu", "src/render");
        plan.kind = NodeKind::Plan;
        let p = learn(&mut m, &plan, 100.0)[0].node;
        assert!(!score::score_all(&m, &View::all())[p as usize].visible);
        let ev = learn(&mut m, &Claim::rule("Migrado el renderizado de OpenGL a wgpu", "src/render/mod.rs"), 200.0);
        assert_eq!((ev[0].node, ev[0].action), (p, "se cumple"));
        assert!(score::score_all(&m, &View::all())[p as usize].visible);
    }

    #[test]
    fn supersede_sinks_the_old_rule() {
        let mut m = Memory::default();
        let old = learn(&mut m, &Claim::rule("Los errores se devuelven como String", "src"), 100.0)[0].node;
        let mut new = Claim::rule("Los errores usan thiserror con un enum por módulo", "src");
        new.supersedes = Some(old);
        learn(&mut m, &new, 200.0);
        assert!(chi(&m, old) < 0.0, "chi = {}", chi(&m, old));
    }
}
