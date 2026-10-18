//! `.cthg` binary format, little endian.
//!
//! ```text
//! "CTHG" | u16 version | u16 reserved | u32 section_count
//! section* : [u8;4] tag | u32 payload_len | payload
//! u32 crc32 (IEEE) of every byte before it
//! ```
//! Sections: META, NODE, EDGE, HYPE. Strings are u32 length + UTF-8.
//! Unknown sections are skipped, so newer writers stay readable.
//! Saves are atomic: write a per-writer `.tmp`, keep the previous file as `.bak`, rename.
//! A corrupt file loads from its `.bak`.

use super::model::*;
use std::fs;
use std::io;
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"CTHG";
/// 2: flag bit 4 (`STALE`) exists. Same layout as 1, so version 1 files still load.
/// 3: flag bit 5 (`PROPOSED`) exists. Same layout again; a file is written as 3 only when a
/// node carries that bit, so readers of version 2 still open every memory without proposals.
pub const VERSION: u16 = 3;
/// What a memory without the newest flags is written as.
const VERSION_WITHOUT_PROPOSALS: u16 = 2;
pub const OLDEST_READABLE: u16 = 1;

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn ids(&mut self, ids: &[u32]) {
        self.u32(ids.len() as u32);
        for &i in ids {
            self.u32(i);
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("cthg: {msg}"))
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if self.p + n > self.b.len() {
            return Err(bad("archivo truncado"));
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad("utf-8 inválido"))
    }
    fn ids(&mut self) -> io::Result<Vec<u32>> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u32()).collect()
    }
    fn done(&self) -> bool {
        self.p >= self.b.len()
    }
}

pub fn encode(m: &Memory) -> Vec<u8> {
    let mut sections: Vec<([u8; 4], W)> = Vec::new();

    let mut s = W::default();
    s.u32(m.session);
    s.u32(m.meta.len() as u32);
    for (k, v) in &m.meta {
        s.str(k);
        s.str(v);
    }
    sections.push((*b"META", s));

    let mut s = W::default();
    s.u32(m.nodes.len() as u32);
    for n in &m.nodes {
        s.u32(n.id);
        s.u8(n.kind.code());
        s.u8(n.origin.code());
        s.u8(n.flags);
        s.str(&n.path);
        s.str(&n.symbol);
        s.str(&n.branch);
        s.str(&n.commit);
        s.f64(n.y);
        s.f32(n.alpha);
        s.f32(n.beta);
        s.f32(n.activation);
        s.u32(n.recalls);
        s.u32(n.created_session);
        s.str(&n.key);
        s.str(&n.label);
        s.str(&n.text);
        s.u16(n.aliases.len() as u16);
        for a in &n.aliases {
            s.str(a);
        }
    }
    sections.push((*b"NODE", s));

    let mut s = W::default();
    s.u32(m.edges.len() as u32);
    for e in &m.edges {
        s.u32(e.from);
        s.u32(e.to);
        s.u8(e.kind.code());
        s.f32(e.alpha);
        s.f32(e.beta);
        s.f32(e.hebb);
        s.u32(e.created_session);
    }
    sections.push((*b"EDGE", s));

    let mut s = W::default();
    s.u32(m.hyperedges.len() as u32);
    for h in &m.hyperedges {
        s.u32(h.id);
        s.str(&h.label);
        s.f64(h.y);
        s.ids(&h.members);
    }
    sections.push((*b"HYPE", s));

    let mut out = W::default();
    out.0.extend_from_slice(MAGIC);
    let proposals = m.nodes.iter().any(|n| n.has(flags::PROPOSED));
    out.u16(if proposals { VERSION } else { VERSION_WITHOUT_PROPOSALS });
    out.u16(0);
    out.u32(sections.len() as u32);
    for (tag, body) in &sections {
        out.0.extend_from_slice(tag);
        out.u32(body.0.len() as u32);
        out.0.extend_from_slice(&body.0);
    }
    let crc = crc32(&out.0);
    out.u32(crc);
    out.0
}

