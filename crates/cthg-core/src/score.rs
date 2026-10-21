//! χ — the confidence axis (Z), computed for a view (a present and a set of branches).
//!
//! Two readings of the same memory, blended:
//! - Structural (E): deterministic propagation. Anchored facts are roots (E = 1). A node is as
//!   determined as its best causal chain back to the roots: E = max(E(parent) · det(edge)).
//!   Contradicting an established memory, being superseded, or being `STALE` (the repo no longer
//!   matches it) gives E = −1. No support: E = 0.
//! - Evidence (A): Bayesian. A = 2·α/(α+β) − 1 from the Beta evidence.
//!
//! χ = w·E + (1−w)·A, with w = K / (K + evidence): with little evidence the structure decides,
//! with a lot of evidence the statistics decide.
//!
//! View gating: a node after the view's present, a plan (`POTENTIAL`) or a node from a branch
//! outside the view does not exist for it → χ = −1.

use super::model::*;

const K: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    pub chi: f32,
    pub engineer: f32,
    pub chance: f32,
    /// false when the node lies outside the view (future, plan, other branch).
    pub visible: bool,
    /// true if superseded by a visible node with chi > 0.
    pub superseded: bool,
}

/// What is visible: everything written up to `now` on trunk or on one of `branches`.
#[derive(Clone, Debug)]
pub struct View {
    pub now: f64,
    /// `None` = every branch (god view).
    pub branches: Option<Vec<String>>,
}

impl View {
    pub fn all() -> Self {
        View { now: f64::INFINITY, branches: None }
    }

    pub fn on_branch(now: f64, branch: &str) -> Self {
        let branches = if branch.is_empty() { vec![] } else { vec![branch.to_string()] };
        View { now, branches: Some(branches) }
    }

    pub fn sees(&self, n: &Node) -> bool {
        n.y <= self.now + 1e-4
            && !n.has(flags::POTENTIAL)
            && (n.branch.is_empty() || self.branches.as_ref().is_none_or(|b| b.contains(&n.branch)))
    }
}

pub fn chance(n: &Node) -> f32 {
    2.0 * n.alpha / (n.alpha + n.beta).max(1e-6) - 1.0
}

pub fn blend(engineer: f32, chance: f32, evidence: f32) -> f32 {
    let w = K / (K + evidence);
    (w * engineer + (1.0 - w) * chance).clamp(-1.0, 1.0)
}

/// Scores every node as seen from `view`.
pub fn score_all(m: &Memory, view: &View) -> Vec<Score> {
    let n = m.nodes.len();
    let visible: Vec<bool> = m.nodes.iter().map(|x| view.sees(x)).collect();

    // Topological order by (y, id): causal edges only count forward in time, so this is a DAG.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        let (na, nb) = (&m.nodes[a as usize], &m.nodes[b as usize]);
        na.y.partial_cmp(&nb.y).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    let mut rank = vec![0usize; n];
    for (r, &id) in order.iter().enumerate() {
        rank[id as usize] = r;
    }

    let mut parents: Vec<Vec<(u32, f32)>> = vec![Vec::new(); n];
    let mut contradicts: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut superseded_by: Vec<Vec<u32>> = vec![Vec::new(); n];
    for e in &m.edges {
        let (f, t) = (e.from as usize, e.to as usize);
        match e.kind {
            EdgeKind::Causes if rank[f] < rank[t] => parents[t].push((e.from, e.determinism())),
            EdgeKind::Contradicts => contradicts[f].push(e.to),
            EdgeKind::Supersedes => superseded_by[t].push(e.from),
            _ => {}
        }
    }

    let mut out = vec![Score::default(); n];
    for &id in &order {
        let i = id as usize;
        let node = &m.nodes[i];
        let a = chance(node);
        if !visible[i] {
            out[i] = Score { chi: -1.0, engineer: -1.0, chance: a, visible: false, superseded: false };
            continue;
        }
        let e = if node.anchored() {
            1.0
        } else {
            // A contradiction only matters against something established and already in memory.
            let against = contradicts[i].iter().any(|&t| {
                let t = t as usize;
                visible[t] && (m.nodes[t].anchored() || (rank[t] < rank[i] && out[t].chi > 0.5))
            });
            if against || node.has(flags::STALE) {
                -1.0
            } else {
                parents[i]
                    .iter()
                    .filter(|(p, _)| visible[*p as usize])
                    .map(|&(p, det)| out[p as usize].engineer.max(0.0) * det)
                    .fold(0.0f32, f32::max)
            }
        };
        let chi = if node.anchored() { blend(1.0, a, node.evidence()).max(0.9) } else { blend(e, a, node.evidence()) };
        out[i] = Score { chi, engineer: e, chance: a, visible: true, superseded: false };
    }
    // Replacements can point in either direction (e.g. cthg supersede new old with new < old).
    // An anchored fact is never superseded (only verify moves it).
    for i in 0..n {
        if !out[i].visible || m.nodes[i].anchored() {
            continue;
        }
        if has_active_substitute(m, &out, i) {
            out[i].superseded = true;
            out[i].engineer = -1.0;
            out[i].chi = blend(-1.0, out[i].chance, m.nodes[i].evidence());
        }
    }
    // Contradictions can point forward in time (an older note vs. a newer fact): one more pass.
    for i in 0..n {
        if !out[i].visible || m.nodes[i].anchored() || out[i].engineer < 0.0 {
            continue;
        }
        if contradicts[i].iter().any(|&t| out[t as usize].visible && out[t as usize].chi > 0.5 && out[t as usize].chi > out[i].chi) {
            out[i].engineer = -1.0;
            out[i].chi = blend(-1.0, out[i].chance, m.nodes[i].evidence());
        }
    }
    out
}

