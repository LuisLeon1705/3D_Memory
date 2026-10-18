//! Learning from the user's corrections, at the end of every turn (`cthg hook stop`).
//!
//! Until now the memory only learned when the agent remembered to run `cthg learn`, and in
//! practice it does not. But a correction is already written in the transcript: the user's next
//! message says the previous turn missed ("sigue roto", "te dije que…", "no, …"). So:
//!
//! - `parse` reads a transcript (Claude Code or Antigravity, told apart line by line) into
//!   turns: one user message and the files the agent read or wrote until the next one;
//! - `detect` decides, with fixed word lists and no model, whether a message corrects the turn
//!   before it (the first message of a conversation never does);
//! - `captured` pairs each correction with the request it corrects and the files that turn
//!   wrote: a rule from the user, anchored there, caused by "the user corrected the request …",
//!   with a low prior (`CAPTURED_PRIOR`) and flagged `PROPOSED`: hidden from every channel but
//!   the read of its own files until `cthg confirm`. `learn` itself reinforces a memory the
//!   correction repeats instead of writing it twice;
//! - `report` counts corrections per conversation, with and without memory, to measure.

use super::antigravity::{self, Step};
use super::json::Json;
use super::learn::{self, Claim, LearnEvent};
use super::model::Memory;
use super::text;
use std::collections::HashSet;

/// Prior of a captured correction: (3; 1) → χ = 0,20 ("sin evidencia"), a proposal. One
/// `confirm`, or the same correction once more, makes it (4; 1): a rule the user said (χ 0,30).
pub const CAPTURED_PRIOR: (f32, f32) = (3.0, 1.0);
/// A correction with fewer content words ("siguen rotas") means nothing without its request,
/// so the request goes into the rule itself, not only into its cause.
pub const STANDALONE_TOKENS: usize = 3;
/// Weak cues ("está mal", "no funciona") count only this early in the message: a complaint
/// about the last turn comes first; further down it is usually a new bug report.
pub const CUE_WITHIN: usize = 12;
/// Words after "sigue / aún / still" in which a failure word must appear ("sigue sin funcionar").
pub const PERSISTENCE_WINDOW: usize = 4;
/// Every signal must appear in the first words: further down, a long message is usually a new
/// task or pasted text that merely quotes "arréglalo" or "again".
pub const SIGNAL_WITHIN: usize = 60;
/// A quote longer than this (characters) is pasted text, not a cited phrase: it runs to the end.
pub const LONG_QUOTE: usize = 300;
const CORRECTION_CHARS: usize = 280;
const REQUEST_CHARS: usize = 100;

/// Lines that carry what a memory hook injected: Claude Code `attachment`s, Antigravity
/// ephemeral or system messages. A file the agent read that quotes these headings does not count.
const INJECTED_TYPES: &[&str] = &["attachment", "EPHEMERAL_MESSAGE", "SYSTEM_MESSAGE"];
const MEMORY_HEADINGS: &[&str] = &["## Memoria relevante", "## Reglas del proyecto", "## Reglas globales"];
/// Claude Code tools that write a file, and the one that reads.
const CLAUDE_EDIT: &[&str] = &["Edit", "MultiEdit", "Write", "NotebookEdit"];

/// Corrections whatever their place in the message (folded words, accents and case removed).
const STRONG: &[&str] = &[
    // It was asked otherwise.
    "no lo habia pedido", "no lo pedi", "no te pedi", "yo no pedi", "no es lo que pedi",
    "no es lo que te pedi", "no era lo que pedi", "no es lo que queria", "no me referia",
    "no es a lo que me referia", "no era a lo que me referia", "esto no es lo que", "eso no es lo que",
    "no era eso", "te dije", "te lo dije", "te habia dicho",
    "te pedi que", "te habia pedido",
    // The last turn missed or broke something.
    "se te olvido", "te olvidaste", "olvidaste", "te falto", "te faltaron", "falto", "faltaron",
    "rompiste", "has roto", "fallaste", "te equivocaste", "te confundiste", "lo hiciste mal",
    "lo has hecho mal", "que le hiciste",
    // It had no effect, or failed the same way.
    "hizo nada", "no cambio nada", "no cambio", "mismo error", "mismo problema",
    // Redo or undo it.
    "arreglalo", "arreglala", "arreglalos", "arreglalas", "corrigelo", "corrigela", "hazlo de nuevo",
    "hazlo otra vez", "vuelve a hacerlo", "rehazlo", "deshaz", "deshazlo", "revierte",
    "devuelve los cambios", "como estaba antes",
    // English.
    "not what i asked", "not what i meant", "not what i wanted", "i didn t ask", "i did not ask",
    "i never asked", "i told you", "i asked you to", "like i said", "as i said", "you forgot",
    "you missed", "you broke", "you messed", "you got it wrong", "you misunderstood", "did nothing",
    "does nothing", "nothing changed", "didn t change", "same error", "same issue", "same problem",
    "fix it", "undo that", "undo it", "revert that", "revert it", "revert the changes", "do it again",
];

/// "sigue / aún / still" followed by one of `FAILURE`: the problem outlived the last turn.
const PERSISTENCE: &[&str] = &["sigue", "siguen", "aun", "todavia", "still"];
/// "otra vez / de nuevo / again" with a failure word on either side ("falla otra vez").
const REPETITION: &[&str] = &["otra vez", "de nuevo", "nuevamente", "again"];
/// Before the repetition only an explicit failure counts: "falla otra vez", but not "…no los
/// tiles, de nuevo funcionará por punteros".
const FAILED: &[&str] = &[
    "falla", "fallo", "fallan", "roto", "rota", "rotos", "rotas", "error", "errores", "crashea", "crasheo", "mal",
    "fails", "failed", "broken", "wrong", "crashes", "crashed", "caes", "caiste", "cae", "cayo", "repites",
    "repetiste",
];
const FAILURE: &[&str] = &[
    "roto", "rota", "rotos", "rotas", "rompiendo", "fallando", "falla", "fallan", "fallo", "mal", "igual",
    "sin", "no", "nada", "error", "errores", "mismo", "misma", "vacio", "vacia", "vacios", "vacias",
    "undefined", "alucinando", "crasheando", "dando", "saliendo", "apareciendo", "pasando", "broken",
    "failing", "fails", "failed", "wrong", "same", "not", "doesn", "isn", "didn", "nothing", "empty",
    "crashing", "giving", "showing", "happening", "errors",
];
/// "¿Por qué <verbo en pasado>?" asks the agent to account for what it did.
const WHY: &[&str] = &["por que", "why did you", "why have you"];
const WHY_VERBS: &[&str] = &[
    "hiciste", "cambiaste", "borraste", "quitaste", "tocaste", "modificaste", "eliminaste", "pusiste",
    "diste", "agregaste", "anadiste", "rompiste",
];

