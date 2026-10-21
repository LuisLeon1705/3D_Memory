# Resultado de `cthg eval` ÔÇö eval/ejemplo

CLAUDE.md: 25 bloques importados (1 fusionados con otro) ┬À CORE confirmado en casos.md: 5 reglas ┬À 10 casos medidos

Sin etiquetar (no cuentan): textos del men├║ de opciones

| Configuraci├│n | Tokens medios | Ahorro vs completo | Recall medio | Precisi├│n media | Cr├¡ticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 503 | 0 % | 100 % | 17 % | 0 |
| anidado ideal | 183 | 64 % | 97 % | 42 % | 1 |
| solo l├®xico | 136 | 73 % | 74 % | 50 % | 4 |
| espacial | 124 | 75 % | 73 % | 54 % | 3 |
| espacial + lectura | 152 | 70 % | 93 % | 59 % | 1 |
| todo | 167 | 67 % | 93 % | 57 % | 1 |

Criterio (`todo`): ahorro ÔëÑ 70 % Ô£ÿ ┬À cr├¡ticas perdidas = 0 Ô£ÿ ┬À recall ÔëÑ 95 % Ô£ÿ

## Por caso (`todo`)

| Caso | Tokens | Recall | Cr├¡ticas perdidas |
|---|---:|---:|---:|
| arreglar el colisionador | 212 | 100 % | 0 |
| mensaje nuevo en el protocolo | 207 | 100 % | 0 |
| color de los botones | 161 | 100 % | 0 |
| desplegar | 144 | 33 % | 1 |
| tests de integraci├│n | 158 | 100 % | 0 |
| memoria de los cuerpos | 163 | 100 % | 0 |
| columna nueva en jugadores | 144 | 100 % | 0 |
| dependencia nueva | 100 | 100 % | 0 |
| paso de simulaci├│n variable | 168 | 100 % | 0 |
| tiempo de espera de conexiones | 209 | 100 % | 0 |

## Fallos

### arreglar el colisionador
- `solo l├®xico`: L15 (sin coincidencia l├®xica), L16! (sin coincidencia l├®xica), L17 (sin coincidencia l├®xica)

### mensaje nuevo en el protocolo
- `solo l├®xico`: L24 (sin coincidencia l├®xica), L25 (sin coincidencia l├®xica)
- `espacial`: L23! (ruta no coincide), L24 (ruta no coincide), L25 (ruta no coincide)

### color de los botones
- `espacial`: L30 (ruta no coincide)

### desplegar
- `anidado ideal`: L41! (ruta no coincide)
- `solo l├®xico`: L41! (sin coincidencia l├®xica), L42 (sin ruta y no CORE)
- `espacial`: L41! (ruta no coincide), L42 (sin ruta y no CORE)
- `espacial + lectura`: L41! (ruta no coincide), L42 (sin ruta y no CORE)
- `todo`: L41! (sin coincidencia l├®xica), L42 (sin ruta y no CORE)

### tests de integraci├│n
- `espacial`: L36 (ruta no coincide)

### memoria de los cuerpos
- `solo l├®xico`: L15 (sin coincidencia l├®xica), L18! (sin coincidencia l├®xica)

### columna nueva en jugadores
- `solo l├®xico`: L43! (sin coincidencia l├®xica)
- `espacial`: L43! (ruta no coincide)

### paso de simulaci├│n variable
- `solo l├®xico`: L15 (sin coincidencia l├®xica)

### tiempo de espera de conexiones
- `solo l├®xico`: L24 (sin coincidencia l├®xica)
- `espacial`: L24 (ruta no coincide), L26 (ruta no coincide)