fn has_active_substitute(m: &Memory, out: &[Score], i: usize) -> bool {
    let mut visited = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    for e in &m.edges {
        if e.to == i as u32 && e.kind == EdgeKind::Supersedes {
            queue.push_back(e.from as usize);
            visited.insert(e.from as usize);
        }
    }
    while let Some(s) = queue.pop_front() {
        if !out[s].visible {
            continue;
        }
        let s_superseded = m.edges.iter().any(|e| e.to == s as u32 && e.kind == EdgeKind::Supersedes);
        if !s_superseded {
            if out[s].chi > 0.0 {
                return true;
            }
        } else {
            if out[s].chi > 0.0 {
                return true;
            }
            for e in &m.edges {
                if e.to == s as u32 && e.kind == EdgeKind::Supersedes && visited.insert(e.from as usize) {
                    queue.push_back(e.from as usize);
                }
            }
        }
    }
    false
}

/// Band name for the injected context and the UI.
pub fn certainty_tag(chi: f32) -> &'static str {
    if chi >= 0.75 {
        "verificado"
    } else if chi >= 0.25 {
        "probable"
    } else if chi > -0.25 {
        "sin evidencia"
    } else if chi > -0.75 {
        "dudoso"
    } else {
        "descartado"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn node(m: &mut Memory, origin: Origin, y: f64) -> u32 {
        let (alpha, beta) = origin.prior();
        let flags = if origin == Origin::Repo { flags::ANCHORED } else { 0 };
        m.add_node(Node {
            id: 0,
            kind: NodeKind::Rule,
            origin,
            flags,
            path: String::new(),
            symbol: String::new(),
            branch: String::new(),
            commit: String::new(),
            y,
            alpha,
            beta,
            activation: 0.0,
            recalls: 0,
            created_session: 0,
            key: String::new(),
            label: String::new(),
            text: String::new(),
            aliases: vec![],
        })
    }

    #[test]
    fn anchored_is_verified_and_future_is_outside() {
        let mut m = Memory::default();
        let a = node(&mut m, Origin::Repo, 100.0);
        let b = node(&mut m, Origin::Repo, 200.0);
        let s = score_all(&m, &View::on_branch(150.0, ""));
        assert!(s[a as usize].chi > 0.9);
        assert_eq!(s[b as usize].chi, -1.0);
        assert!(!s[b as usize].visible);
    }

    #[test]
    fn causal_chain_propagates_structural_view() {
        let mut m = Memory::default();
        let root = node(&mut m, Origin::Repo, 100.0);
        let child = node(&mut m, Origin::Agent, 101.0);
        let lone = node(&mut m, Origin::Agent, 101.0);
        m.upsert_edge(root, child, EdgeKind::Causes, 9.0, 1.0);
        let s = score_all(&m, &View::all());
        assert!(s[child as usize].engineer > 0.7);
        assert!(s[child as usize].chi > s[lone as usize].chi);
        assert!(s[lone as usize].chi.abs() < 1e-3, "even prior and no cause = no evidence");
    }

    #[test]
    fn contradicting_anchored_sinks() {
        let mut m = Memory::default();
        let fact = node(&mut m, Origin::Repo, 100.0);
        let claim = node(&mut m, Origin::Agent, 102.0);
        m.upsert_edge(claim, fact, EdgeKind::Contradicts, 1.0, 1.0);
        m.nodes[claim as usize].beta += 6.0;
        let s = score_all(&m, &View::all());
        assert_eq!(s[claim as usize].engineer, -1.0);
        assert!(s[claim as usize].chi < -0.75, "chi = {}", s[claim as usize].chi);
    }

    #[test]
    fn superseded_rule_sinks_and_branches_are_separate() {
        let mut m = Memory::default();
        let old = node(&mut m, Origin::User, 100.0);
        let new = node(&mut m, Origin::User, 110.0);
        m.upsert_edge(new, old, EdgeKind::Supersedes, 1.0, 1.0);
        let s = score_all(&m, &View::all());
        assert!(s[old as usize].chi < 0.0 && s[new as usize].chi > 0.25);

        m.nodes[new as usize].branch = "feature/x".into();
        let trunk = score_all(&m, &View::on_branch(f64::INFINITY, ""));
        assert!(!trunk[new as usize].visible, "branch memory is not on trunk until merged");
        assert!(trunk[old as usize].chi > 0.25, "the old rule still holds on trunk");
    }

    #[test]
    fn superseded_chain_invariant_to_id_order() {
        // Case A: substitute has smaller ID (0 supersedes 1, 1 supersedes 2)
        let mut m_a = Memory::default();
        let a0 = node(&mut m_a, Origin::User, 100.0);
        let a1 = node(&mut m_a, Origin::User, 101.0);
        let a2 = node(&mut m_a, Origin::User, 102.0);
        m_a.upsert_edge(a0, a1, EdgeKind::Supersedes, 1.0, 1.0);
        m_a.upsert_edge(a1, a2, EdgeKind::Supersedes, 1.0, 1.0);
        let s_a = score_all(&m_a, &View::all());
        assert!(!s_a[a0 as usize].superseded);
        assert!(s_a[a1 as usize].superseded);
        assert!(s_a[a2 as usize].superseded);
        assert!(s_a[a0 as usize].chi > 0.0);
        assert!(s_a[a1 as usize].chi < 0.0);
        assert!(s_a[a2 as usize].chi < 0.0);

        // Case B: substitute has larger ID (2 supersedes 1, 1 supersedes 0)
        let mut m_b = Memory::default();
        let b0 = node(&mut m_b, Origin::User, 100.0);
        let b1 = node(&mut m_b, Origin::User, 101.0);
        let b2 = node(&mut m_b, Origin::User, 102.0);
        m_b.upsert_edge(b2, b1, EdgeKind::Supersedes, 1.0, 1.0);
        m_b.upsert_edge(b1, b0, EdgeKind::Supersedes, 1.0, 1.0);
        let s_b = score_all(&m_b, &View::all());
        assert!(!s_b[b2 as usize].superseded);
        assert!(s_b[b1 as usize].superseded);
        assert!(s_b[b0 as usize].superseded);
        assert!(s_b[b2 as usize].chi > 0.0);
        assert!(s_b[b1 as usize].chi < 0.0);
        assert!(s_b[b0 as usize].chi < 0.0);
    }

    #[test]
    fn anchored_node_never_superseded_by_edge() {
        let mut m = Memory::default();
        let fact = node(&mut m, Origin::Repo, 100.0);
        let replacement = node(&mut m, Origin::User, 101.0);
        m.upsert_edge(replacement, fact, EdgeKind::Supersedes, 1.0, 1.0);
        let s = score_all(&m, &View::all());
        assert!(!s[fact as usize].superseded, "anchored fact must never be marked superseded");
        assert!(s[fact as usize].chi >= 0.9, "anchored fact chi stays >= 0.9");
    }
}