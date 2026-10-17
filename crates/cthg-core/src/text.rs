//! Spanish/English tokenization for lexical matching.

const STOP: &[&str] = &[
    // es
    "a", "al", "algo", "ante", "aqui", "asi", "aun", "bajo", "bien", "cada", "como", "con", "contra",
    "cual", "cuando", "de", "del", "desde", "donde", "dos", "el", "ella", "ellas", "ellos", "en",
    "entre", "era", "eres", "es", "esa", "ese", "eso", "esta", "estas", "este", "esto", "estoy",
    "fue", "ha", "han", "hay", "la", "las", "le", "les", "lo", "los", "mas", "me", "mi", "mis",
    "mucho", "muy", "nada", "ni", "no", "nos", "o", "os", "para", "pero", "poco", "por", "porque",
    "que", "quien", "se", "sea", "ser", "si", "sin", "sobre", "solo", "son", "su", "sus", "tal",
    "te", "tan", "tengo", "ti", "tiene", "toda", "todo", "tu", "tus", "un", "una", "uno", "unos",
    "y", "ya", "yo", "hace", "vez", "haz", "hazlo",
    // en
    "the", "of", "and", "for", "with", "this", "that", "from", "are", "was", "were", "has", "have",
    "not", "but", "you", "your", "its", "into", "when", "then", "than", "there", "here", "use",
    "can", "will", "should", "must", "all", "any", "out", "also",
];

pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

/// Content tokens. Paths tokenize too: `src/core/collider.rs` → `src core collider`.
pub fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split_whitespace()
        .filter(|w| w.len() > 2 && !STOP.contains(w))
        .map(stem)
        .collect()
}

/// Very light stemming so "reglas" ~ "regla", "tests" ~ "test".
fn stem(w: &str) -> String {
    let w = w.trim_end_matches("mente");
    for suf in ["es", "s"] {
        if w.len() > 4 {
            if let Some(b) = w.strip_suffix(suf) {
                return b.to_string();
            }
        }
    }
    w.to_string()
}

/// Whole-phrase match of `needle` inside folded `hay` (word boundaries).
pub fn contains_phrase(hay_folded: &str, needle: &str) -> bool {
    let n = fold(needle);
    let n = n.trim();
    if n.len() < 3 {
        return false;
    }
    let padded = format!(" {} ", hay_folded.split_whitespace().collect::<Vec<_>>().join(" "));
    padded.contains(&format!(" {} ", n.split_whitespace().collect::<Vec<_>>().join(" ")))
}

/// How well claim `a` matches document `b`: half set-cosine, half coverage of `a` by `b`
/// (so a short claim can still match a long description). Duplicates ignored.
pub fn similarity(a: &[String], b: &[String]) -> f32 {
    use std::collections::HashSet;
    let a: HashSet<&String> = a.iter().collect();
    let b: HashSet<&String> = b.iter().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f32;
    let cosine = inter / ((a.len() * b.len()) as f32).sqrt();
    let coverage = inter / a.len() as f32;
    0.5 * cosine + 0.5 * coverage
}

pub fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Rough token count for budgets (≈ 4 characters per token).
pub fn approx_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

/// Deterministic jitter in [-1, 1] from a string, so positions are stable across runs.
pub fn hash_unit(s: &str, salt: u32) -> f32 {
    let mut h: u32 = 2166136261 ^ salt;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 20001) as f32 / 10000.0 - 1.0
}
