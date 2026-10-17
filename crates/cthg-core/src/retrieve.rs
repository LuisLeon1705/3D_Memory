//! Recall: three channels fused, then spreading activation, ranked by trust, cut to a budget.
//!
//! - Channel 0, core: `CORE` nodes, always (see `core`). Never repeated by `recall`.
//! - Spatial: paths being worked on (git status/diff, the file just read) → `spatial::seeds`.
//! - Lexical: label/alias phrases and content tokens of the prompt.
//!
//! seed = lexical + W_SPATIAL · spatial; one hop of spreading activation over edges and
//! hyperedges; final = (relevance + 0.3·activation) · (0.6 + 0.4·χ).

use super::model::*;
use super::score::Score;
use super::spatial::{self, Target};
use super::text;

/// An exact-file anchor weighs as much as naming the memory literally in the prompt.
pub const W_SPATIAL: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct Recall {
    pub id: u32,
    pub relevance: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: String,
    pub targets: Vec<Target>,
}

fn admitted(s: &Score, min_chi: f32) -> bool {
    s.visible && s.chi >= min_chi
}

pub fn lexical_seeds(m: &Memory, scores: &[Score], query: &str, min_chi: f32) -> Vec<f32> {
    let q = text::tokens(query);
    let folded = text::fold(query);
    m.nodes
        .iter()
        .map(|node| {
            if !admitted(&scores[node.id as usize], min_chi) {
                return 0.0;
            }
            let mut r = 0.0;
            if std::iter::once(&node.label)
                .chain(node.aliases.iter())
                .any(|a| text::contains_phrase(&folded, a))
            {
                r += 2.0;
            }
            if !q.is_empty() {
                let doc = text::tokens(&format!("{} {} {}", node.label, node.text, node.path));
                let hits = q.iter().filter(|t| doc.contains(t)).count() as f32;
                r += hits / (doc.len() as f32).sqrt().max(1.0) * 1.5;
            }
            r
        })
        .collect()
}

/// One hop of spreading activation from the seeds, weighted by edge strength.
/// Causes bring the "why"; contradictions and replacements bring what was already tried.
pub fn spread(m: &Memory, scores: &[Score], seeds: &[f32], min_chi: f32) -> Vec<f32> {
    let mut rel = seeds.to_vec();
    for e in &m.edges {
        let w = match e.kind {
            EdgeKind::Causes => 0.45 * e.determinism(),
            EdgeKind::Relates => 0.25 + 0.5 * e.hebb,
            EdgeKind::Contradicts | EdgeKind::Supersedes => 0.3,
            EdgeKind::Mentions => 0.2,
            EdgeKind::Precedes => 0.15,
        };
        let (f, t) = (e.from as usize, e.to as usize);
        if seeds[f] > 0.0 && admitted(&scores[t], min_chi) {
            rel[t] += seeds[f] * w;
        }
        if seeds[t] > 0.0 && admitted(&scores[f], min_chi) {
            rel[f] += seeds[t] * w * 0.7;
        }
    }
    for h in &m.hyperedges {
        let peak = h.members.iter().map(|&i| seeds[i as usize]).fold(0.0f32, f32::max);
        if peak > 0.0 {
            for &i in &h.members {
                if admitted(&scores[i as usize], min_chi) {
                    rel[i as usize] += peak * 0.15;
                }
            }
        }
    }
    rel
}

/// `min_chi` filters what can come back: −1 lets discarded alternatives through (ranked low),
/// −0.25 keeps only what is not doubtful.
pub fn recall(m: &Memory, scores: &[Score], q: &Query, limit: usize, min_chi: f32) -> Vec<Recall> {
    let lex = lexical_seeds(m, scores, &q.text, min_chi);
    let spa = spatial::seeds(m, &q.targets);
    let seeds: Vec<f32> = m
        .nodes
        .iter()
        .map(|n| {
            let i = n.id as usize;
            if admitted(&scores[i], min_chi) {
                lex[i] + W_SPATIAL * spa[i]
            } else {
                0.0
            }
        })
        .collect();
    let rel = spread(m, scores, &seeds, min_chi);

    let mut out: Vec<Recall> = m
        .nodes
        .iter()
        .filter(|node| rel[node.id as usize] > 0.05 && !node.has(flags::CORE))
        .map(|node| {
            let chi = scores[node.id as usize].chi;
            let recency = 0.3 * node.activation;
            let trust = 0.6 + 0.4 * chi;
            Recall { id: node.id, relevance: (rel[node.id as usize] + recency) * trust }
        })
        .collect();
    out.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

/// Channel 0: every visible `CORE` node, most trusted first.
pub fn core(m: &Memory, scores: &[Score]) -> Vec<u32> {
    let mut ids: Vec<u32> = m
        .nodes
        .iter()
        .filter(|n| n.has(flags::CORE) && scores[n.id as usize].visible)
        .map(|n| n.id)
        .collect();
    ids.sort_by(|&a, &b| scores[b as usize].chi.partial_cmp(&scores[a as usize].chi).unwrap_or(std::cmp::Ordering::Equal));
    ids
}

/// Greedy cut to a token budget, in the given order. `render` is the line that will be injected.
pub fn fit_budget(m: &Memory, ids: &[u32], budget: usize, render: impl Fn(&Node) -> String) -> Vec<u32> {
    let mut used = 0;
    let mut out = Vec::new();
    for &id in ids {
        let cost = text::approx_tokens(&render(&m.nodes[id as usize]));
        if used + cost > budget {
            continue;
        }
        used += cost;
        out.push(id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::{learn, Claim};
    use crate::score::{score_all, View};

    #[test]
    fn touched_file_recalls_with_no_keywords() {
        let mut m = Memory::default();
        let ev = learn(&mut m, &Claim::rule("Nunca uses unwrap aquí", "src/core/collider.rs").because("Un panic tumbó el servidor en el partido 3"), 100.0);
        let rule = ev.iter().find(|e| e.label.contains("unwrap")).unwrap().node;
        let cause = ev.iter().find(|e| e.label.contains("panic")).unwrap().node;
        let dir = learn(&mut m, &Claim::rule("Todo el núcleo es determinista: sin rand", "src/core"), 101.0)[0].node;
        let ui = learn(&mut m, &Claim::rule("Los botones usan el tema oscuro", "src/ui"), 102.0)[0].node;
        let mut core = Claim::rule("Nunca hagas push a main", "");
        core.core = true;
        let core = learn(&mut m, &core, 103.0)[0].node;

        let s = score_all(&m, &View::all());
        let q = Query { text: "arréglalo".into(), targets: vec![Target::new("src/core/collider.rs", 0.8)] };
        let got: Vec<u32> = recall(&m, &s, &q, 10, -1.0).iter().map(|r| r.id).collect();
        assert!(got.contains(&rule), "exact file");
        assert!(got.contains(&dir), "inherited from the directory");
        assert!(got.contains(&cause), "the why comes along the causal edge");
        assert!(!got.contains(&ui), "unrelated subtree");
        assert!(!got.contains(&core), "core is channel 0, not repeated");
        assert_eq!(super::core(&m, &s), vec![core]);
        assert_eq!(got[0], rule, "the file's own rule ranks first");
    }
}