/// Cues that need the start of the message and a statement, not a question.
const CUES: &[&str] = &[
    "no funciona", "no funciono", "no funcionan", "no sirve", "no sirvio", "tampoco funciona", "no veo",
    "sigue sin", "te falta", "te faltan", "no lo arreglaste", "no se arreglo", "no quedo bien",
    "no esta bien", "no es legible", "no se entiende", "doesn t work", "didn t work", "not working",
    "isn t working", "i don t see",
];
/// "está / es / quedó … roto": a verdict on the result, within two words of the verb.
const COPULA: &[&str] = &["esta", "estan", "es", "son", "quedo", "quedaron", "sale", "salen", "is", "are", "looks", "s"];
const BAD: &[&str] = &[
    "roto", "rota", "rotos", "rotas", "mal", "malo", "mala", "malos", "malas", "malisimo", "malisima",
    "horrible", "errado", "errada", "equivocado", "equivocada", "incorrecto", "incorrecta", "vacio",
    "vacia", "vacios", "vacias", "broken", "wrong", "empty", "worse", "desastre", "ilegible",
];
/// Verdicts that need no verb: "tienes un desastre en la estructura", "el modelo es malísimo".
const VERDICT: &[&str] = &["malisimo", "malisima", "horrible", "incorrecto", "errado", "equivocado", "desastre", "ilegible"];
/// "no, …" opens a correction, except in these ("no, gracias", "no te preocupes").
const NO_IS_FINE: &[&str] = &[
    "gracias", "thanks", "thank", "problem", "worries", "te preocupes", "pasa nada", "hace falta",
    "importa", "need", "rush",
];
/// "no, también …" adds to the request instead of correcting it.
const ADDITION: &[&str] = &["tambien", "ademas", "y", "also", "and", "plus"];
/// A colon after these introduces someone else's words: "claude me dio esto: …", "dice: …".
const CITED_BY: &[&str] = &[
    "resultado", "dice", "dijo", "dio", "responde", "respondio", "escribio", "propone", "opina",
    "this", "says", "said", "wrote", "replied",
];

/// The user's own words: without quoted text ("…", «…», “…”, `…`; an unclosed quote, or one
/// longer than `LONG_QUOTE`, runs to the end) and without what follows a citation
/// ("antigravity da este resultado: …"). Quoting
/// "arréglalo" while talking about corrections, or pasting someone's report, corrects nothing.
pub fn own_words(message: &str) -> String {
    let mut cut = message.len();
    for (i, _) in message.match_indices(':') {
        let before = text::fold(&message[..i]);
        if before.split_whitespace().last().is_some_and(|w| CITED_BY.contains(&w)) {
            cut = i;
            break;
        }
    }
    let text = &message[..cut];
    let mut out = String::with_capacity(cut);
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let close = match c {
            '"' | '`' => Some(c),
            '“' => Some('”'),
            '«' => Some('»'),
            _ => None,
        };
        let Some(close) = close else {
            out.push(c);
            i += c.len_utf8();
            continue;
        };
        let start = i + c.len_utf8();
        match text[start..].find(close) {
            // A long quote is a pasted block, and its inner quotes would pair up wrongly
            // (closing it early and letting a quoted "arréglalo" out): it runs to the end.
            Some(len) if text[start..start + len].chars().count() <= LONG_QUOTE => i = start + len + close.len_utf8(),
            _ => break,
        }
    }
    out
}

/// Folded words; "denuevo" (a frequent typo) is "de nuevo".
fn words_of(message: &str) -> Vec<String> {
    let folded = text::fold(message);
    folded
        .split_whitespace()
        .flat_map(|w| if w == "denuevo" { vec!["de", "nuevo"] } else { vec![w] })
        .map(str::to_string)
        .collect()
}

/// Where `phrase` (space-separated words) starts in `words`.
fn find_phrase(words: &[String], phrase: &str) -> Option<usize> {
    let p: Vec<&str> = phrase.split(' ').collect();
    (0..words.len().saturating_sub(p.len() - 1)).find(|&i| p.iter().enumerate().all(|(k, w)| words[i + k] == *w))
}

fn has_any(words: &[String], set: &[&str]) -> bool {
    words.iter().any(|w| set.contains(&w.as_str()))
}

/// "sigue / aún / still" + a failure word in the next `PERSISTENCE_WINDOW` words. "Todavía no
/// lo he probado" is the user's own pending step, not the agent's failure.
fn persistence(words: &[String]) -> Option<&'static str> {
    for (i, w) in words.iter().enumerate() {
        let Some(p) = PERSISTENCE.iter().find(|p| **p == w.as_str()) else { continue };
        let after = &words[i + 1..(i + 1 + PERSISTENCE_WINDOW).min(words.len())];
        let users_own = after.first().is_some_and(|w| w == "no")
            && after.iter().skip(1).take(2).any(|w| w == "he" || w == "hemos");
        if has_any(after, FAILURE) && !users_own {
            return Some(p);
        }
    }
    for r in REPETITION {
        if let Some(i) = find_phrase(words, r) {
            let len = r.split(' ').count();
            let before = &words[i.saturating_sub(3)..i];
            let after = &words[i + len..(i + len + PERSISTENCE_WINDOW).min(words.len())];
            if has_any(before, FAILED) || has_any(after, FAILURE) {
                return Some(r);
            }
        }
    }
    for why in WHY {
        if let Some(i) = find_phrase(words, why) {
            let len = why.split(' ').count();
            if has_any(&words[i + len..(i + len + 3).min(words.len())], WHY_VERBS) {
                return Some(why);
            }
        }
    }
    None
}

