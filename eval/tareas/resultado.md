# Resultado de `cthg eval` — eval/tareas

AGENTS.md: 8 bloques importados (0 fusionados con otro) · CORE propuesto por import: 4 reglas · 5 casos medidos

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 234 | 0 % | 100 % | 65 % | 0 |
| anidado ideal | 123 | 47 % | 66 % | 68 % | 7 |
| solo léxico | 269 | -15 % | 100 % | 65 % | 0 |
| espacial | 269 | -15 % | 100 % | 65 % | 0 |
| espacial + lectura | 269 | -15 % | 100 % | 65 % | 0 |
| todo | 269 | -15 % | 100 % | 65 % | 0 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✔ · recall ≥ 95 % ✔

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| worker 1 — id y almacenamiento | 269 | 100 % | 0 |
| worker 2 — interfaz de línea de comandos | 269 | 100 % | 0 |
| worker 3 — tests de integración | 269 | 100 % | 0 |
| encargo sin nombrar archivos — persistencia | 269 | 100 % | 0 |
| encargo sin nombrar archivos — errores del ejecutable | 269 | 100 % | 0 |

## Fallos

### worker 1 — id y almacenamiento
- `anidado ideal`: L4! (ruta no coincide), L5! (ruta no coincide)

### worker 2 — interfaz de línea de comandos
- `anidado ideal`: L6! (ruta no coincide)

### worker 3 — tests de integración
- `anidado ideal`: L4 (ruta no coincide), L5 (ruta no coincide), L6! (ruta no coincide)

### encargo sin nombrar archivos — persistencia
- `anidado ideal`: L4! (ruta no coincide), L5! (ruta no coincide)

### encargo sin nombrar archivos — errores del ejecutable
- `anidado ideal`: L6! (ruta no coincide)
