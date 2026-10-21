# Casos de evaluación — este repo (3D-Memory)

`AGENTS.md` es una copia congelada del de la raíz: las líneas (`L27`) se refieren a ella.
Los casos son tareas reales de las fases 2–6. Las etiquetas las puso el mismo agente que
hizo el trabajo, así que es una **prueba inicial**, no una medición independiente.
Se evalúa con `cthg eval eval/3d-memory --repo .` (las rutas se comprueban en este repo).

core: L8, L16, L27, L31, L32, L66, L67

## caso: implementar la fase 2 (import)
prompt: implementa la fase 2: cthg import
lee: GUIA.md, AGENTS.md, crates/cthg-cli/src/main.rs, crates/cthg-core/src/learn.rs
relevantes: L3, L16!, L20, L21, L22, L24, L25, L26, L27!, L31!, L32, L46, L47, L48!, L49

## caso: símbolo del método editado
prompt: el símbolo del diff sale como impl Collider en vez del método; arréglalo
git: crates/cthg-core/src/spatial.rs
lee: crates/cthg-core/src/spatial.rs
relevantes: L16!, L22, L27!, L32, L33, L64, L66!

## caso: implementar los hooks
prompt: implementa cthg hook para Claude Code
lee: crates/cthg-cli/src/main.rs, hooks/README.md, hooks/settings.example.json
relevantes: L16!, L22, L26, L27!, L31!, L52!, L53!, L54, L55, L56!, L57

## caso: verify idempotente
prompt: verify suma beta cada vez que se ejecuta; que dos pasadas no cambien nada
git: crates/cthg-core/src/verify.rs
lee: crates/cthg-core/src/verify.rs, crates/cthg-core/src/score.rs
relevantes: L16!, L22, L27!, L38!, L60, L66!

## caso: flag STALE en el formato
prompt: añade un flag STALE que se guarde en el archivo
git: crates/cthg-core/src/model.rs, crates/cthg-core/src/binary.rs
lee: crates/cthg-core/src/binary.rs, docs/formato-cthg.md
relevantes: L16!, L27!, L35!, L41

## caso: cambiar lambda
prompt: sube lambda a 0,9 para que las carpetas pesen más
git: crates/cthg-core/src/spatial.rs
lee: crates/cthg-core/src/spatial.rs
relevantes: L16!, L27!, L34!

## caso: commit de la fase
prompt: haz el commit de la fase 6
relevantes: L8!, L27

## caso: comprobaciones antes de terminar
prompt: ¿qué comprobaciones hay que pasar antes de dar algo por terminado?
relevantes: L16!, L24, L25

## caso: embeddings para el léxico
prompt: añade embeddings para mejorar el recall léxico
lee: crates/cthg-core/src/retrieve.rs
relevantes: L31!, L64, L65!

## caso: test de verify que falla
prompt: un test de verify falla, ajústalo para que pase
git: crates/cthg-core/src/verify.rs
lee: crates/cthg-core/src/verify.rs
relevantes: L16!, L66!

## caso: serializar nuevo flag en Node
prompt: serializa el flag IMMUTABLE en el nodo de memoria
git: crates/cthg-core/src/model.rs
lee: crates/cthg-core/src/model.rs
relevantes: L16!, L27!, L35!

## caso: identificador encode en prompt
prompt: la función encode produce un buffer corrupto al guardar
lee: crates/cthg-core/src/lib.rs
relevantes: L16!, L27!, L35!

## caso: carga de binario en index
prompt: calcula el hash del archivo usando la función crc32 de binary
git: crates/cthg-core/src/index.rs
lee: crates/cthg-core/src/index.rs
relevantes: L16!, L27!, L35!

## caso: configuración de hooks en el CLI
prompt: el comando hook session-start no encuentra la plantilla de settings
git: crates/cthg-cli/src/main.rs
lee: crates/cthg-cli/src/main.rs
relevantes: L16!, L27!, L56!, L57

## caso: identificador OLDEST_READABLE
prompt: comprueba si la versión del archivo es menor que OLDEST_READABLE
lee: crates/cthg-core/src/lib.rs
relevantes: L16!, L27!, L35!

## caso: cambio en spatial sin tocar binary
prompt: refactoriza la función ancestors en spatial.rs
git: crates/cthg-core/src/spatial.rs
lee: crates/cthg-core/src/spatial.rs
relevantes: L16!, L22, L27!, L34!

## caso: cambio en verify sin tocar binary ni hooks
prompt: añade un método can_verify a RepoProbe
git: crates/cthg-core/src/verify.rs
lee: crates/cthg-core/src/verify.rs
relevantes: L16!, L22, L27!, L60
