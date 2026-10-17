# 3D-Memory (`cthg`)

Memoria persistente de proyecto para agentes de código (**Claude Code** y **Google Antigravity**), impulsada por un **Causal-Temporal HyperGraph (CTHG)** almacenado en un único archivo binario (`.cthg/memoria.cthg`) sin dependencias externas.

En lugar de inyectar un `CLAUDE.md` o `AGENTS.md` completo en cada turno, **3D-Memory** inyecta **un núcleo pequeño fijo (`CORE`) + únicamente los recuerdos que aplican a los archivos, símbolos y tarea actuales**, ponderados por ruta, tiempo y confianza.

---

## Modelo de tres ejes

Cada recuerdo es un nodo posicionado en tres dimensiones:

1. **Eje X · Espacio (árbol del repositorio)**  
   Ruta normalizada (`src/core/collider.rs`) y símbolo opcional (`resolve_contacts`). Las reglas heredan hacia abajo en el árbol con decaimiento $\lambda^d$ ($\lambda = 0{,}8$): una regla en `src/core/` aplica a todo su subárbol, y lo más específico pesa más. La raíz (`""`) tiene afinidad espacial $0$, por lo que las reglas transversales se marcan como `CORE`.
2. **Eje Y · Tiempo monótono y ramas**  
   Tiempo Unix estrictamente creciente ($y_{\text{nuevo}} = \max(t, y_{\max} + \varepsilon)$). Nada se sobreescribe ni se borra: las reglas actualizadas crean una arista `sustituye` y hunden a la versión anterior. Los planes (`POTENTIAL`) permanecen invisibles hasta que un aprendizaje posterior los cumple.
3. **Eje Z · Confianza ($\chi \in [-1, 1]$)**  
   Combina evidencia estadística $\text{Beta}(\alpha, \beta)$ según el origen (`repo`, `usuario`, `importado`, `sesion`, `agente`) con una lectura estructural acíclica $E$ a través de aristas causales (`causa`, `contradice`, `sustituye`) y verificación directa contra el repositorio (`ANCHORED` / `STALE`).

---

## Canales de recuperación

| Canal | Cuándo se activa | Entrada | Presupuesto |
| :--- | :--- | :--- | :--- |
| **0 · Núcleo (`CORE`)** | Inicio de sesión | `CORE` global (`~/.cthg/global.cthg`) + `CORE` del proyecto | $\le 500$ tokens |
| **1 · Espacial (prompt)** | En cada mensaje | Archivos modificados (`git status`) y símbolos de hunks (`git diff -M`) | $\le 800$ tokens (compartido con canal 3) |
| **2 · Espacial (justo a tiempo)** | Al leer un archivo (`Read` / `view_file`) | Ruta del archivo leído y sus carpetas antecesoras | $\le 300$ tokens por archivo |
| **3 · Léxico** | En cada mensaje | Tokens y frases del prompt (con *stemming* ES/EN y contexto de sección) | $\le 800$ tokens (compartido con canal 1) |

---

## Instalación

Requiere Rust estable. Sin dependencias externas en `cthg-core` ni en `cthg-cli`:

```bash
# Instalar el binario `cthg` en ~/.cargo/bin (necesario para los hooks)
cargo install --path crates/cthg-cli --locked

# Ejecutar tests y linter
cargo test
cargo clippy --all-targets
```

---

## Comandos del CLI (`cthg`)

### Inicialización y escritura
```bash
# Crear .cthg/memoria.cthg en la raíz del repositorio y detectar la rama tronco
cthg init

# Guardar un recuerdo nuevo, reforzar uno existente o cumplir un plan
cthg learn "<texto>" \
  [--kind regla|decision|hecho|incidente|convencion|plan|comando] \
  [--path <ruta>] [--symbol <símbolo>] \
  [--cause "<motivo>"] [--cause-id <id>] \
  [--contradicts <id>] [--supersedes <id>] \
  [--origin usuario|agente|repo|importado] \
  [--core] [--plan] [--deny]

# Confirmar (α + 1) o negar (β + 1) un recuerdo existente por su ID
cthg confirm <id>
cthg deny <id>

# Importar o reimportar un CLAUDE.md / AGENTS.md (propone CORE dentro de 500 tokens)
cthg import <archivo.md> [--yes]
```

