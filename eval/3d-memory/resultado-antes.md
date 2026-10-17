# Resultado de `cthg eval` — eval/3d-memory

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 16 % | 0 |
| anidado ideal | 954 | 25 % | 94 % | 17 % | 1 |
| solo léxico | 422 | 67 % | 73 % | 33 % | 2 |
| espacial | 246 | 81 % | 54 % | 34 % | 8 |
| espacial + lectura | 274 | 78 % | 59 % | 36 % | 7 |
| todo | 432 | 66 % | 73 % | 33 % | 2 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 604 | 47 % | 1 |
| símbolo del método editado | 246 | 57 % | 0 |
| implementar los hooks | 546 | 73 % | 0 |
| verify idempotente | 354 | 83 % | 0 |
| flag STALE en el formato | 553 | 75 % | 0 |
| cambiar lambda | 378 | 67 % | 1 |
| commit de la fase | 415 | 100 % | 0 |
| comprobaciones antes de terminar | 354 | 67 % | 0 |
| embeddings para el léxico | 413 | 67 % | 0 |
| test de verify que falla | 461 | 100 % | 0 |

## Fallos

### implementar la fase 2 (import)
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin coincidencia léxica), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial`: L3 (ruta no coincide), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (ruta no coincide), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial + lectura`: L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)

### símbolo del método editado
- `anidado ideal`: L32 (ruta no coincide)
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)

### implementar los hooks
- `anidado ideal`: L26 (ruta no coincide)
- `solo léxico`: L22 (sin ruta y no CORE), L26 (sin coincidencia léxica), L55 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L26 (ruta no coincide), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (sin ruta y no CORE), L57 (ruta no coincide)
- `espacial + lectura`: L22 (sin ruta y no CORE), L26 (ruta no coincide), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L26 (ruta no coincide), L55 (sin ruta y no CORE)

### verify idempotente
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)

### flag STALE en el formato
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L35! (ruta no coincide), L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)

### cambiar lambda
- `anidado ideal`: L34! (ruta no coincide)
- `solo léxico`: L34! (sin coincidencia léxica)
- `espacial`: L34! (ruta no coincide)
- `espacial + lectura`: L34! (ruta no coincide)
- `todo`: L34! (ruta no coincide)

### comprobaciones antes de terminar
- `solo léxico`: L25 (sin ruta y no CORE)
- `espacial`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `espacial + lectura`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `todo`: L25 (sin ruta y no CORE)

### embeddings para el léxico
- `solo léxico`: L64 (sin ruta y no CORE)
- `espacial`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `espacial + lectura`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `todo`: L64 (sin ruta y no CORE)
