# AGENTS.md — cómo trabajar en 3D-Memory

Lee primero **[GUIA.md](GUIA.md) entera**. Ahí están el diseño, las matemáticas, el estado de cada fase y lo que falta. Este archivo explica *cómo* trabajar; la guía explica *qué* construir.

## Entorno

- Windows 11, Rust estable (cargo 1.96). La carpeta está en OneDrive.
- Este directorio es **su propio repositorio git**, pero está dentro de otro repo más grande (la carpeta de usuario). Ejecuta git siempre desde aquí o con `git -C <esta carpeta>`, y nunca hagas commit en el repo padre.
- Comprobación obligatoria antes de dar algo por hecho:

```bash
cargo test
cargo clippy --all-targets
```

Las dos tienen que terminar limpias: tests en verde y clippy sin avisos.

## Cómo trabajar

1. **Una fase por encargo**, en el orden de la sección 13 de la guía. Las siguientes son la **2** (`import`) y la **4** (`hook`). No empieces otra fase sin que te la pidan.
2. Antes de escribir código, resume en pocas líneas qué vas a hacer y qué archivos vas a tocar.
3. Cada pieza nueva lleva **tests** en el mismo módulo (`#[cfg(test)] mod tests`), con el estilo de los que ya hay: casos concretos con nombres que dicen qué demuestran.
4. Al terminar una fase:
   - pasa las dos comprobaciones;
   - ejecuta el comando nuevo en un repo git de prueba **fuera de esta carpeta** (por ejemplo en `%TEMP%`) y enseña la salida real;
   - actualiza la tabla de fases (sección 13) y la del CLI (sección 9) de `GUIA.md`.
5. **No hagas commits ni push.** El usuario revisa el diff y decide.

## Reglas del código

- **`cthg-core` no tiene dependencias** y debe seguir así. El CLI tampoco las tiene; si de verdad hace falta una (por ejemplo `serde_json` para leer el JSON de los hooks), pregunta antes y justifica por qué.
- Código, identificadores y comentarios **en inglés**. `GUIA.md`, la documentación y los mensajes del CLI **en español**.
- Imita el estilo existente: comentarios `//!` al principio de cada módulo explicando el modelo, funciones pequeñas, sin `unwrap()` sobre datos externos (archivos, git, stdin).
- **Las matemáticas están en la guía.** Si cambias una fórmula o una constante (λ, σ, K, W, ω, los priors, los umbrales de similitud, el decaimiento, los presupuestos), cambia también la sección correspondiente de `GUIA.md` en el mismo trabajo, ejemplos con números incluidos. Guía y código no pueden contradecirse.
- **Formato binario**: cualquier cambio en lo que se guarda sube `VERSION` en `binary.rs`, actualiza [docs/formato-cthg.md](docs/formato-cthg.md) y añade un test de ida y vuelta. Las secciones nuevas van con etiqueta propia para que los lectores viejos puedan saltarlas.
- **Invariantes que no se rompen** (hay tests que los vigilan, no los relajes para que pasen):
  - Y solo avanza: nada escrito se mueve hacia atrás ni se borra.
  - Lo anclado (`ANCHORED`) no lo mueve ni `learn` ni `confirm` ni `deny`; solo `verify`.
  - Un plan (`POTENTIAL`) es invisible hasta que se cumple.
  - La raíz (`""`) tiene afinidad 0: las reglas transversales van por `CORE`, no por ruta.
  - χ no se guarda, se calcula para la vista.

## Notas por fase

### Fase 2 · `cthg import <archivo.md>`
- Parte el markdown en nodos: una viñeta o párrafo por nodo, con origen `importado`.
- Deduce `path` cuando la línea nombra una ruta que existe en el repo. Lo demás queda sin ruta.
- **Propón** qué reglas son `CORE` (las que no tienen ruta y son prohibiciones u obligaciones generales), pero no las marques sin confirmación: muestra la lista y deja que el usuario la acepte (por ejemplo con `--yes` o un segundo comando).
- Importar dos veces el mismo archivo no debe duplicar nodos.

### Fase 4 · `cthg hook <session-start|prompt|read|edit>`
- **Antes de implementar, comprueba en la documentación oficial de hooks de Claude Code** el formato exacto del JSON de entrada de cada evento y cómo `PostToolUse` añade contexto (`hookSpecificOutput.additionalContext` o lo que diga la documentación actual). No lo supongas.
- Un hook **nunca bloquea ni falla en voz alta**: ante cualquier error, código de salida 0 y sin salida.
- Deduplica por sesión en `.cthg/sesion/<session_id>.json`.
- Respeta los presupuestos de la guía (núcleo 500, prompt 800, por archivo 300 tokens).
- Instala los hooks solo en el `.claude/settings.json` **de un proyecto de prueba**. Nunca toques la configuración global de Claude Code (`~/.claude/settings.json`).
- Plantilla: [hooks/settings.example.json](hooks/settings.example.json); reglas: [hooks/README.md](hooks/README.md).

### Fase 3, 5 y 7
Sigue lo que dice la guía. En `verify` (fase 5), quitar un ancla es la **única** forma de que un hecho del repo pierda peso: hazlo explícito en la salida.

## Qué no hacer

- No reescribas módulos enteros que funcionan para «limpiarlos». Cambia lo que la fase necesita.
- No añadas embeddings, servicios de red ni modelos: la recuperación es local y determinista.
- No borres ni cambies tests existentes para que algo pase. Si un test está mal, dilo y explica por qué.
- No toques nada fuera de esta carpeta, salvo el repo de prueba temporal.