### Consulta y mantenimiento
```bash
# Mostrar el núcleo (canal 0) o marcar/desmarcar nodos como CORE
cthg core [<id>... [--unset]] [--budget <N>]

# Recuperar recuerdos relevantes (canales 1–3)
cthg recall [--prompt "<texto>"] [--path <ruta>]... [--git] [--budget <N>] [--limit <N>]

# Listar todos los recuerdos visibles (o todos con --all) con su χ, banda y flags
cthg list [--all]

# Actualizar las rutas de la memoria tras mover o renombrar un archivo o carpeta
cthg mv <de> <a>

# Comprobar rutas, símbolos, dependencias y comandos contra el estado actual del repo
cthg verify [--run-commands]
```

### Memoria global (`--global`)
Las reglas que aplican a **todos** los proyectos del usuario se guardan en `~/.cthg/global.cthg` (o en la ruta definida por `CTHG_GLOBAL`):

```bash
cthg <init|learn|confirm|deny|core|list|import> --global
```

### Hooks y evaluación
```bash
# Hooks para Claude Code (leen JSON por stdin y deduplican por sesión)
cthg hook <session-start|prompt|read|edit>

# Hook PreInvocation para Google Antigravity (lee el transcript incrementalmente)
cthg hook antigravity

# Evaluar ahorro de tokens, recall, precisión y críticas perdidas en 6 configuraciones
cthg eval <carpeta> [--markdown] [--repo <DIR>] [--file <CLAUDE.md>] [--core-auto]
```

---

## Estructura del proyecto

```text
3D-Memory/
├── crates/
│   ├── cthg-core/           # Motor CTHG puro en Rust (0 dependencias)
│   │   └── src/
│   │       ├── model.rs     # Nodos, aristas, hiperaristas, flags y priors Beta(α, β)
│   │       ├── score.rs     # Vista activa, visibilidad y cálculo de confianza χ (E + A)
│   │       ├── spatial.rs   # Eje X: normalización, afinidad λ^d y renombres
│   │       ├── retrieve.rs  # Canales 0–3, semillas, propagación a 1 salto y presupuesto
│   │       ├── learn.rs     # Plasticidad, planes, consolidación y olvido por sesión
│   │       ├── import.rs    # Parser Markdown, deducción de rutas, propuesta CORE y reimportación
│   │       ├── verify.rs    # Comprobación de rutas, símbolos, manifiestos y comandos
│   │       ├── hook.rs      # Contrato de hooks para Claude Code
│   │       ├── antigravity.rs # Contrato PreInvocation y lector de transcript para Antigravity
│   │       ├── eval.rs      # Benchmark reproducible en RAM sobre 6 configuraciones
│   │       ├── text.rs      # Tokenización ES/EN, similitud y estimación de tokens
│   │       ├── json.rs      # Parser JSON mínimo sin dependencias
│   │       └── binary.rs    # Formato binario .cthg v2 con CRC32 y escritura atómica
│   └── cthg-cli/            # Binario `cthg`
├── docs/formato-cthg.md     # Especificación byte a byte del archivo .cthg
├── hooks/                   # Plantillas y guía para Claude Code y Google Antigravity
├── skill/SKILL.md           # Instrucciones para que el agente escriba en la memoria
├── eval/                    # Conjuntos de evaluación sintéticos y reales
└── GUIA.md                  # Diseño detallado, formulación matemática y estado de fases
```

---

## Documentación adicional

- **Diseño, matemáticas y estado de fases**: [GUIA.md](GUIA.md)
- **Especificación del formato binario `.cthg`**: [docs/formato-cthg.md](docs/formato-cthg.md)
- **Configuración de hooks (Claude Code y Antigravity)**: [hooks/README.md](hooks/README.md)
- **Skill de escritura para agentes**: [skill/SKILL.md](skill/SKILL.md)
- **Visualizador de escritorio 3D (`memviz`)**: [`../3D-Memory-Visualizer`](../3D-Memory-Visualizer/README.md)
