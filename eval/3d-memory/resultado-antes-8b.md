# Resultado de `cthg eval` ÔÇö eval/3d-memory

AGENTS.md: 36 bloques importados (0 fusionados con otro) ┬À CORE confirmado en casos.md: 7 reglas ┬À 17 casos medidos

| Configuraci├│n | Tokens medios | Ahorro vs completo | Recall medio | Precisi├│n media | Cr├¡ticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1282 | 0 % | 100 % | 13 % | 0 |
| anidado ideal | 1067 | 17 % | 89 % | 13 % | 6 |
| solo l├®xico | 492 | 62 % | 79 % | 28 % | 4 |
| espacial | 249 | 81 % | 58 % | 32 % | 13 |
| espacial + lectura | 251 | 80 % | 59 % | 33 % | 13 |
| todo | 472 | 63 % | 79 % | 29 % | 4 |

Criterio (`todo`): ahorro ÔëÑ 70 % Ô£ÿ ┬À cr├¡ticas perdidas = 0 Ô£ÿ ┬À recall ÔëÑ 95 % Ô£ÿ

## Por caso (`todo`)

| Caso | Tokens | Recall | Cr├¡ticas perdidas |
|---|---:|---:|---:|
| implementar la fase 2 (import) | 828 | 87 % | 0 |
| s├¡mbolo del m├®todo editado | 244 | 57 % | 0 |
| implementar los hooks | 636 | 82 % | 0 |
| verify idempotente | 352 | 83 % | 0 |
| flag STALE en el formato | 323 | 75 % | 0 |
| cambiar lambda | 378 | 67 % | 1 |
| commit de la fase | 828 | 100 % | 0 |
| comprobaciones antes de terminar | 362 | 67 % | 0 |
| embeddings para el l├®xico | 413 | 67 % | 0 |
| test de verify que falla | 536 | 100 % | 0 |
| serializar nuevo flag en Node | 294 | 67 % | 1 |
| identificador encode en prompt | 299 | 67 % | 1 |
| carga de binario en index | 613 | 100 % | 0 |
| configuraci├│n de hooks en el CLI | 505 | 100 % | 0 |
| identificador OLDEST_READABLE | 663 | 100 % | 0 |
| cambio en spatial sin tocar binary | 299 | 50 % | 1 |
| cambio en verify sin tocar binary ni hooks | 453 | 75 % | 0 |

## Fallos

### implementar la fase 2 (import)
- `solo l├®xico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L46 (sin ruta y no CORE), L47 (sin ruta y no CORE), L48! (sin ruta y no CORE), L49 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE)

### s├¡mbolo del m├®todo editado
- `solo l├®xico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L64 (sin ruta y no CORE)

### implementar los hooks
- `anidado ideal`: L56! (ruta no coincide)
- `solo l├®xico`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE), L52! (sin ruta y no CORE), L53! (sin ruta y no CORE), L54 (sin ruta y no CORE), L55 (sin ruta y no CORE), L56! (ruta no coincide)
- `todo`: L22 (sin ruta y no CORE), L26 (sin ruta y no CORE)

### verify idempotente
- `solo l├®xico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L38! (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)

### flag STALE en el formato
- `solo l├®xico`: L41 (sin ruta y no CORE)
- `espacial`: L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)

### cambiar lambda
- `solo l├®xico`: L34! (sin ruta y no CORE)
- `espacial`: L34! (sin ruta y no CORE)
- `espacial + lectura`: L34! (sin ruta y no CORE)
- `todo`: L34! (sin ruta y no CORE)

### comprobaciones antes de terminar
- `solo l├®xico`: L25 (sin ruta y no CORE)
- `espacial`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `espacial + lectura`: L24 (sin ruta y no CORE), L25 (sin ruta y no CORE)
- `todo`: L25 (sin ruta y no CORE)

### embeddings para el l├®xico
- `solo l├®xico`: L64 (sin ruta y no CORE)
- `espacial`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `espacial + lectura`: L64 (sin ruta y no CORE), L65! (sin ruta y no CORE)
- `todo`: L64 (sin ruta y no CORE)

### serializar nuevo flag en Node
- `anidado ideal`: L35! (ruta no coincide)
- `solo l├®xico`: L35! (sin coincidencia l├®xica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### identificador encode en prompt
- `anidado ideal`: L35! (ruta no coincide)
- `solo l├®xico`: L35! (sin coincidencia l├®xica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### carga de binario en index
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### configuraci├│n de hooks en el CLI
- `anidado ideal`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L56! (ruta no coincide), L57 (ruta no coincide)

### identificador OLDEST_READABLE
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### cambio en spatial sin tocar binary
- `solo l├®xico`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)

### cambio en verify sin tocar binary ni hooks
- `solo l├®xico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L60 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)
