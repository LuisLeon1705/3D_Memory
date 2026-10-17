# Resultado de `cthg eval` — eval/3d-memory-prueba

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 12 % | 0 |
| anidado ideal | 947 | 25 % | 85 % | 12 % | 1 |
| solo léxico | 457 | 64 % | 76 % | 26 % | 1 |
| espacial | 246 | 81 % | 55 % | 30 % | 4 |
| espacial + lectura | 266 | 79 % | 59 % | 31 % | 4 |
| todo | 468 | 63 % | 77 % | 26 % | 1 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| candado de escritura | 387 | 75 % | 0 |
| rutas de Windows en el hook | 593 | 60 % | 0 |
| implementar la fase 6 (eval) | 604 | 62 % | 0 |
| verify ejecuta comandos | 416 | 100 % | 0 |
| versión 3 del formato | 566 | 75 % | 0 |
| serde_json en el CLI | 572 | 100 % | 0 |
| umbral de similitud | 345 | 50 % | 0 |
| borrar un test molesto | 386 | 100 % | 0 |
| instalar los hooks aquí | 561 | 100 % | 0 |
| limpiar retrieve | 246 | 50 % | 1 |

## Fallos

### candado de escritura
- `solo léxico`: L33 (sin ruta y no CORE)
- `espacial`: L33 (sin ruta y no CORE)
- `espacial + lectura`: L33 (sin ruta y no CORE)
- `todo`: L33 (sin ruta y no CORE)

### rutas de Windows en el hook
- `solo léxico`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)
- `espacial`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L53! (sin ruta y no CORE)
- `espacial + lectura`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE), L53! (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE), L33 (sin ruta y no CORE)

### implementar la fase 6 (eval)
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin coincidencia léxica), L65 (sin ruta y no CORE)
- `espacial`: L3 (ruta no coincide), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (ruta no coincide), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `espacial + lectura`: L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L65 (sin ruta y no CORE)

### verify ejecuta comandos
- `espacial`: L60 (sin ruta y no CORE)
- `espacial + lectura`: L60 (sin ruta y no CORE)

### versión 3 del formato
- `anidado ideal`: L35! (ruta no coincide)
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L35! (ruta no coincide), L41 (sin ruta y no CORE)
- `espacial + lectura`: L35! (ruta no coincide), L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)

### umbral de similitud
- `anidado ideal`: L3 (ruta no coincide), L34 (ruta no coincide)
- `solo léxico`: L3 (sin coincidencia léxica)
- `espacial`: L3 (ruta no coincide), L34 (ruta no coincide)
- `espacial + lectura`: L3 (ruta no coincide), L34 (ruta no coincide)
- `todo`: L3 (sin coincidencia léxica)

### instalar los hooks aquí
- `espacial`: L56! (sin ruta y no CORE), L57 (ruta no coincide)
- `espacial + lectura`: L56! (sin ruta y no CORE)

### limpiar retrieve
- `anidado ideal`: L34 (ruta no coincide)
- `solo léxico`: L34 (sin coincidencia léxica), L64! (sin ruta y no CORE)
- `espacial`: L34 (ruta no coincide), L64! (sin ruta y no CORE)
- `espacial + lectura`: L34 (ruta no coincide), L64! (sin ruta y no CORE)
- `todo`: L34 (ruta no coincide), L64! (sin ruta y no CORE)
