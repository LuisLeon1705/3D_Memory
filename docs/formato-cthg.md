# Formato `.cthg` (versión 2)

Little endian. Cadenas = `u32` longitud + bytes UTF-8. Listas de ids = `u32` cuenta + `u32`×n.

```text
cabecera   "CTHG" | u16 versión (=2; se lee también 1) | u16 reservado (=0) | u32 nº de secciones
sección*   [u8;4] etiqueta | u32 longitud | carga
cola       u32 CRC32 (IEEE) de todos los bytes anteriores
```

Las secciones desconocidas se saltan: un lector viejo abre un archivo nuevo.
Al guardar: con `.cthg/lock` tomado, cada escritor escribe su propio `memoria.cthg.<pid>.<ns>.tmp`, copia el archivo anterior a `memoria.cthg.bak` y renombra (atómico). Al leer, si el archivo falla (CRC, truncado), se carga `memoria.cthg.bak`.

Versión 2 = versión 1 + el bit 4 de `flags` (`STALE`). La disposición de bytes es la misma, por eso un archivo v1 se lee tal cual.

## META
`u32 sesión` · `u32 n` · n × (`str clave`, `str valor`)

Claves usadas: `trunk` (rama tronco), `created` (Unix).

## NODE
`u32 n` · n ×

| Campo | Tipo | Nota |
|---|---|---|
| id | u32 | consecutivo desde 0 (se valida) |
| kind | u8 | 0 regla · 1 decision · 2 hecho · 3 incidente · 4 convencion · 5 plan · 6 comando · 7 sesion |
| origin | u8 | 0 repo · 1 importado · 2 agente · 3 usuario · 4 sesion |
| flags | u8 | bit0 ANCHORED · bit1 CONSOLIDATED · bit2 CORE · bit3 POTENTIAL · bit4 STALE (v2) |
| path | str | X: ruta relativa con `/`; vacía = todo el proyecto |
| symbol | str | X fino: función o tipo; vacío = la ruta entera |
| branch | str | rama donde se aprendió; vacía = tronco |
| commit | str | hash corto |
| y | f64 | Y: segundos Unix |
| alpha, beta | f32, f32 | evidencia a favor / en contra |
| activation | f32 | [0, 1] |
| recalls | u32 | |
| created_session | u32 | |
| key, label, text | str ×3 | |
| aliases | u16 n + n × str | |

χ **no** se guarda: se calcula al cargar para la vista (rama, presente) que toque.

## EDGE
`u32 n` · n × (`u32 from`, `u32 to`, `u8 kind`, `f32 alpha`, `f32 beta`, `f32 hebb`, `u32 created_session`)

kind: 0 causa · 1 precede · 2 relacion · 3 contradice · 4 menciona · 5 sustituye

## HYPE
`u32 n` · n × (`u32 id`, `str label`, `f64 y`, ids miembros)

## Validación al leer
CRC, magia, versión, ids de nodo consecutivos, ninguna arista ni miembro apuntando fuera, ninguna sección con bytes sobrantes.
