# Resultado de `cthg eval` — eval/ejemplo

CLAUDE.md: 25 bloques importados (1 fusionados con otro) · CORE confirmado en casos.md: 5 reglas · 10 casos medidos

Sin etiquetar (no cuentan): textos del menú de opciones

| Configuración | Tokens medios | Ahorro vs completo | Recall medio | Precisión media | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 491 | 0 % | 100 % | 17 % | 0 |
| anidado ideal | 183 | 63 % | 97 % | 42 % | 1 |
| solo léxico | 136 | 72 % | 74 % | 50 % | 4 |
| espacial | 124 | 75 % | 73 % | 54 % | 3 |
| espacial + lectura | 152 | 69 % | 93 % | 59 % | 1 |
| todo | 171 | 65 % | 93 % | 56 % | 1 |

Criterio (`todo`): ahorro ≥ 70 % ✘ · críticas perdidas = 0 ✘ · recall ≥ 95 % ✘

## Por caso (`todo`)

| Caso | Tokens | Recall | Críticas perdidas |
|---|---:|---:|---:|
| arreglar el colisionador | 212 | 100 % | 0 |
| mensaje nuevo en el protocolo | 207 | 100 % | 0 |
| color de los botones | 161 | 100 % | 0 |
| desplegar | 144 | 33 % | 1 |
| tests de integración | 158 | 100 % | 0 |
| memoria de los cuerpos | 163 | 100 % | 0 |
| columna nueva en jugadores | 144 | 100 % | 0 |
| dependencia nueva | 100 | 100 % | 0 |
| paso de simulación variable | 209 | 100 % | 0 |
| tiempo de espera de conexiones | 209 | 100 % | 0 |

## Fallos

### arreglar el colisionador
- `solo léxico`: L15 (sin coincidencia léxica), L16! (sin coincidencia léxica), L17 (sin coincidencia léxica)

### mensaje nuevo en el protocolo
- `solo léxico`: L24 (sin coincidencia léxica), L25 (sin coincidencia léxica)
- `espacial`: L23! (ruta no coincide), L24 (ruta no coincide), L25 (ruta no coincide)

### color de los botones
- `espacial`: L30 (ruta no coincide)

### desplegar
- `anidado ideal`: L41! (ruta no coincide)
- `solo léxico`: L41! (sin coincidencia léxica), L42 (sin ruta y no CORE)
- `espacial`: L41! (ruta no coincide), L42 (sin ruta y no CORE)
- `espacial + lectura`: L41! (ruta no coincide), L42 (sin ruta y no CORE)
- `todo`: L41! (sin coincidencia léxica), L42 (sin ruta y no CORE)

### tests de integración
- `espacial`: L36 (ruta no coincide)

### memoria de los cuerpos
- `solo léxico`: L15 (sin coincidencia léxica), L18! (sin coincidencia léxica)

### columna nueva en jugadores
- `solo léxico`: L43! (sin coincidencia léxica)
- `espacial`: L43! (ruta no coincide)

### paso de simulación variable
- `solo léxico`: L15 (sin coincidencia léxica)

### tiempo de espera de conexiones
- `solo léxico`: L24 (sin coincidencia léxica)
- `espacial`: L24 (ruta no coincide), L26 (ruta no coincide)
