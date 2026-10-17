# Casos de evaluación — ejemplo sintético

Las líneas (`L12`) son del `CLAUDE.md` de esta carpeta. `!` marca una regla crítica.
Las reglas generales se cuentan como relevantes solo donde aplican a la tarea.

core: L7, L8, L9, L10, L11

## caso: arreglar el colisionador
prompt: arréglalo
git: src/core/collider.rs
lee: src/core/collider.rs
relevantes: L7!, L9, L11, L15, L16!, L17

## caso: mensaje nuevo en el protocolo
prompt: añade un mensaje de chat al protocolo
lee: src/net/protocol.rs
relevantes: L7!, L9, L11, L23!, L24, L25

## caso: color de los botones
prompt: cambia el color de los botones del menú
lee: src/ui/menu.rs
relevantes: L7!, L9, L11, L30

## caso: desplegar
prompt: ¿cómo despliego una versión nueva?
relevantes: L8!, L41!, L42

## caso: tests de integración
prompt: los tests de integración fallan en mi máquina
lee: tests/integration/db.rs
relevantes: L11, L36

## caso: memoria de los cuerpos
prompt: reduce la memoria que ocupan los cuerpos
git: src/core/body.rs
lee: src/core/body.rs
relevantes: L7!, L9, L11, L15, L18!

## caso: columna nueva en jugadores
prompt: añade una columna de nivel a la tabla de jugadores
lee: migrations/0001_init.sql
relevantes: L7!, L11, L43!

## caso: dependencia nueva
prompt: añade serde_json para leer la configuración
git: Cargo.toml
lee: Cargo.toml
relevantes: L7!, L10!, L11

## caso: paso de simulación variable
prompt: haz que la simulación avance según los fps
git: src/core/world.rs
lee: src/core/world.rs
relevantes: L7!, L9, L11, L15, L19!

## caso: tiempo de espera de conexiones
prompt: sube el tiempo de espera de las conexiones
lee: src/net/server.rs
relevantes: L7!, L9, L11, L24, L26

## caso: textos del menú de opciones
prompt: revisa los textos del menú de opciones
lee: src/ui/menu.rs
relevantes: # REVISAR
