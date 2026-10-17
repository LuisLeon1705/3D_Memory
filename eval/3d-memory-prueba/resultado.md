# Resultado de `cthg eval` — eval/3d-memory-prueba

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE confirmado en casos.md: 7 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 12 % | 0 |
| anidado ideal | 1121 | 11 % | 100 % | 13 % | 0 |
| solo léxico | 513 | 59 % | 79 % | 23 % | 1 |
| espacial | 252 | 80 % | 57 % | 31 % | 3 |
| espacial + lectura | 265 | 79 % | 60 % | 31 % | 3 |
| todo | 513 | 59 % | 79 % | 23 % | 1 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| candado de escritura | 387 | 75 % | 0 |
| rutas de Windows en el hook | 632 | 60 % | 0 |
| implementar la fase 6 (eval) | 894 | 77 % | 0 |
| verify ejecuta comandos | 414 | 100 % | 0 |
| versión 3 del formato | 560 | 75 % | 0 |
| serde_json en el CLI | 593 | 100 % | 0 |
| umbral de similitud | 340 | 50 % | 0 |
| borrar un test molesto | 467 | 100 % | 0 |
| instalar los hooks aquí | 601 | 100 % | 0 |
| limpiar retrieve | 244 | 50 % | 1 |

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
- `solo léxico`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L20 (sin ruta y no CORE), L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE), L64 (sin ruta y no CORE), L65 (sin ruta y no CORE)
- `todo`: L21 (sin ruta y no CORE), L22 (sin ruta y no CORE), L65 (sin ruta y no CORE)

### verify ejecuta comandos
- `espacial`: L60 (sin ruta y no CORE)
- `espacial + lectura`: L60 (sin ruta y no CORE)

### versión 3 del formato
- `solo léxico`: L41 (sin ruta y no CORE)
- `espacial`: L41 (sin ruta y no CORE)
- `espacial + lectura`: L41 (sin ruta y no CORE)
- `todo`: L41 (sin ruta y no CORE)

### umbral de similitud
- `solo léxico`: L3 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L34 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L34 (sin ruta y no CORE)
- `todo`: L3 (sin ruta y no CORE)

### instalar los hooks aquí
- `espacial`: L56! (sin ruta y no CORE), L57 (ruta no coincide)
- `espacial + lectura`: L56! (sin ruta y no CORE)

### limpiar retrieve
- `solo léxico`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `espacial`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `espacial + lectura`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
- `todo`: L34 (sin ruta y no CORE), L64! (sin ruta y no CORE)
