# Resultado de `cthg eval` — eval/3d-memory

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 16 % | 0 |
| anidado ideal | 1114 | 12 % | 100 % | 16 % | 0 |
| solo léxico | 536 | 58 % | 78 % | 30 % | 1 |
| espacial | 252 | 80 % | 57 % | 35 % | 7 |
| espacial + lectura | 256 | 80 % | 58 % | 36 % | 7 |
| todo | 536 | 58 % | 78 % | 30 % | 1 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 894 | 87 % | 0 |
| símbolo del método editado | 244 | 57 % | 0 |
| implementar los hooks | 701 | 82 % | 0 |
| verify idempotente | 352 | 83 % | 0 |
| flag STALE en el formato | 663 | 75 % | 0 |
| cambiar lambda | 378 | 67 % | 1 |
| commit de la fase | 822 | 100 % | 0 |
| comprobaciones antes de terminar | 362 | 67 % | 0 |
| embeddings para el léxico | 413 | 67 % | 0 |
| test de verify que falla | 536 | 100 % | 0 |

## Fallos

### implementar la fase 2 (import)
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)

### símbolo del método editado
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)

### implementar los hooks
- `solo léxico`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (sin ruta y no CORE), L57 (ruta no coincide)
- `espacial + lectura`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)

### verify idempotente
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)

### flag STALE en el formato
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)

### cambiar lambda
- `solo léxico`: L34! (sin ruta y no CORE)
- `espacial`: L34! (sin ruta y no CORE)
- `espacial + lectura`: L34! (sin ruta y no CORE)
- `todo`: L34! (sin ruta y no CORE)

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
