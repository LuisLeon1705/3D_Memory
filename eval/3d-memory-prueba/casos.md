# Casos de prueba — este repo (3D-Memory), conjunto apartado

Escritos **antes** de las mejoras de import A–D y no se usan para ajustar nada: sirven para
comprobar si lo aprendido con `eval/3d-memory` (conjunto de ajuste) generaliza.
Mismo `AGENTS.md` congelado y mismo CORE. Etiquetas puestas por el agente: prueba inicial.
`cthg eval eval/3d-memory-prueba --repo .`

core: L8, L16, L27, L31, L32, L66, L67

## caso: candado de escritura
prompt: añade un candado para que dos procesos no guarden la memoria a la vez
git: crates/cthg-cli/src/main.rs
lee: crates/cthg-cli/src/main.rs, crates/cthg-core/src/binary.rs
relevantes: L16!, L27!, L31!, L33

## caso: rutas de Windows en el hook
prompt: el hook de lectura falla con rutas de Windows
git: crates/cthg-core/src/hook.rs
lee: crates/cthg-core/src/hook.rs
relevantes: L16!, L22, L27!, L33, L53!

## caso: implementar la fase 6 (eval)
prompt: implementa la fase 6: cthg eval
lee: GUIA.md, AGENTS.md, crates/cthg-core/src/import.rs, crates/cthg-cli/src/main.rs
relevantes: L3, L16!, L20, L21, L22, L24, L25, L26, L27!, L31!, L32, L64, L65

## caso: verify ejecuta comandos
prompt: verify ejecuta los comandos de la memoria sin preguntar
git: crates/cthg-core/src/verify.rs
lee: crates/cthg-core/src/verify.rs
relevantes: L16!, L27!, L60

## caso: versión 3 del formato
prompt: sube la versión del formato binario a 3 para añadir una sección nueva
git: crates/cthg-core/src/binary.rs
lee: crates/cthg-core/src/binary.rs
relevantes: L16!, L27!, L35!, L41

## caso: serde_json en el CLI
prompt: añade serde_json al CLI para leer el JSON de los hooks
git: crates/cthg-cli/Cargo.toml
lee: crates/cthg-cli/Cargo.toml
relevantes: L16!, L27!, L31!

## caso: umbral de similitud
prompt: ¿por qué el umbral de similitud de learn es 0,5?
relevantes: L3, L34

## caso: borrar un test molesto
prompt: borra el test de consolidación, molesta
git: crates/cthg-core/src/learn.rs
lee: crates/cthg-core/src/learn.rs
relevantes: L16!, L66!

## caso: instalar los hooks aquí
prompt: instala los hooks en este repo
lee: hooks/settings.example.json
relevantes: L56!, L57, L67!

## caso: limpiar retrieve
prompt: reescribe retrieve.rs para que quede más limpio
lee: crates/cthg-core/src/retrieve.rs
relevantes: L16!, L27!, L34, L64!

## caso: campo nuevo en Memory
prompt: añade un campo revision a la estructura Memory
git: crates/cthg-core/src/model.rs
lee: crates/cthg-core/src/model.rs
relevantes: L16!, L27!, L35!

## caso: identificador decode en el mensaje
prompt: la función decode falla con datos truncados
lee: crates/cthg-core/src/lib.rs
relevantes: L16!, L27!, L35!

## caso: guardar desde el CLI
prompt: añade el comando dump para serializar la memoria a stdout
git: crates/cthg-cli/src/main.rs
lee: crates/cthg-cli/src/main.rs
relevantes: L16!, L27!, L31!, L35!

## caso: identificador crc32 en index
prompt: optimiza crc32 para acelerar el escaneo incremental
git: crates/cthg-core/src/index.rs
lee: crates/cthg-core/src/index.rs
relevantes: L16!, L27!, L35!

## caso: plantilla de hooks desde README
prompt: actualiza la plantilla de configuración de hooks para Claude Code
git: hooks/README.md
lee: hooks/README.md
relevantes: L16!, L27!, L56!, L57

## caso: cambio en texto sin vecinos relevantes
prompt: añade la función levenshtein en text.rs
git: crates/cthg-core/src/text.rs
lee: crates/cthg-core/src/text.rs
relevantes: L16!, L22, L27!, L32

## caso: cambio en learn sin relación con binary ni hooks
prompt: ajusta el decaimiento de activación en learn
git: crates/cthg-core/src/learn.rs
lee: crates/cthg-core/src/learn.rs
relevantes: L16!, L22, L27!, L34!
