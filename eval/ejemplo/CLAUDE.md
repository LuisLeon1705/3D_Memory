# Guía del proyecto

Servidor de juego multijugador con motor físico propio.

## Reglas generales

- Nunca hagas push directo a `main`; todo entra por PR.
- Los secretos van en el gestor de secretos y jamás en el repositorio.
- Comentarios e identificadores siempre en inglés.
- No añadas dependencias nuevas sin preguntar antes.
- Antes de dar algo por terminado, ejecuta `cargo test` y `cargo clippy`.

## Motor físico

- Todo lo que cuelga de `src/core/` debe ser determinista: nada de `rand` ni de relojes del sistema.
- En `src/core/collider.rs` no uses `unwrap()`: un panic tumbó el servidor en el torneo de marzo.
- `src/core/collider.rs::resolve_contacts` ordena los contactos por id para que la simulación sea reproducible.
- Las posiciones en `src/core/body.rs` usan f64; no las bajes a f32.
- El paso de simulación de `src/core/world.rs` es fijo (1/60 s); no lo hagas variable.

## Red

- Todos los mensajes de `src/net/protocol.rs` llevan número de versión; si cambias un mensaje, sube la versión.
- En `src/net/` nunca bloquees el hilo de red: usa los canales asíncronos.
- Los paquetes de `src/net/protocol.rs` no pueden superar 1200 bytes.
- Las conexiones de `src/net/server.rs` se cierran tras 30 segundos sin latido.

## Interfaz

- Los botones de `src/ui/` usan el tema oscuro.
- Los textos de `src/ui/` salen de `assets/i18n/es.ftl`, nunca escritos a mano.
- Los botones de `src/ui/` usan siempre el tema oscuro del juego.

## Pruebas

- Los tests de `tests/integration/` necesitan la base de datos local levantada.
- Las pruebas del motor en `tests/physics/` usan semilla fija.

## Despliegue

- El despliegue se hace con `scripts/deploy.sh` y solo desde la rama `main`.
- Antes de desplegar, avisa en el canal de operaciones.
- Las migraciones de base de datos viven en `migrations/` y no se editan una vez aplicadas.

## Pendiente

- [ ] Pendiente: migrar el renderizado de OpenGL a wgpu.
- La guía de estilo de `src/legacy/` ya no aplica.