pub fn decode(bytes: &[u8]) -> io::Result<Memory> {
    if bytes.len() < 16 {
        return Err(bad("archivo demasiado corto"));
    }
    let (body, tail) = bytes.split_at(bytes.len() - 4);
    let stored = u32::from_le_bytes(tail.try_into().unwrap());
    if crc32(body) != stored {
        return Err(bad("CRC32 no coincide (archivo corrupto)"));
    }
    let mut r = R { b: body, p: 0 };
    if r.take(4)? != MAGIC {
        return Err(bad("no es un archivo CTHG"));
    }
    let version = r.u16()?;
    if !(OLDEST_READABLE..=VERSION).contains(&version) {
        return Err(bad(&format!("versión {version} no soportada")));
    }
    r.u16()?;
    let count = r.u32()?;
    let mut m = Memory::default();
    for _ in 0..count {
        let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
        let len = r.u32()? as usize;
        let mut s = R { b: r.take(len)?, p: 0 };
        match &tag {
            b"META" => {
                m.session = s.u32()?;
                for _ in 0..s.u32()? {
                    let k = s.str()?;
                    let v = s.str()?;
                    m.meta.push((k, v));
                }
            }
            b"NODE" => {
                for _ in 0..s.u32()? {
                    let id = s.u32()?;
                    let kind = NodeKind::from_code(s.u8()?).ok_or_else(|| bad("tipo de nodo"))?;
                    let origin = Origin::from_code(s.u8()?).ok_or_else(|| bad("origen"))?;
                    let flags = s.u8()?;
                    let path = s.str()?;
                    let symbol = s.str()?;
                    let branch = s.str()?;
                    let commit = s.str()?;
                    let y = s.f64()?;
                    let alpha = s.f32()?;
                    let beta = s.f32()?;
                    let activation = s.f32()?;
                    let recalls = s.u32()?;
                    let created_session = s.u32()?;
                    let key = s.str()?;
                    let label = s.str()?;
                    let text = s.str()?;
                    let mut aliases = Vec::new();
                    for _ in 0..s.u16()? {
                        aliases.push(s.str()?);
                    }
                    if id as usize != m.nodes.len() {
                        return Err(bad("ids de nodo no consecutivos"));
                    }
                    m.nodes.push(Node {
                        id,
                        kind,
                        origin,
                        flags,
                        path,
                        symbol,
                        branch,
                        commit,
                        y,
                        alpha,
                        beta,
                        activation,
                        recalls,
                        created_session,
                        key,
                        label,
                        text,
                        aliases,
                    });
                }
            }
            b"EDGE" => {
                for _ in 0..s.u32()? {
                    m.edges.push(Edge {
                        from: s.u32()?,
                        to: s.u32()?,
                        kind: EdgeKind::from_code(s.u8()?).ok_or_else(|| bad("tipo de arista"))?,
                        alpha: s.f32()?,
                        beta: s.f32()?,
                        hebb: s.f32()?,
                        created_session: s.u32()?,
                    });
                }
            }
            b"HYPE" => {
                for _ in 0..s.u32()? {
                    m.hyperedges.push(Hyperedge {
                        id: s.u32()?,
                        label: s.str()?,
                        y: s.f64()?,
                        members: s.ids()?,
                    });
                }
            }
            _ => continue, // unknown section from a newer writer: skip
        }
        if !s.done() {
            return Err(bad(&format!("sección {} con bytes sobrantes", String::from_utf8_lossy(&tag))));
        }
    }
    let n = m.nodes.len() as u32;
    if m.edges.iter().any(|e| e.from >= n || e.to >= n)
        || m.hyperedges.iter().any(|h| h.members.iter().any(|&i| i >= n))
    {
        return Err(bad("arista apunta a un nodo inexistente"));
    }
    Ok(m)
}