/// "no, …", "nope", "casi, …", "mal, …" at the very start.
fn opener(message: &str, words: &[String]) -> Option<&'static str> {
    let lower = message.trim_start().to_lowercase();
    let first = words.first().map(String::as_str).unwrap_or("");
    let punct_after = |w: &str| lower.strip_prefix(w).and_then(|r| r.chars().next()).is_some_and(|c| ",.!;:".contains(c));
    match first {
        "nope" | "nop" | "nah" => return Some("nope"),
        "no" if punct_after("no") || words.get(1).is_some_and(|w| w == "no") => {
            let rest = words[1..].join(" ");
            let adds = words.get(1).is_some_and(|w| ADDITION.contains(&w.as_str()));
            if !adds && !NO_IS_FINE.iter().any(|ok| rest.starts_with(ok)) {
                return Some("no,");
            }
        }
        "casi" | "almost" if punct_after(first) => return Some("casi,"),
        "mal" | "wrong" if punct_after(first) => return Some("mal,"),
        _ => {}
    }
    None
}

/// A copula followed within two words by a bad result ("está muy mal", "is broken"), or a
/// verdict on its own ("malísimo").
fn verdict(words: &[String]) -> Option<&'static str> {
    for (i, w) in words.iter().enumerate() {
        if VERDICT.contains(&w.as_str()) {
            return BAD.iter().find(|b| **b == w.as_str()).copied();
        }
        if COPULA.contains(&w.as_str()) {
            if let Some(b) = words[i + 1..(i + 3).min(words.len())].iter().find_map(|x| BAD.iter().find(|b| **b == x.as_str())) {
                return Some(b);
            }
        }
    }
    None
}

/// Whether `message` corrects the turn before it, and what gave it away. Deterministic: fixed
/// lists, in order of strength, over the user's own words (`own_words`). Questions only count
/// with a strong signal ("¿por qué borraste…?").
pub fn detect(message: &str) -> Option<&'static str> {
    let own = own_words(message);
    let message = own.as_str();
    let mut words = words_of(message);
    words.truncate(SIGNAL_WITHIN);
    if words.is_empty() {
        return None;
    }
    if let Some(p) = STRONG.iter().find(|p| find_phrase(&words, p).is_some()) {
        return Some(p);
    }
    if let Some(p) = persistence(&words) {
        return Some(p);
    }
    if message.trim_end().ends_with('?') {
        return None;
    }
    if let Some(o) = opener(message, &words) {
        return Some(o);
    }
    let head = &words[..words.len().min(CUE_WITHIN)];
    CUES.iter().find(|p| find_phrase(head, p).is_some()).copied().or_else(|| verdict(head))
}

/// One user message and what the agent did until the next one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    pub request: String,
    /// Files written or edited (paths as the agent gave them).
    pub edits: Vec<String>,
    pub reads: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversation {
    /// Day of the first user message (`AAAA-MM-DD`), when the transcript has it.
    pub date: String,
    pub turns: Vec<Turn>,
    /// A memory hook injected something in this conversation.
    pub with_memory: bool,
}

