# Resultado de `cthg eval` — eval/3d-memory-prueba

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 17 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1282 | 0 % | 100 % | 11 % | 0 |
| anidado ideal | 1071 | 16 % | 88 % | 11 % | 6 |
| solo léxico | 437 | 66 % | 81 % | 27 % | 3 |
| espacial | 249 | 81 % | 60 % | 32 % | 9 |
| espacial + lectura | 256 | 80 % | 62 % | 32 % | 9 |
| todo | 435 | 66 % | 81 % | 27 % | 3 |
| todo + vecinos | 444 | 65 % | 85 % | 29 % | 1 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘
Criterio (`todo + vecinos`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| candado de escritura | 387 | 75 % | 0 |
| rutas de Windows en el hook | 638 | 60 % | 0 |
| implementar la fase 6 (eval) | 828 | 77 % | 0 |
| verify ejecuta comandos | 414 | 100 % | 0 |
| versión 3 del formato | 323 | 75 % | 0 |
| serde_json en el CLI | 409 | 100 % | 0 |
| umbral de similitud | 340 | 50 % | 0 |
| borrar un test molesto | 467 | 100 % | 0 |
| instalar los hooks aquí | 607 | 100 % | 0 |
| limpiar retrieve | 244 | 50 % | 1 |
| campo nuevo en Memory | 387 | 100 % | 0 |
| identificador decode en el mensaje | 327 | 67 % | 1 |
| guardar desde el CLI | 483 | 100 % | 0 |
| identificador crc32 en index | 244 | 67 % | 1 |
| plantilla de hooks desde README | 528 | 100 % | 0 |
| cambio en texto sin vecinos relevantes | 434 | 75 % | 0 |
| cambio en learn sin relación con binary ni hooks | 340 | 75 % | 0 |

## Por caso (`todo + vecinos`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| candado de escritura | 333 | 75 % | 0 |
| rutas de Windows en el hook | 638 | 60 % | 0 |
| implementar la fase 6 (eval) | 925 | 77 % | 0 |
| verify ejecuta comandos | 414 | 100 % | 0 |
| versión 3 del formato | 323 | 75 % | 0 |
| serde_json en el CLI | 409 | 100 % | 0 |
| umbral de similitud | 340 | 50 % | 0 |
| borrar un test molesto | 477 | 100 % | 0 |
| instalar los hooks aquí | 607 | 100 % | 0 |
| limpiar retrieve | 244 | 50 % | 1 |
| campo nuevo en Memory | 334 | 100 % | 0 |
| identificador decode en el mensaje | 370 | 100 % | 0 |
| guardar desde el CLI | 429 | 100 % | 0 |
| identificador crc32 en index | 323 | 100 % | 0 |
| plantilla de hooks desde README | 528 | 100 % | 0 |
| cambio en texto sin vecinos relevantes | 434 | 75 % | 0 |
| cambio en learn sin relación con binary ni hooks | 422 | 75 % | 0 |

## Fallos

### candado de escritura
- `solo léxico`: L33 (sin ruta y no CORE)
- `espacial`: L33 (sin ruta y no CORE)
- `espacial + lectura`: L33 (sin ruta y no CORE)
- `todo`: L33 (sin ruta y no CORE)
- `todo + vecinos`: L33 (sin ruta y no CORE)

### rutas de Windows en el hook
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L53! (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L53! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)

### implementar la fase 6 (eval)
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `todo + vecinos`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L65 (sin ruta y no CORE)

### verify ejecuta comandos
- `espacial`: L60 (sin ruta y no CORE)
- `espacial + lectura`: L60 (sin ruta y no CORE)

### versión 3 del formato
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)
- `todo + vecinos`: L41 (sin ruta y no CORE)

### umbral de similitud
- `solo léxico`: L3 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L34 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L34 (sin ruta y no CORE)
- `todo`: L3 (sin ruta y no CORE)
- `todo + vecinos`: L3 (sin ruta y no CORE)

### instalar los hooks aquí
- `anidado ideal`: L56! (ruta no coincide)
- `espacial`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L56! (ruta no coincide)

### limpiar retrieve
- `solo léxico`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `espacial`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `espacial + lectura`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `todo`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `todo + vecinos`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)

### campo nuevo en Memory
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### identificador decode en el mensaje
- `anidado ideal`: L35! (ruta no coincide)
- `solo léxico`: L35! (sin coincidencia léxica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### guardar desde el CLI
- `anidado ideal`: L35! (ruta no coincide)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)

### identificador crc32 en index
- `anidado ideal`: L35! (ruta no coincide)
- `solo léxico`: L35! (sin coincidencia léxica)
- `espacial`: L35! (ruta no coincide)
- `espacial + lectura`: L35! (ruta no coincide)
- `todo`: L35! (ruta no coincide)

### plantilla de hooks desde README
- `anidado ideal`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial`: L56! (ruta no coincide), L57 (ruta no coincide)
- `espacial + lectura`: L56! (ruta no coincide), L57 (ruta no coincide)

### cambio en texto sin vecinos relevantes
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE)

### cambio en learn sin relación con binary ni hooks
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L34! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)
- `todo + vecinos`: L22 (sin ruta y no CORE)