pub fn save(m: &Memory, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // One temp file per writer: two processes saving at once must never share a half-written file.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("cthg.{}.{nanos}.tmp", std::process::id()));
    fs::write(&tmp, encode(m))?;
    if path.exists() {
        let _ = fs::copy(path, path.with_extension("cthg.bak"));
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Loads the memory; if it is unreadable or corrupt, falls back to the `.bak` of the previous save.
pub fn load(path: &Path) -> io::Result<Memory> {
    match fs::read(path).and_then(|b| decode(&b)) {
        Ok(m) => Ok(m),
        Err(e) => fs::read(path.with_extension("cthg.bak")).and_then(|b| decode(&b)).map_err(|_| e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::{learn, Claim};

    #[test]
    fn round_trip_and_corruption() {
        let mut m = Memory::default();
        m.set_meta("trunk", "main");
        learn(&mut m, &Claim::rule("No uses unwrap en el colisionador", "src/core/collider.rs").because("panic en producción"), 1000.0);
        m.hyperedges.push(Hyperedge { id: 0, label: "PR 12".into(), y: 1000.0, members: vec![0, 1] });
        let bytes = encode(&m);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.nodes.len(), m.nodes.len());
        assert_eq!(back.edges.len(), m.edges.len());
        assert_eq!(back.nodes[0].path, m.nodes[0].path);
        assert_eq!(back.meta("trunk"), Some("main"));
        assert_eq!(encode(&back), bytes, "re-encoding is byte-identical");

        let mut broken = bytes.clone();
        broken[20] ^= 0xFF;
        assert!(decode(&broken).is_err());
    }

    #[test]
    fn proposals_round_trip_as_version_3_and_the_rest_stays_version_2() {
        let mut m = Memory::default();
        learn(&mut m, &Claim::rule("Los mensajes del CLI van en español", "src"), 1.0);
        let plain = encode(&m);
        assert_eq!(u16::from_le_bytes([plain[4], plain[5]]), 2, "no proposal: a version 2 reader still opens it");

        let mut c = Claim::rule("siguen rotas (al pedir «arregla las ventanas»)", "src/ventanas.rs");
        c.proposed = true;
        let id = learn(&mut m, &c, 2.0)[0].node;
        let bytes = encode(&m);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 3);
        let back = decode(&bytes).unwrap();
        assert!(back.nodes[id as usize].has(flags::PROPOSED));
        assert!(!back.nodes[0].has(flags::PROPOSED));
        assert_eq!(encode(&back), bytes, "re-encoding is byte-identical");
    }

    #[test]
    fn version_1_files_still_load() {
        let mut m = Memory::default();
        learn(&mut m, &Claim::rule("Los tests usan una semilla fija", "tests"), 1.0);
        let mut v1 = encode(&m);
        v1[4..6].copy_from_slice(&1u16.to_le_bytes());
        let n = v1.len() - 4;
        let crc = crc32(&v1[..n]);
        v1[n..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&v1).unwrap().nodes.len(), m.nodes.len());
    }

    #[test]
    fn corrupt_file_falls_back_to_backup() {
        let dir = std::env::temp_dir().join(format!("cthg-bak-{}", std::process::id()));
        let path = dir.join("memoria.cthg");
        let mut m = Memory::default();
        learn(&mut m, &Claim::rule("Primera regla del proyecto", ""), 1.0);
        save(&m, &path).unwrap();
        learn(&mut m, &Claim::rule("Segunda regla distinta del todo", ""), 2.0);
        save(&m, &path).unwrap(); // .bak now holds the one-rule version
        let mut bytes = fs::read(&path).unwrap();
        bytes[20] ^= 0xFF;
        fs::write(&path, bytes).unwrap();
        assert_eq!(load(&path).unwrap().nodes.len(), 1, "previous save, not an error");
        let _ = fs::remove_dir_all(&dir);
    }
}
