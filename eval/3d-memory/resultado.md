# Resultado de `cthg eval` — eval/3d-memory

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 17 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1282 | 0 % | 100 % | 13 % | 0 |
| anidado ideal | 1067 | 17 % | 89 % | 13 % | 6 |
| solo léxico | 492 | 62 % | 79 % | 28 % | 4 |
| espacial | 249 | 81 % | 58 % | 32 % | 13 |
| espacial + lectura | 251 | 80 % | 59 % | 33 % | 13 |
| todo | 472 | 63 % | 79 % | 29 % | 4 |
| todo + vecinos | 460 | 64 % | 83 % | 32 % | 2 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘
Criterio (`todo + vecinos`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 828 | 87 % | 0 |
| símbolo del método editado | 244 | 57 % | 0 |
| implementar los hooks | 636 | 82 % | 0 |
| verify idempotente | 352 | 83 % | 0 |
| flag STALE en el formato | 323 | 75 % | 0 |
| cambiar lambda | 378 | 67 % | 1 |
| commit de la fase | 828 | 100 % | 0 |
| comprobaciones antes de terminar | 362 | 67 % | 0 |
| embeddings para el léxico | 413 | 67 % | 0 |
| test de verify que falla | 536 | 100 % | 0 |
| serializar nuevo flag en Node | 294 | 67 % | 1 |
| identificador encode en prompt | 299 | 67 % | 1 |
| carga de binario en index | 613 | 100 % | 0 |
| configuración de hooks en el CLI | 505 | 100 % | 0 |
| identificador OLDEST_READABLE | 663 | 100 % | 0 |
| cambio en spatial sin tocar binary | 299 | 50 % | 1 |
| cambio en verify sin tocar binary ni hooks | 453 | 75 % | 0 |

## Por caso (`todo + vecinos`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 925 | 87 % | 0 |
| símbolo del método editado | 244 | 57 % | 0 |
| implementar los hooks | 732 | 82 % | 0 |
| verify idempotente | 352 | 83 % | 0 |
| flag STALE en el formato | 323 | 75 % | 0 |
| cambiar lambda | 378 | 67 % | 1 |
| commit de la fase | 828 | 100 % | 0 |
| comprobaciones antes de terminar | 362 | 67 % | 0 |
| embeddings para el léxico | 413 | 67 % | 0 |
| test de verify que falla | 536 | 100 % | 0 |
| serializar nuevo flag en Node | 376 | 100 % | 0 |
| identificador encode en prompt | 323 | 100 % | 0 |
| carga de binario en index | 370 | 100 % | 0 |
| configuración de hooks en el CLI | 586 | 100 % | 0 |
| identificador OLDEST_READABLE | 323 | 100 % | 0 |
| cambio en spatial sin tocar binary | 299 | 50 % | 1 |
| cambio en verify sin tocar binary ni hooks | 453 | 75 % | 0 |

## Fallos

### implementar la fase 2 (import)
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)
- `todo + vecinos`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)

### símbolo del método editado
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)

### implementar los hooks
- `anidado ideal`: L56! (ruta no coincide)
- `solo léxico`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (ruta no coincide)
- `todo`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)

### verify idempotente
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE)

### flag STALE en el formato
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)
- `todo + vecinos`: L41 (sin ruta y no CORE)

### cambiar lambda
- `solo léxico`: L34! (sin ruta y no CORE)
- `espacial`: L34! (sin ruta y no CORE)
- `espacial + lectura`: L34! (sin ruta y no CORE)
- `todo`: L34! (sin ruta y no CORE)
- `todo + vecinos`: L34! (sin ruta y no CORE)

### comprobaciones antes de terminar
- `solo léxico`: L25 (sin ruta y no CORE)
- `espacial`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `espacial + lectura`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `todo`: L25 (sin ruta y no CORE)
- `todo + vecinos`: L25 (sin ruta y no CORE)

### embeddings para el léxico
- `solo léxico`: L64 (sin ruta y no CORE)
- `espacial`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `espacial + lectura`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `todo`: L64 (sin ruta y no CORE)
- `todo + vecinos`: L64 (sin ruta y no CORE)

### serializar nuevo flag en Node
- `anidado ideal`: L35! (ruta no coincide)
- `solo léxico`: L35! (sin coincidencia léxica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### identificador encode en prompt
- `anidado ideal`: L35! (ruta no coincide)
- `solo léxico`: L35! (sin coincidencia léxica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### carga de binario en index
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### configuración de hooks en el CLI
- `anidado ideal`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L56! (ruta no coincide), L57 (ruta no coincide)

### identificador OLDEST_READABLE
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### cambio en spatial sin tocar binary
- `solo léxico`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)

### cambio en verify sin tocar binary ni hooks
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE)
