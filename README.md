# 3D-Memory

Memoria de proyecto para agentes de código: un núcleo pequeño que se inyecta siempre y, en vez de un `CLAUDE.md` entero, solo lo que aplica a lo que se está tocando. Cada recuerdo tiene ruta, tiempo y confianza.

- Diseño, matemáticas y fases: [GUIA.md](GUIA.md)
- Formato binario: [docs/formato-cthg.md](docs/formato-cthg.md)
- Hooks de Claude Code: [hooks/](hooks/README.md)
- Skill de aprendizaje: [skill/SKILL.md](skill/SKILL.md)

```bash
cargo test
cargo build --release
```
