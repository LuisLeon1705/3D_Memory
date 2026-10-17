# Resultado de `cthg eval` — eval/3d-memory-prueba

AGENTS.md: 36 bloques importados (0 fusionados con otro) · CORE propuesto por import: 12 reglas · 10 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 1265 | 0 % | 100 % | 12 % | 0 |
| anidado ideal | 1121 | 11 % | 100 % | 13 % | 0 |
| solo léxico | 662 | 48 % | 85 % | 20 % | 0 |
| espacial | 438 | 65 % | 70 % | 24 % | 2 |
| espacial + lectura | 451 | 64 % | 73 % | 24 % | 2 |
| todo | 662 | 48 % | 85 % | 20 % | 0 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✔ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| candado de escritura | 573 | 75 % | 0 |
| rutas de Windows en el hook | 776 | 60 % | 0 |
| implementar la fase 6 (eval) | 1010 | 92 % | 0 |
| verify ejecuta comandos | 599 | 100 % | 0 |
| versión 3 del formato | 640 | 75 % | 0 |
| serde_json en el CLI | 737 | 100 % | 0 |
| umbral de similitud | 462 | 50 % | 0 |
| borrar un test molesto | 652 | 100 % | 0 |
| instalar los hooks aquí | 744 | 100 % | 0 |
| limpiar retrieve | 430 | 100 % | 0 |

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
- `solo léxico`: L22 (sin ruta y no CORE)
- `espacial`: L3 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE), L22 (sin ruta y no CORE), L24 (sin ruta y no CORE), L25 (sin ruta y no CORE), L26 (sin ruta y no CORE)
- `todo`: L22 (sin ruta y no CORE)

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
- `espacial`: L3 (sin ruta y no CORE)
- `espacial + lectura`: L3 (sin ruta y no CORE)
- `todo`: L3 (sin ruta y no CORE)

### instalar los hooks aquí
- `espacial`: L56! (sin ruta y no CORE), L57 (ruta no coincide)
- `espacial + lectura`: L56! (sin ruta y no CORE)
