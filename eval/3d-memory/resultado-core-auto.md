# Resultado de `cthg eval` — eval/3d-memory

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE propuesto por import: 12 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 16 % | 0 |
| anidado ideal | 1114 | 12 % | 100 % | 16 % | 0 |
| solo léxico | 696 | 45 % | 87 % | 25 % | 0 |
| espacial | 438 | 65 % | 70 % | 26 % | 5 |
| espacial + lectura | 442 | 65 % | 71 % | 26 % | 5 |
| todo | 696 | 45 % | 87 % | 25 % | 0 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✔ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 1010 | 93 % | 0 |
| símbolo del método editado | 430 | 71 % | 0 |
| implementar los hooks | 845 | 82 % | 0 |
| verify idempotente | 537 | 83 % | 0 |
| flag STALE en el formato | 823 | 75 % | 0 |
| cambiar lambda | 563 | 100 % | 0 |
| commit de la fase | 938 | 100 % | 0 |
| comprobaciones antes de terminar | 521 | 67 % | 0 |
| embeddings para el léxico | 573 | 100 % | 0 |
| test de verify que falla | 721 | 100 % | 0 |

## Fallos

### implementar la fase 2 (import)
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)

### símbolo del método editado
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)

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

### comprobaciones antes de terminar
- `solo léxico`: L25 (sin ruta y no CORE)
- `espacial`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `espacial + lectura`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `todo`: L25 (sin ruta y no CORE)