/// Removes what the user did not write: system reminders and pasted blocks.
fn strip_blocks(raw: &str) -> String {
    let mut s = raw.to_string();
    for (open, close) in [("<system-reminder>", "</system-reminder>"), ("<pasted_content", "</pasted_content>")] {
        while let Some(a) = s.find(open) {
            let end = s[a..].find(close).map(|b| a + b + close.len()).unwrap_or(s.len());
            s.replace_range(a..end, " ");
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Typed by a person: not a notification, command output, interruption marker or side question.
fn is_human(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('<')
        && !["[Request interrupted", "[Image", "This is a side question"].iter().any(|p| text.starts_with(p))
}

fn flag(v: &Json, key: &str) -> bool {
    v.get(key) == Some(&Json::Bool(true))
}

/// The person's words in a Claude Code `user` line, if a person wrote it. The transcript format
/// is internal to Claude Code, so every field is optional and anything unexpected is skipped.
fn claude_user_text(v: &Json) -> Option<String> {
    if ["isMeta", "isCompactSummary", "isVisibleInTranscriptOnly"].iter().any(|k| flag(v, k)) {
        return None;
    }
    if let Some(kind) = v.get("origin").and_then(|o| o.get("kind")).and_then(Json::str) {
        if kind != "human" {
            return None;
        }
    }
    let raw = match v.get("message")?.get("content")? {
        Json::Str(s) => s.clone(),
        Json::Arr(blocks) => {
            if blocks.iter().any(|b| b.get("type").and_then(Json::str) == Some("tool_result")) {
                return None;
            }
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(Json::str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Json::str))
                .collect();
            texts.join("\n")
        }
        _ => return None,
    };
    let text = strip_blocks(&raw);
    is_human(&text).then_some(text)
}

/// Files a Claude Code `assistant` line reads or writes.
fn claude_tools(v: &Json, steps: &mut Vec<Step>) {
    for b in v.get("message").and_then(|m| m.get("content")).map(Json::arr).unwrap_or(&[]) {
        if b.get("type").and_then(Json::str) != Some("tool_use") {
            continue;
        }
        let name = b.get("name").and_then(Json::str).unwrap_or("");
        let arg = |k: &str| b.get("input").and_then(|i| i.get(k)).and_then(Json::str).map(str::to_string);
        if CLAUDE_EDIT.contains(&name) {
            steps.extend(arg("file_path").or_else(|| arg("notebook_path")).map(Step::Edit));
        } else if name == "Read" {
            steps.extend(arg("file_path").map(Step::Read));
        }
    }
}

/// Reads a whole transcript, Claude Code (`type` user/assistant) or Antigravity (`USER_INPUT`,
/// `tool_calls`). Lines that do not parse (a half-written last one) are skipped.
pub fn parse(transcript: &str) -> Conversation {
    let mut conv = Conversation::default();
    let mut steps: Vec<Step> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for line in transcript.lines() {
        let Some(v) = Json::parse(line) else { continue };
        let kind = v.get("type").and_then(Json::str).unwrap_or("");
        if INJECTED_TYPES.contains(&kind) && MEMORY_HEADINGS.iter().any(|h| line.contains(h)) {
            conv.with_memory = true;
        }
        // Subagents' prompts come from the main agent, not from the user.
        if flag(&v, "isSidechain") {
            continue;
        }
        let before = steps.len();
        match kind {
            "user" => {
                // Forked or resumed sessions may repeat a line: one message per uuid.
                let fresh = v.get("uuid").and_then(Json::str).is_none_or(|u| seen.insert(u.to_string()));
                if let Some(text) = claude_user_text(&v).filter(|_| fresh) {
                    steps.push(Step::User { index: 0, text });
                }
            }
            "assistant" => claude_tools(&v, &mut steps),
            _ => {
                let mut found = Vec::new();
                antigravity::line_steps(&v, &mut found);
                for s in found {
                    match s {
                        Step::User { index, text } => {
                            let text = strip_blocks(&text);
                            if is_human(&text) {
                                steps.push(Step::User { index, text });
                            }
                        }
                        other => steps.push(other),
                    }
                }
            }
        }
        let new_user = steps[before..].iter().any(|s| matches!(s, Step::User { .. }));
        if new_user && conv.date.is_empty() {
            let at = v.get("timestamp").or_else(|| v.get("created_at")).and_then(Json::str).unwrap_or("");
            conv.date = at.chars().take(10).collect();
        }
    }
    for s in steps {
        match s {
            // The same message sent twice in a row is a resend, not a second turn.
            Step::User { text, .. } if conv.turns.last().is_some_and(|t| t.request == text) => {}
            Step::User { text, .. } => conv.turns.push(Turn { request: text, ..Turn::default() }),
            // Messages from another agent (subagent assignments, rejections, reports) are not user turns.
            Step::Agent { .. } => {}
            Step::Edit(p) => {
                if let Some(t) = conv.turns.last_mut() {
                    t.edits.push(p);
                }
            }
            Step::Read(p) => {
                if let Some(t) = conv.turns.last_mut() {
                    t.reads.push(p);
                }
            }
        }
    }
    conv
}

/// For each turn, what made it a correction (`None` for the first message, which corrects nothing).
pub fn corrections(conv: &Conversation) -> Vec<Option<&'static str>> {
    conv.turns.iter().enumerate().map(|(i, t)| if i == 0 { None } else { detect(&t.request) }).collect()
}

/// Where the rule goes: the one file, else the deepest folder holding them all, else (they
/// only meet at the root, which recalls nothing) the file edited most, the first on a tie.
pub fn anchor(paths: &[String]) -> String {
    let mut uniq: Vec<(&str, usize)> = Vec::new();
    for p in paths.iter().filter(|p| !p.is_empty()) {
        match uniq.iter_mut().find(|(q, _)| q == p) {
            Some(e) => e.1 += 1,
            None => uniq.push((p, 1)),
        }
    }
    if uniq.len() <= 1 {
        return uniq.first().map(|(p, _)| p.to_string()).unwrap_or_default();
    }
    let mut common: Option<Vec<&str>> = None;
    for (p, _) in &uniq {
        let parts: Vec<&str> = p.split('/').collect();
        let dir = &parts[..parts.len() - 1];
        common = Some(match common {
            None => dir.to_vec(),
            Some(c) => c.iter().zip(dir).take_while(|(a, b)| a == b).map(|(a, _)| *a).collect(),
        });
    }
    let common = common.unwrap_or_default().join("/");
    if !common.is_empty() {
        return common;
    }
    uniq.iter().rev().max_by_key(|(_, n)| *n).map(|(p, _)| p.to_string()).unwrap_or_default()
}

/// A correction ready to learn.
#[derive(Clone, Debug)]
pub struct Captured {
    /// Index of the correcting turn.
    pub turn: usize,
    pub trigger: &'static str,
    /// The request it corrects: the last one before it that was not itself a correction.
    pub request: String,
    pub correction: String,
    pub claim: Claim,
    /// Identity across hook runs and resumed sessions, so a correction is learned once.
    pub key: String,
}

/// FNV-1a, 64 bits, in hex: stable across runs and platforms.
fn fingerprint(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Every correction in `conv`, paired with its request and the files of the turn it corrects.
/// `relative` turns an agent's path into a repo path ("" = outside the repo or noise). Files
/// come from the corrected turn's edits, else the correcting turn's (the fix), else the files
/// the corrected turn read.
pub fn captured(conv: &Conversation, relative: impl Fn(&str) -> String) -> Vec<Captured> {
    let marks = corrections(conv);
    let mut out = Vec::new();
    for (i, trigger) in marks.iter().enumerate() {
        let Some(trigger) = *trigger else { continue };
        let mut j = i - 1;
        while j > 0 && marks[j].is_some() {
            j -= 1;
        }
        let request = conv.turns[j].request.clone();
        let correction = conv.turns[i].request.clone();
        let rel = |ps: &[String]| -> Vec<String> { ps.iter().map(|p| relative(p)).filter(|p| !p.is_empty()).collect() };
        let prev = &conv.turns[i - 1];
        let path = [rel(&prev.edits), rel(&conv.turns[i].edits), rel(&prev.reads)]
            .into_iter()
            .find(|ps| !ps.is_empty())
            .map(|ps| anchor(&ps))
            .unwrap_or_default();
        let c = text::clip(&correction, CORRECTION_CHARS);
        let p = text::clip(&request, REQUEST_CHARS);
        // The request is said once: in the cause when the correction stands alone, in the rule
        // when it does not (then the cause quotes the short correction instead).
        let standalone = text::tokens(&c).len() >= STANDALONE_TOKENS || text::tokens(&p).is_empty();
        let (body, cause) = if standalone {
            let cause = if text::tokens(&p).is_empty() { format!("Corrección del usuario: «{}»", text::clip(&c, 60)) } else { format!("Corrección del usuario al pedir «{p}»") };
            (c, cause)
        } else {
            (format!("{c} (al pedir «{p}»)"), format!("Corrección del usuario: «{c}»"))
        };
        if text::tokens(&body).len() < 2 {
            continue;
        }
        let mut claim = Claim::rule(&body, &path);
        claim.prior = Some(CAPTURED_PRIOR);
        claim.proposed = true;
        claim.cause = Some(cause);
        // The turn too: the same words said twice are two corrections (the second reinforces).
        let key = fingerprint(&format!("{i}\u{1f}{}\u{1f}{}", text::fold(&request), text::fold(&correction)));
        out.push(Captured { turn: i, trigger, request, correction, claim, key });
    }
    out
}

/// Learns one captured correction. The incident written as its cause gets no path: the rule
/// already shows it (`← Corrección del usuario…`), and anchored at the same file it would be
/// injected again as a line of its own, repeating the same words.
pub fn learn_correction(m: &mut Memory, claim: &Claim, now: f64) -> Vec<LearnEvent> {
    let events = learn::learn(m, claim, now);
    for e in events.iter().filter(|e| e.action == "nueva causa") {
        if let Some(n) = m.node_mut(e.node) {
            n.path.clear();
        }
    }
    events
}

/// Corrections in one conversation, for `report`.
#[derive(Clone, Debug)]
pub struct ConvStats {
    pub name: String,
    pub date: String,
    pub messages: usize,
    pub with_memory: bool,
    /// (turn, trigger, message)
    pub corrections: Vec<(usize, &'static str, String)>,
}

pub fn stats(name: &str, conv: &Conversation) -> ConvStats {
    let corrections = corrections(conv)
        .into_iter()
        .enumerate()
        .filter_map(|(i, t)| t.map(|t| (i, t, conv.turns[i].request.clone())))
        .collect();
    ConvStats { name: name.to_string(), date: conv.date.clone(), messages: conv.turns.len(), with_memory: conv.with_memory, corrections }
}

/// `1,25`: decimals as the rest of the Spanish output writes them.
fn dec(x: f32) -> String {
    format!("{x:.2}").replace('.', ",")
}

/// A row of `report`.
#[derive(Clone, Copy)]
enum Group<'a> {
    Memory(bool),
    Before(&'a str),
    Since(&'a str),
    All,
}

impl Group<'_> {
    fn label(&self) -> String {
        match self {
            Group::Memory(true) => "con memoria".into(),
            Group::Memory(false) => "sin memoria".into(),
            Group::Before(d) => format!("antes del {d}"),
            Group::Since(d) => format!("desde el {d}"),
            Group::All => "total".into(),
        }
    }
    fn holds(&self, c: &ConvStats) -> bool {
        match self {
            Group::Memory(w) => c.with_memory == *w,
            Group::Before(d) => !c.date.is_empty() && c.date.as_str() < *d,
            Group::Since(d) => !c.date.is_empty() && c.date.as_str() >= *d,
            Group::All => true,
        }
    }
}

/// Corrections per conversation, with and without memory, and before and after `since`
/// (`AAAA-MM-DD`) if given. The share is over the messages that can correct something (all but
/// the first of each conversation).
pub fn report(convs: &[ConvStats], since: Option<&str>, detail: bool) -> String {
    let convs: Vec<&ConvStats> = convs.iter().filter(|c| c.messages > 0).collect();
    let mut out = format!("# Correcciones del usuario ({} conversaciones)\n\n", convs.len());
    out.push_str("| Grupo | Conversaciones | Mensajes | Correcciones | Por conversación | % de respuestas corregidas |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|\n");
    let mut groups = vec![Group::Memory(true), Group::Memory(false)];
    if let Some(d) = since {
        groups.extend([Group::Before(d), Group::Since(d)]);
    }
    groups.push(Group::All);
    for group in groups {
        let name = group.label();
        let g: Vec<&&ConvStats> = convs.iter().filter(|c| group.holds(c)).collect();
        if g.is_empty() {
            continue;
        }
        let msgs: usize = g.iter().map(|c| c.messages).sum();
        let corr: usize = g.iter().map(|c| c.corrections.len()).sum();
        let answers = msgs - g.len();
        let share = if answers == 0 { 0.0 } else { corr as f32 / answers as f32 };
        out.push_str(&format!(
            "| {name} | {} | {msgs} | {corr} | {} | {:.0} % |\n",
            g.len(),
            dec(corr as f32 / g.len() as f32),
            share * 100.0
        ));
    }
    if detail {
        let mut sorted = convs.clone();
        sorted.sort_by(|a, b| a.date.cmp(&b.date).then(a.name.cmp(&b.name)));
        out.push_str("\n## Por conversación\n");
        for c in sorted {
            let memory = if c.with_memory { "con memoria" } else { "sin memoria" };
            let date = if c.date.is_empty() { "?" } else { &c.date };
            out.push_str(&format!("\n- {date} · {} · {memory} · {} mensajes · {} correcciones\n", c.name, c.messages, c.corrections.len()));
            for (turn, trigger, msg) in &c.corrections {
                out.push_str(&format!("  - #{turn} [{trigger}] «{}»\n", text::clip(msg, 110)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::learn;
    use crate::model::*;
    use crate::score::{self, View};

    #[test]
    fn detects_the_users_corrections_and_not_new_requests() {
        // Phrasings from the task and from the user's real Antigravity transcripts.
        let corrections = [
            "arréglalo",
            "no lo había pedido así",
            "esto no es a lo que me refería",
            "no, el botón va arriba",
            "otra vez falla el build",
            "te dije que los mensajes en español",
            "siguen rotas",
            "Sigue saleindo undefined, al menos ahora la lista si cambia",
            "no hizo nada, se siente exactamente igual al original",
            "Nope,{\"detailed\":[],\"general\":[]}",
            "el guardamanos esta muy mal diseñado, hazlo denuevo",
            "mmm, rompiste un poco la logica",
            "casi, hay un cilindro que sobresale del cuerpo",
            "se te olvido algo sutil, no añadiste el gitignore",
            "el modelo es malisimo, puedes ejecutar blender tu mismo",
            "los spawns estan todos rotos, y el metro tambien",
            "ya cerre la terminal y todo, mismo error",
            "¿por qué borraste el test?",
            "that's not what I asked for",
            "its still giving 0.1, and still picks the third skill, check again",
            "I told you to keep the API stable",
            "no, sigue alucinando",
        ];
        for c in corrections {
            assert!(detect(c).is_some(), "should be a correction: {c}");
        }
        let requests = [
            "genial, siguiente fase, la numero 3,",
            "que tanto te falta?",
            "no sé qué es mejor, ¿tú qué opinas?",
            "no, gracias, así está bien",
            "todavía no lo he probado, haz el commit",
            "/btw sigue trabajando?",
            "No hay, no aqui, por ahora este proyecto esta terminado",
            "adelante",
            "añade un endpoint de pagos con su test",
            "no le digo a gemini que lo vaya desarrollando?",
            "hay un monton de operadores que se calculan con los buffs de la version anterior y deberiamos revisar los que estan mal",
            "perdon, te interrumpi por error",
            "el boton de debug solo debe guardar los frames, no los tiles, de nuevo funcionara por punteros",
        ];
        for r in requests {
            assert_eq!(detect(r), None, "should not be a correction: {r}");
        }
        assert_eq!(detect("no, gracias"), None);

        // The design conversation of this repo: its two real corrections, and the three messages
        // the first version took for corrections (an addition, a quoted example, a quoted report).
        let missed = [
            "tienes un desastre en la estructura dos visuales de los nodos, ademas de que no hay cosas como nodos de apariencia, eso no es legible, sabes de constelaciones? graficamente deberia de parecerse mas a eso, borra el grafico",
            "seria un fix? alli es donde caes nuevamente, es memoria tridimensional, me puedes recordar que hace la teoria del caos a partir de 3 variables?",
        ];
        for c in missed {
            assert!(detect(c).is_some(), "should be a correction: {c}");
        }
        let quoted = [
            "no, tambien esta este manga: https://www.arknights.global/comic-detail?name=Prelude+Suite:+Old+Wounds&chapter=1",
            "esa es la parte donde hago trampas, lo fabrique a beneficiar mi estilo, el sistema se aprende sin constante recordatorio, esencialmente reducira los prompts innecesarios de \"arreglalo\" \"yo no lo habia pedido asi\" \"esto no es a lo que me referia\" al permitir que el sistema pueda autocompletar las cosas",
            "pregunta esto, opiniones? \"La memoria ya aprende sola las correcciones con el nuevo hook `Stop`. Tests y clippy limpios. No hay commit, y los hooks de este repo todavía no tienen `Stop`.\"",
            "por cierto, te hice trampa de nuevo, en lo que tu otro chat termina, piensa que el beneficio de 15k antiguos a 10k actuales fue antes de cualquier compact",
            "claude me dio esto, quieres testearlo? \"Real messages are vaguer (“se apaga solo, arréglalo”); that’s where the lexical channel struggles",
            "antigravity da este resultado: sigue fallando el recall en dos casos y no funciona el léxico",
        ];
        for q in quoted {
            assert_eq!(detect(q), None, "quoted, cited or added, not a correction: {q}");
        }
        assert_eq!(own_words("te dije que «no» y luego \"sí, sin cerrar"), "te dije que  y luego ");
        assert_eq!(own_words("Claude dice: rompiste el build"), "Claude dice");
        let pasted = format!("gemini propone esto como fix \"Cómo blindarlo {} y luego \"arréglalo\" con más texto", "palabra ".repeat(60));
        assert_eq!(own_words(&pasted), "gemini propone esto como fix ", "a long quote with quotes inside runs to the end");
        assert_eq!(detect(&pasted), None);
        // A long task that only quotes a correction far down is a new task.
        let long = format!("{} y el ejemplo del caso dice arréglalo", "implementa la fase seis con sus casos de prueba".repeat(8));
        assert_eq!(detect(&long), None);
        assert_eq!(detect("sigue sin funcionar"), Some("sigue"));
    }

    // Shapes copied from a real Claude Code transcript (`~/.claude/projects/<proyecto>/<sesión>.jsonl`).
    const CLAUDE: &str = concat!(
        r###"{"type":"attachment","attachment":{"type":"hook_success","hookEvent":"SessionStart","content":"## Reglas del proyecto (siempre)\n- #1 regla: x"}}"###, "\n",
        r#"{"type":"user","uuid":"u1","timestamp":"2026-10-16T10:00:00Z","origin":{"kind":"human"},"message":{"role":"user","content":"añade el comando import al CLI"}}"#, "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"C:\\repo\\crates\\cli\\src\\main.rs"}}]}}"#, "\n",
        r###"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"## Memoria relevante (citada en un archivo)"}]}}"###, "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"C:\\repo\\crates\\cli\\src\\main.rs","old_string":"a"}},{"type":"tool_use","name":"Write","input":{"file_path":"C:\\repo\\crates\\cli\\src\\import.rs"}}]}}"#, "\n",
        r#"{"type":"user","isMeta":true,"message":{"content":"[Usage limit reached]"}}"#, "\n",
        r#"{"type":"user","uuid":"u2","origin":{"kind":"task-notification"},"message":{"content":"<task-notification>x</task-notification>"}}"#, "\n",
        r#"{"type":"user","uuid":"u3","isSidechain":true,"message":{"content":"no, eso es del subagente"}}"#, "\n",
        r#"{"type":"user","uuid":"u4","origin":{"kind":"human"},"message":{"content":[{"type":"text","text":"no, te dije que los mensajes del CLI van en español <system-reminder>ruido</system-reminder>"}]}}"#, "\n",
        r#"{"type":"user","uuid":"u4","origin":{"kind":"human"},"message":{"content":[{"type":"text","text":"no, te dije que los mensajes del CLI van en español"}]}}"#, "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"C:\\repo\\crates\\cli\\src\\main.rs"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"u5","origin":{"kind":"human"},"message":{"content":"siguen rotas"}}"#, "\n",
        r#"{"type":"user","uuid":"u6","origin":{"kind":"hum"#,
    );

    fn rel(p: &str) -> String {
        crate::hook::relativize_path(std::path::Path::new(r"C:\repo"), p)
    }

    #[test]
    fn reads_claude_code_transcripts_into_turns() {
        let conv = parse(CLAUDE);
        assert!(conv.with_memory, "the SessionStart attachment carries the memory");
        assert_eq!(conv.date, "2026-10-16");
        let requests: Vec<&str> = conv.turns.iter().map(|t| t.request.as_str()).collect();
        assert_eq!(
            requests,
            ["añade el comando import al CLI", "no, te dije que los mensajes del CLI van en español", "siguen rotas"],
            "no meta lines, notifications, subagents, tool results or repeated uuids; reminders stripped"
        );
        assert_eq!(conv.turns[0].edits, [r"C:\repo\crates\cli\src\main.rs", r"C:\repo\crates\cli\src\import.rs"]);
        assert_eq!(conv.turns[0].reads, [r"C:\repo\crates\cli\src\main.rs"]);
        assert_eq!(corrections(&conv), [None, Some("te dije"), Some("siguen")]);

        // A file the agent read that quotes the headings is not an injection.
        let quoted = parse(&CLAUDE.lines().filter(|l| !l.contains("attachment")).collect::<Vec<_>>().join("\n"));
        assert!(!quoted.with_memory);
    }

    #[test]
    fn pairs_each_correction_with_its_request_and_files() {
        let conv = parse(CLAUDE);
        let caps = captured(&conv, rel);
        assert_eq!(caps.len(), 2);

        let first = &caps[0];
        assert_eq!(first.request, "añade el comando import al CLI");
        assert_eq!(first.claim.path, "crates/cli/src", "two files: their deepest common folder");
        assert_eq!(first.claim.text, "no, te dije que los mensajes del CLI van en español", "says enough on its own");
        assert_eq!(first.claim.origin, Origin::User);
        assert_eq!(first.claim.prior, Some(CAPTURED_PRIOR));
        assert_eq!(first.claim.cause.as_deref(), Some("Corrección del usuario al pedir «añade el comando import al CLI»"));

        // A correction of a correction still corrects the original request; too short to stand
        // alone, it carries the request in its text.
        let second = &caps[1];
        assert_eq!(second.request, "añade el comando import al CLI");
        assert_eq!(second.claim.text, "siguen rotas (al pedir «añade el comando import al CLI»)");
        assert_eq!(second.claim.cause.as_deref(), Some("Corrección del usuario: «siguen rotas»"), "the request is said once");
        assert_eq!(second.claim.path, "crates/cli/src/main.rs", "what the corrected turn edited");
        assert_ne!(first.key, second.key);
        assert_eq!(captured(&parse(CLAUDE), rel)[0].key, first.key, "same correction, same key on every run");
    }

    #[test]
    fn a_captured_correction_is_a_proposal_and_a_repeated_one_reinforces() {
        let mut m = Memory::default();
        let existing = learn(&mut m, &Claim::rule("Los mensajes del CLI van en español", "crates/cli"), 1.0)[0].node;
        let alpha = m.nodes[existing as usize].alpha;

        // It repeats a rule already there: α + 1, no new node.
        let caps = captured(&parse(CLAUDE), rel);
        let ev = learn_correction(&mut m, &caps[0].claim, 2.0);
        assert_eq!((ev[0].node, ev[0].action), (existing, "refuerza"));
        assert_eq!(m.nodes[existing as usize].alpha, alpha + 1.0);

        // A new one starts low: (3; 1) → χ 0,20, "sin evidencia", with its cause.
        let ev = learn_correction(&mut m, &caps[1].claim, 3.0);
        let rule = ev.last().unwrap().node;
        assert_eq!(ev.last().unwrap().action, "nuevo");
        let n = &m.nodes[rule as usize];
        assert_eq!((n.alpha, n.beta, n.origin), (3.0, 1.0, Origin::User));
        let chi = score::score_all(&m, &View::all())[rule as usize].chi;
        assert!((chi - 0.20).abs() < 1e-3, "χ = {chi}");
        let cause = ev.iter().find(|e| e.action == "nueva causa").expect("its cause").node;
        // Reading the file brings the rule with its cause on the same line, not the cause again.
        let mut session = crate::hook::SessionState::new("s");
        let out = crate::hook::read_context(&mut m, &View::all(), &mut session, "crates/cli/src/main.rs");
        assert!(out.contains("siguen rotas (al pedir «añade el comando import al CLI») ← Corrección del usuario: «siguen rotas»"), "{out}");
        assert!(!session.was_injected(cause) && m.nodes[cause as usize].path.is_empty(), "{out}");

        // Unconfirmed, it is a proposal: no message brings it, however well it matches, and
        // another file's read does not either.
        assert!(m.nodes[rule as usize].has(flags::PROPOSED) && m.nodes[cause as usize].has(flags::PROPOSED));
        let at_main = || vec![crate::spatial::Target::new("crates/cli/src/main.rs", 0.8)];
        let mut fresh = crate::hook::SessionState::new("p");
        let out = crate::hook::handle_prompt(&mut m, &View::all(), &mut fresh, "siguen rotas, añade el comando import", at_main());
        assert!(!fresh.was_injected(rule) && !fresh.was_injected(cause), "{out}");
        crate::hook::read_context(&mut m, &View::all(), &mut fresh, "crates/cli/src/import.rs");
        assert!(!fresh.was_injected(rule), "not one of its files");

        // A deliberate `learn` saying the same is not swallowed by the hidden proposal.
        let said = learn(&mut m, &Claim::rule("siguen rotas al pedir añade el comando import al CLI", "crates/cli/src/main.rs"), 4.0);
        assert_ne!(said[0].node, rule);
        assert!(!m.nodes[said[0].node as usize].has(flags::PROPOSED));

        // One confirm: it weighs like a rule the user said (χ 0,30) and is injected everywhere.
        crate::learn::feedback(&mut m, rule, 1);
        assert!(!m.nodes[rule as usize].has(flags::PROPOSED));
        let chi = score::score_all(&m, &View::all())[rule as usize].chi;
        assert!((chi - 0.30).abs() < 1e-3, "χ = {chi}");
        let mut after = crate::hook::SessionState::new("c");
        crate::hook::handle_prompt(&mut m, &View::all(), &mut after, "siguen rotas, añade el comando import", at_main());
        assert!(after.was_injected(rule));
    }

    #[test]
    fn denying_a_proposal_keeps_it_hidden() {
        let mut m = Memory::default();
        let caps = captured(&parse(CLAUDE), rel);
        let rule = learn_correction(&mut m, &caps[1].claim, 1.0).last().unwrap().node;
        crate::learn::feedback(&mut m, rule, -1);
        assert!(m.nodes[rule as usize].has(flags::PROPOSED));
        let mut session = crate::hook::SessionState::new("d");
        crate::hook::handle_prompt(&mut m, &View::all(), &mut session, "siguen rotas", vec![crate::spatial::Target::new("crates/cli/src/main.rs", 0.8)]);
        assert!(!session.was_injected(rule));
    }

    #[test]
    fn anchors_at_one_file_a_common_folder_or_the_most_edited_file() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(anchor(&s(&["src/a.rs", "src/a.rs"])), "src/a.rs");
        assert_eq!(anchor(&s(&["src/core/a.rs", "src/core/b/c.rs"])), "src/core");
        assert_eq!(anchor(&s(&["GUIA.md", "src/a.rs", "src/a.rs"])), "src/a.rs", "they only meet at the root");
        assert_eq!(anchor(&s(&["GUIA.md", "src/a.rs"])), "GUIA.md", "a tie goes to the first");
        assert_eq!(anchor(&[]), "");
    }

    // Shapes copied from real Antigravity transcripts (`brain/<id>/.system_generated/logs/transcript.jsonl`).
    const AGY: &str = concat!(
        r#"{"step_index":0,"type":"USER_INPUT","created_at":"2026-10-15T01:59:18Z","content":"<USER_REQUEST>\narregla las ventanas\n</USER_REQUEST>"}"#, "\n",
        r###"{"step_index":1,"type":"EPHEMERAL_MESSAGE","content":"## Memoria relevante\n- #3 regla: x"}"###, "\n",
        r#"{"step_index":2,"type":"PLANNER_RESPONSE","tool_calls":[{"name":"replace_file_content","args":{"TargetFile":"C:\\repo\\src\\windows.rs"}}]}"#, "\n",
        r#"{"step_index":3,"type":"USER_INPUT","content":"<USER_REQUEST>This is a side question from the user. Answer directly.</USER_REQUEST>"}"#, "\n",
        r#"{"step_index":4,"type":"USER_INPUT","content":"<USER_REQUEST>siguen rotas</USER_REQUEST>"}"#, "\n",
        r#"{"step_index":5,"type":"USER_INPUT","content":"<USER_REQUEST>siguen rotas</USER_REQUEST>"}"#, "\n",
        r#"{"step_index":5,"type":"USER_INPUT","content":"<USER_REQUEST>genial, ahora el metro</USER_REQUEST>"}"#, "\n",
    );

    #[test]
    fn reads_antigravity_transcripts_and_counts_corrections() {
        let conv = parse(AGY);
        assert!(conv.with_memory);
        assert_eq!(conv.turns.len(), 3, "the side question is not the user's message, a resend is not a turn");
        assert_eq!(conv.turns[0].edits, [r"C:\repo\src\windows.rs"]);
        let caps = captured(&conv, rel);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].claim.path, "src/windows.rs");

        let mut plain = parse(&AGY.replace("EPHEMERAL_MESSAGE", "PLANNER_RESPONSE"));
        plain.date = String::new();
        let all = [stats("a1", &conv), stats("b2", &plain), stats("vacía", &Conversation::default())];
        let out = report(&all, None, true);
        assert!(out.contains("| con memoria | 1 | 3 | 1 | 1,00 | 50 % |"), "{out}");
        assert!(out.contains("| sin memoria | 1 | 3 | 1 | 1,00 | 50 % |"), "{out}");
        assert!(out.contains("| total | 2 | 6 | 2 | 1,00 | 50 % |"), "{out}");
        assert!(out.contains("- 2026-10-15 · a1 · con memoria · 3 mensajes · 1 correcciones"), "{out}");
        assert!(out.contains("  - #1 [siguen] «siguen rotas»"), "{out}");
        assert!(out.starts_with("# Correcciones del usuario (2 conversaciones)"), "empty conversations are left out");

        // Before and after a date (when the hooks were installed, say); undated ones only in the total.
        let out = report(&all, Some("2026-10-01"), false);
        assert!(out.contains("| desde el 2026-10-01 | 1 | 3 | 1 | 1,00 | 50 % |"), "{out}");
        assert!(!out.contains("antes del"), "no dated conversation before: no row
{out}");
        assert!(!out.contains("## Por conversación"));
    }
}
