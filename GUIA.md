# 3D-Memory — guía de diseño

Memoria de proyecto para agentes de código (Claude Code primero). Objetivo: sustituir la mayor parte de un `CLAUDE.md` / `AGENTS.md` por **un núcleo pequeño que se inyecta siempre + los recuerdos que aplican a lo que se está tocando**, con confianza, causas y tiempo explícitos.

Motor: **CTHG** (*Causal-Temporal HyperGraph*): un grafo de recuerdos en un archivo binario, con tres ejes.

Estado: `cthg-core` completo y probado (56 tests, más 1 del CLI); funciona con Claude Code y con Google Antigravity; CLI con `init`, `learn`, `confirm`, `deny`, `core`, `recall`, `list`, `mv`, `import`, `hook`, `verify`, `eval` y `corrections`. La memoria captura sola las correcciones del usuario al final de cada turno, como propuestas que no se inyectan hasta `cthg confirm` (sección 6.1). Falta la medición sobre repos reales y la fase 7 (ver [Fases](#13-fases)).

---

## 1. Qué problema resuelve

| Un `CLAUDE.md` | 3D-Memory |
|---|---|
| Se carga entero en cada sesión, venga o no al caso | Un núcleo que crece con el proyecto y se estabiliza (sección 4.4) + lo que aplica a la tarea y a los archivos tocados |
| Todas las líneas pesan igual | χ ∈ [−1, 1]: verificado / probable / sin evidencia / dudoso / descartado |
| Las reglas caducan sin avisar | El tiempo solo avanza; lo sustituido o contradicho se hunde en vez de convivir |
| No dice *por qué* existe una regla | Aristas causales: regla ← incidente / decisión que la motivó |
| Mezcla planes con hechos | Los planes son *potenciales*: no existen hasta que algo demuestra que ocurrieron |
| Lo decidido en una rama contamina main | Lo aprendido en una rama solo existe en esa rama hasta que se fusiona |

Lo que **no** promete: cero fallos de recuperación. Un `CLAUDE.md` estático nunca olvida una regla; uno recuperado sí puede. El diseño gira alrededor de reducir ese riesgo y de **medirlo** (sección 12).

---

## 2. El modelo

Cada recuerdo es un **nodo**:

| Campo | Significado |
|---|---|
| `kind` | `regla`, `decision`, `hecho`, `incidente`, `convencion`, `plan`, `comando`, `sesion` |
| `origin` | `repo` (lo demuestra el repo), `importado` (de un `CLAUDE.md`), `usuario`, `agente`, `sesion` |
| `path`, `symbol` | **X**: dónde aplica |
| `y`, `commit`, `branch` | **Y**: cuándo y en qué rama se aprendió |
| `alpha`, `beta` | evidencia a favor / en contra → **Z** (χ) |
| `activation`, `recalls` | lo reciente que está (olvido) |
| flags | `ANCHORED` (probado por el repo), `CORE` (siempre se inyecta), `POTENTIAL` (plan), `CONSOLIDATED`, `STALE` (el repo ya no coincide) |

Y **aristas** entre nodos: `causa` (con su propio α/β = cuán determinista es), `contradice`, `sustituye` (nuevo → viejo), `relacion` (hebbiana), `menciona`, `precede`; más **hiperaristas** que agrupan varios nodos (un incidente, un PR, una decisión que tocó varios sitios).

---

## 3. Los ejes

### 3.1 X — el árbol del repositorio

X no es un número: es una **ruta** normalizada (relativa, separada por `/`). La ruta vacía significa «todo el proyecto». Opcionalmente se afina con un símbolo (`collider.rs::resolve_contacts`).

Sea `prof(p)` el número de componentes de la ruta (`prof("src/core/a.rs") = 3`, `prof("") = 0`), y `a ⪯ p` «`a` es `p` o una carpeta antecesora de `p`» (comparando componentes enteros: `src/co` **no** es antecesora de `src/core/a.rs`).

**Afinidad** de un nodo anclado en `a` con un archivo objetivo `t`:

```
             ⎧ λ^(prof(t) − prof(a)) · σ     si a ≠ "" y a ⪯ t
afinidad  =  ⎨
             ⎩ 0                             en otro caso

λ = 0,8
σ = 0,5  si el nodo nombra un símbolo, t es el mismo archivo y los símbolos tocados de t no lo incluyen
σ = 1    en otro caso
```

Consecuencias:

- Una regla del archivo exacto vale 1; la de su carpeta 0,8; la de la carpeta de encima 0,64. **Herencia**: una regla puesta en `src/core/` aplica a todo lo que cuelga de ahí, y lo más concreto pesa más.
- La raíz (`""`) tiene afinidad 0 con todo: **una regla sin lugar en el árbol nunca se recupera por ruta**. Por eso las reglas transversales tienen que ser `CORE`.
- **Renombres**: si `src/core` pasa a `src/physics`, todos los nodos con `src/core ⪯ path` se reescriben (`cthg mv`; automático con `git diff -M` y, en `verify`, con los renombres ya confirmados en commits). Lo automático solo ocurre **en el tronco**: en otra rama el tronco sigue teniendo las rutas viejas.

### 3.2 Y — el tiempo, solo hacia delante

Y es tiempo Unix en segundos. Al escribir:

```
y_nuevo = max( t_reloj ,  max{ y_n : n no es plan } + ε )        ε = 1 ms
```

Nada escrito se mueve ni se borra; un reloj que retrocede no puede mover Y hacia atrás. Cada nodo guarda además el `commit` y la `branch`.

### 3.3 Vista y visibilidad

Todo se calcula **para una vista** `(ahora, ramas)`. Un nodo es visible si:

```
V(n) = [ y_n ≤ ahora ]  ∧  [ n no es POTENTIAL ]  ∧  [ branch_n = "" ∨ branch_n ∈ ramas ]
```

Un nodo invisible tiene χ = −1 y no puede recuperarse. Así:

- Un **plan** no existe en el pasado. Solo se vuelve real cuando un aprendizaje posterior lo describe (similitud ≥ 0,55 en una ruta compatible): entonces se le quita `POTENTIAL` y se le pone `y = y_nuevo` («se cumple»). El paso del tiempo no lo cumple.
- Lo aprendido en `feature/x` no existe desde `main` hasta que esa rama se fusiona (fase 7: `git merge-base --is-ancestor`).

### 3.4 Z — la confianza χ

χ mezcla dos lecturas del mismo recuerdo.

**Lectura de evidencia (A)**. Cada nodo lleva una distribución Beta(α, β). Su media reescalada a [−1, 1]:

```
A = 2·α/(α+β) − 1
```

El punto de partida (prior) depende del origen:

| Origen | (α₀, β₀) | A inicial | Por qué |
|---|---|---|---|
| `repo` | (20; 0,5) | +0,95 | lo demuestra el propio código |
| `importado` | (5; 1,5) | +0,54 | lo escribió una persona en un `CLAUDE.md` |
| `usuario` | (4; 1) | +0,60 | lo dijo el usuario |
| `usuario`, corrección capturada | (3; 1) | +0,50 | lo dijo el usuario, pero lo eligió una heurística: es una propuesta, oculta hasta `confirm` (sección 6.1) |
| `sesion` | (3; 1) | +0,50 | resumen de sesión |
| `agente` | (1,5; 1,5) | 0 | deducción propia: **no cuenta como prueba** |

Confirmar suma 1 a α; negar suma 1 a β.

**Lectura estructural (E)**. Se recorren los nodos visibles en orden de `(y, id)`. Como una causa solo cuenta si es anterior a su efecto, el grafo causal es acíclico y basta **una pasada**:

```
       ⎧ +1                                  si n está ANCHORED
       ⎪ −1                                  si n contradice algo visible y establecido
       ⎪                                     (anclado, o anterior con χ > 0,5)
E(n) = ⎨ −1                                  si lo sustituye un nodo visible con χ > 0
       ⎪ −1                                  si está STALE (verify vio que el repo ya no coincide)
       ⎪ max  max(E(p), 0) · det(p→n)        sobre las causas visibles p de n
       ⎩ 0                                   si no tiene causas

det(e) = clamp( 2·α_e/(α_e+β_e) − 1 , 0, 1 )      (cuán determinista es la arista causal)
```

Es decir: un recuerdo está tan *explicado* como su mejor cadena causal hasta algo probado por el repo, y cada eslabón debilita según lo determinista que sea.

**Mezcla**. Con poca evidencia manda la estructura; con mucha, la estadística:

```
e = α + β − 2                 (evidencia acumulada, incluido el prior)
w = K / (K + e)               K = 3
χ = clamp( w·E + (1 − w)·A , −1, 1 )
χ_anclado = max(χ, 0,9)       χ_invisible = −1
```

Tras la pasada principal se aplican dos correcciones, porque apuntan hacia delante en el tiempo: los nodos sustituidos y los que contradicen a algo **posterior** con más χ que ellos pasan a E = −1 y se recalcula χ.

**Bandas** (lo que ve el agente):

| χ | Banda |
|---|---|
| ≥ 0,75 | verificado |
| [0,25; 0,75) | probable |
| (−0,25; 0,25) | sin evidencia |
| (−0,75; −0,25] | dudoso |
| ≤ −0,75 | descartado (se muestra como «ya se probó o se sustituyó: no repetir») |

**Consolidación**: un nodo no anclado se marca `CONSOLIDATED` cuando la evidencia ganada sobre su prior, `(α+β) − (α₀+β₀)`, llega a 3 y A ≥ 0,6.

#### Ejemplos con números

1. **Regla del usuario recién dicha**: α=4, β=1 → A = 0,6; e = 3 → w = 0,5; sin causas, E = 0 → χ = 0,5·0 + 0,5·0,6 = **0,30** (probable).
2. **La misma regla confirmada tres veces**: α=7, β=1 → A = 0,75; e = 6 → w = 1/3 → χ = 0,5 (probable, consolidada).
3. **Regla causada por un hecho del repo** con arista α_e=9, β_e=1 → det = 0,8 → E = 0,8; con el prior de usuario, χ = 0,5·0,8 + 0,5·0,6 = **0,70**.
4. **El agente niega un hecho del repo**: el nodo de la negación nace con prior de agente y β+2 → α=1,5, β=3,5 → A = −0,4; e = 3 → w = 0,5; contradice algo anclado → E = −1 → χ = −0,5 − 0,2 = **−0,70** (dudoso). El hecho anclado sigue en ≥ 0,9.
5. **Regla sustituida**: la vieja (α=4, β=1) pasa a E = −1 → χ = −0,5 + 0,3 = **−0,20**; aún visible en el histórico, ya sin peso.

---

## 4. Recuperación

### 4.1 Canales

| Canal | Cuándo | Entrada | Qué cubre |
|---|---|---|---|
| **0 · núcleo** | al iniciar la sesión | `CORE` global (todos los proyectos) y luego el del proyecto | reglas transversales sin lugar en el árbol |
| **1 · espacial (prompt)** | con cada mensaje | `git status` y símbolos de los hunks de `git diff` | lo que ya se está tocando |
| **2 · espacial (justo a tiempo)** | cuando el agente **lee** un archivo | esa ruta | lo que se va a tocar: llega antes de cualquier edición, porque editar exige leer antes |
| **3 · léxico** | con cada mensaje | el texto del prompt | preguntas de intención sin archivo («¿cómo despliego?») |

### 4.2 Semillas

**Léxica** (solo nodos visibles con χ ≥ χ_min):

```
r_lex(n) = 2·[la etiqueta o un alias de n aparece como frase en el prompt]
         + 1,5 · |tokens(prompt) ∩ tokens(n)| / √|tokens(n)|
```

Los tokens se pliegan (minúsculas, sin tildes), se quitan palabras vacías en español e inglés y se aplica un stemming ligero (`reglas` ~ `regla`). Las rutas también se tokenizan (`src/core/collider.rs` → `src core collider`).

**Espacial**, con un peso por objetivo según cuánto sabemos que importa ahora:

```
r_esp(n) = max_t  peso(t) · afinidad(n, t)

peso = 1,0  archivo recién leído o pasado con --path
       0,8  modificado en git status
       0,5  sin seguimiento (??)
```

Se descartan los lockfiles y `target/`, `node_modules/`, `dist/`, `build/`, `.cthg/`, se ordenan por peso y tamaño del diff (`git diff -M -U0`) y se toman como mucho 12 rutas.

**Símbolos tocados**: para cada hunk se lee el archivo y se busca hacia arriba, desde la primera línea del hunk, la declaración más cercana (`fn`, `def`, `function`, `class`, `impl`…; los comentarios no cuentan). La cabecera `@@` de git no basta: para un método indentado muestra `impl Collider {`, y una regla anclada en el método editado quedaba penalizada con σ = 0,5. Se guardan los dos (`resolve_contacts` y `Collider`), así las reglas ancladas al tipo también coinciden.

**Vecinos e identificadores del grafo de código (Fase 8b)**:

Cuando se dispone del índice de código derivado (`.cthg/indice.tsv`), el canal espacial se enriquece con la topología real del código:
1. **Vecinos de importación (1 salto)**: cada objetivo espacial $t$ (archivo leído en lectura JIT, editado o modificado en git) expande como objetivos a sus vecinos inmediatos (lo que importa y lo que lo importa):
   $$\text{peso}(v) = W_{\text{vecino}} \cdot \text{peso}(t), \quad W_{\text{vecino}} = 0,5$$
   - Máximo 6 vecinos por objetivo (`MAX_VECINOS = 6`), ordenados descendentemente por número de enlaces entre ambos archivos.
   - Estrictamente a un salto: nunca se admiten vecinos de vecinos.
   - Si un archivo ya era un objetivo directo, conserva el peso mayor y mantiene su condición de objetivo directo.
2. **Identificadores en el mensaje**: los identificadores presentes en el texto del prompt que coinciden exactamente (sensibles a mayúsculas, incluyendo `Tipo::metodo`) con símbolos del índice:
   - Convierten en objetivo espacial a su archivo de definición con peso $W_{\text{símbolo}} = 0,9$.
   - Asignan dicho símbolo a `Target.symbols`, permitiendo que el factor $\sigma$ distinga reglas del símbolo frente a otras del mismo archivo.
   - Filtros de relevancia: se descartan identificadores de menos de 4 caracteres (`MIN_IDENT_LEN = 4`), símbolos definidos en más de 3 archivos (`MAX_DEFS = 3`) y símbolos declarados dentro de módulos de prueba (`mod tests`).
3. **Desempate directo vs. vecino**: si una regla llega a la vez por vía directa y como vecina, la vía directa prevalece estrictamente (afinidad directa íntegra, sin atenuación y sin prefijo `vecino de`).
4. **Atribución explícita**: la regla inyectada comunica al agente el motivo del enrutamiento:
   `- #3 regla (src/cli.rs, vecino de src/main.rs): ...`
5. **Invariante de propuestas**: un recuerdo en estado de propuesta (`PROPOSED`) solo se desbloquea leyendo su propio archivo directo (`may_inject(n, Some(read_path))`), nunca a través de vecindad.

**Fusión**:

```
s(n) = r_lex(n) + W · r_esp(n)        W = 2
```

W = 2 hace que tocar el archivo exacto de una regla pese lo mismo que nombrarla literalmente en el prompt.

### 4.3 Propagación (un salto)

```
r(i) = s(i)
     + Σ_{aristas j→i} s(j)·ω
     + Σ_{aristas i→j} s(j)·ω·0,7
     + 0,15 · max_{hiperaristas h ∋ i} max_{k ∈ h} s(k)
```

| Arista | ω | Para qué |
|---|---|---|
| causa | 0,45 · det | traer el **por qué** de una regla |
| relacion | 0 (inerte, ver 4.5) | — |
| contradice, sustituye | 0,3 | traer lo que **ya se probó** y se descartó |
| menciona | 0,2 | |
| precede | 0,15 | |

### 4.4 Ranking y presupuesto

```
R(n) = ( r(n) + 0,3·activación(n) ) · ( 0,6 + 0,4·χ(n) )
       solo si r(n) > max(0,05; 0,25 · max_m r(m)) y n no es CORE
```

El corte relativo (un cuarto de la mejor coincidencia de la misma consulta) quita la cola de coincidencias débiles.

El factor de confianza va de 0,2 (χ = −1) a 1 (χ = +1): lo descartado puede aparecer, pero abajo.

Después se recorta con un presupuesto de tokens (≈ caracteres/4), en orden de R, saltando lo que no cabe. La salida se agrupa por bandas de χ, cada línea con su ruta y su causa (`← …`).

#### Presupuestos dinámicos

Un presupuesto fijo (antes: núcleo 500, mensaje 800, archivo 300) falla por los dos lados. En un proyecto ligero recorta un archivo que cabría entero. En uno pesado (`.md` de más de 10 000 tokens) se queda corto para las reglas que deben estar siempre. Ahora todos dependen de `S`, lo que costaría inyectar toda la memoria del proyecto: texto + ruta + 6 tokens de línea por recuerdo, sin planes, obsoletos ni sesiones.

```
B(S; c, piso) = max( min(S, piso),  S · c / (c + S) )      (crece con S y se estabiliza en c)

núcleo   = B(S; 1 500, 300)
mensaje  = B(S; 2 000, 300) · h / (h + q)        q = palabras útiles del mensaje, h = 12
archivo  = B(S;   600, 150)
conversación (solo memoria del proyecto) ≤ max(S, 2 000)
capa global: su propio tope de 600, no comparte con el proyecto
```

- **El piso:** una memoria pequeña entra entera (`S ≤ WHOLE_MAX`, con `WHOLE_MAX = 300`), porque decidir qué dejar fuera cuesta más que darla. En ese caso `core_context` no se limita a los nodos marcados `CORE`: selecciona primero los `CORE` y a continuación el resto de recuerdos inyectables ordenados por χ (descartando planes `POTENTIAL`, obsoletos `STALE`, sesiones `Session`, propuestas sin confirmar `PROPOSED`, nativos y duplicados de la capa global). El presupuesto del núcleo pasa a ser `max(budgets.core, S)`, y lo inyectado descuenta de `spent`.
- **La curva:** crece casi como `S` al principio y se estabiliza en `c`. Un proyecto enorme no paga en proporción a su tamaño.
- **El factor de especificidad** `h/(h + q)` va al revés que el canal léxico. Un mensaje vago («arréglalo», q = 1) recibe el 92 % de su presupuesto, porque es el que más necesita que la memoria complete lo que no dice. Uno de 12 palabras útiles recibe el 50 %, y uno de 36, el 25 %.
- **El tope por conversación** (`spent` en el estado de sesión): por larga que sea, la memoria del proyecto nunca inyecta más que el archivo estático que sustituye. Así se corta la acumulación medida en Antigravity (5,5 k tokens en el turno 6 frente a 3,3 k del `.md`).

| `S` (tamaño de la memoria) | núcleo | mensaje (q = 1 · q = 12) | archivo | conversación |
|---:|---:|---:|---:|---:|
| 120 (proyecto mínimo) | 120 | 111 · 60 | 120 | 2 000 |
| 264 (`eval/tareas`, `exp-memoria`) | 264 | 224 · 122 | 196 | 2 000 |
| 1 265 (`AGENTS.md` de este repo) | 686 | 715 · 388 | 407 | 2 000 |
| 12 800 (los tres `.md` de Clay-Myrrh) | 1 343 | 1 597 · 865 | 573 | 12 800 |
| 200 000 | 1 489 | 1 832 · 992 | 598 | 200 000 |

`h = 12` se eligió con el conjunto de ajuste: con `h = 6`, un mensaje de 4 palabras ya perdía el 40 % y se caían reglas relevantes «fuera de presupuesto».

`WHOLE_MAX = 300` se calibró comparando `eval/tareas` (`S = 264`) antes y después:
- **Antes (`WHOLE_MAX = 0` o solo `CORE`):** canal `todo` inyectaba 224 tokens (ahorro 4 %, recall 89 %, 2 críticas perdidas: `L10!` en worker 2 por no tener ruta ni entrar en los 4 nodos auto-CORE; `L6!` en worker 3 anclada a `src/cli.rs` que un prompt sobre `tests/` no tocaba). En canales espaciales puros (`espacial` / `espacial + lectura`), recall 54 % y 9 críticas perdidas.
- **Después (`WHOLE_MAX = 300`):** entra la memoria entera al inicio (269 tokens formateados con prefijos). Ahorro -15 %, recall **100 %**, **0 críticas perdidas** en todos los canales (incluyendo workers que escriben sin leer o mensajes sin rutas).
- **Control de regresión:** en `eval/ejemplo` (`S = 588`), `eval/3d-memory` (`S = 1 265`) y `eval/3d-memory-prueba` (`S = 1 265`), como `S > WHOLE_MAX`, los resultados son **100 % idénticos** antes y después en las 6 configuraciones.

| `todo`, antes → después | CORE humano | CORE automático |
|---|---|---|
| ajuste (`eval/3d-memory`) | 490 tok · 78 % → 490 · 78 % | 652 · 87 % → 699 · **89 %** |
| prueba (una sola vez) | 473 · 79 % → 466 · 79 % | 637 · 85 % → 684 · **90 %** |
| ejemplo sintético | 167 · 93 % → igual | — |
| tareas (`exp-memoria`, `S = 264`) | — | 224 tok · 89 % (2 críticas) → 269 · **100 % (0 críticas)** |

Las críticas perdidas no cambian en memorias medianas o grandes: 1 con el CORE humano y 0 con el automático. En memorias pequeñas (`S ≤ 300`), la inyección entera garantiza 0 críticas perdidas sin depender de que el prompt mencione los archivos.

### 4.5 Olvido y asociación

Por sesión: `activación ← 0,92 · activación`. Cuando un nodo se inyecta: `activación = 1`.

**Inyectados juntos no es asociados.** Antes, cada inyección unía con `relacion` a todos los pares de nodos inyectados a la vez. Cada inyección se volvía una camarilla, y una sola palabra que coincidiera arrastraba la camarilla entera, que al inyectarse volvía a reforzarse. Lo detectó el agente de Antigravity al usar la memoria: en este repo había 254 aristas `relacion` entre 36 nodos, y un mensaje cuya única palabra técnica era «tokens» inyectaba 23 nodos. Ahora la co-inyección solo refresca la activación, y las aristas `relacion` que existan no propagan (peso 0) hasta que algo aprenda asociaciones con sentido. El mismo mensaje inyecta 2 nodos. En los conjuntos de medición el recall no cambia, y los tokens bajan entre un 3 y un 9 %.

### 4.6 Ejemplo: «arréglalo» sin una sola palabra útil

El prompt es «arréglalo» y en `git status` está modificado `src/core/collider.rs` (peso 0,8). La memoria tiene:

- #1 regla en `src/core/collider.rs`: «No uses unwrap en el colisionador», causada por
- #2 incidente en el mismo archivo: «Un panic tumbó el servidor…» (arista α=2, β=1 → det = 1/3 → ω = 0,15)
- #3 regla en `src/core`: «Todo el núcleo es determinista»
- #4 regla en `src/ui`: «Los botones usan el tema oscuro»

| Nodo | s | r tras propagar | R (χ = 0,30, activación 1) |
|---|---|---|---|
| #1 | 2·0,8·1 = 1,60 | 1,60 + 1,60·0,15 = 1,84 | (1,84+0,3)·0,72 = **1,54** |
| #2 | 1,60 | 1,60 + 1,60·0,15·0,7 = 1,77 | **1,49** |
| #3 | 2·0,8·0,8 = 1,28 | 1,28 | **1,14** |
| #4 | 0 | 0 | — (fuera) |

#### Ejemplo con vecinos del grafo de código

Si el archivo `src/main.rs` importa `src/cli.rs` (enlace de 1 salto) y se edita `src/main.rs` (peso objetivo = 1,0):
- Objetivo directo: `src/main.rs`, peso = 1,0.
- Objetivo vecino derivado: `src/cli.rs`, peso = $0,5 \cdot 1,0 = 0,5$, `neighbor_of = Some("src/main.rs")`.
- Para una regla $n$ anclada en `src/cli.rs`:
  - Afinidad directa desde `src/main.rs`: 0 (no es antecesor ni idéntico en el árbol).
  - Afinidad desde el vecino: $\text{peso}(v) \cdot \lambda^0 \cdot \sigma = 0,5 \cdot 1,0 \cdot 1,0 = 0,50$.
  - Semilla espacial: $s(n) = 0 + 2 \cdot 0,50 = 1,00$.
  - Línea inyectada: `- #3 regla (src/cli.rs, vecino de src/main.rs): ...`

Salida real de `cthg recall --prompt "arréglalo" --git`:

```
## Memoria relevante
[probable]
- #1 regla (src/core/collider.rs): No uses unwrap en el colisionador ← Un panic tumbó el servidor en el partido tres
- #2 incidente (src/core/collider.rs): Un panic tumbó el servidor en el partido tres
- #3 regla (src/core): Todo el núcleo es determinista, sin rand
```

---

## 5. Por qué el canal espacial no basta solo

Entrar por la ruta es la pieza más importante: es determinista y no depende de cómo se escriba el mensaje. Pero no da «cero fallos» por sí mismo:

| Hueco | Ejemplo | Solución |
|---|---|---|
| El archivo aún no se ha tocado | «añade un endpoint de pagos» con `git status` limpio | **canal 2**: inyectar al leer el archivo |
| Reglas sin lugar en el árbol | «nunca hagas push a main», «comentarios en inglés» | **`CORE`** (la raíz tiene afinidad 0 a propósito) |
| Árbol de trabajo sucio | lockfiles, generados, veinte archivos a medias | lista de ruido, ranking por diff, máximo de rutas, pesos, presupuesto |
| Renombres | `src/core/` pasa a `src/physics/` | `cthg mv`; automático con `git diff -M` |
| Reglas de carpeta | aplica a todo `src/core/` | herencia λ^d |
| Funciones concretas | la regla es de `resolve_contacts`, no del archivo | X fino por símbolo (σ) |
| Preguntas sin archivo | «¿cómo despliego?» | **canal 3** léxico, con alias |
| Fuera de git | no hay `status` | canales 0, 2 y 3 siguen funcionando |

Riesgo residual: una regla crítica que no es `CORE` y está mal anclada. Es medible (sección 12): cada fallo se lista y se corrige.

### 5.1 Índice del proyecto (grafo de código)

El canal espacial original solo conocía el árbol de carpetas (en `spatial.rs`: un ancla que no es el objetivo ni su antecesor da afinidad 0) y solo identificaba símbolos tocados mediante los hunks de `git diff`. La persona o el agente debían construir el mapa del código a mano explorando con `grep` y `view_file`. La fase 8 automatiza este entendimiento estructural.

#### Decisiones de diseño (memorias #46 y #48)
- **Índice derivado y reconstruible, no memoria**:
  - No entra en `memoria.cthg` (no incrementa `VERSION` de `binary.rs`).
  - No cuenta en $S$ (`hook::memory_size`).
  - Nunca se inyecta como regla en el contexto del agente.
  - *Motivo*: $Y$ solo avanza hacia delante, mientras que el código cambia y se refactoriza continuamente; almacenar el AST o grafo de código como nodos de memoria dejaría basura histórica permanente e inflaría desproporcionadamente los presupuestos de contexto.
- **Qué se extrae**: archivos, símbolos de primer nivel (`fn`, métodos con su tipo contenedor `parent`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`, `macro_rules!`, `class`, `interface`, `var`, `id`), sus **firmas** (`signature`: qué reciben y devuelven, hasta 160 caracteres), decoradores en Python (`decorators`), importaciones resueltas entre archivos del repositorio y llamadas símbolo → nombre. Las variables locales **no** se extraen (son ruido y no enrutan reglas).
- **Lenguajes soportados (Fase 8c)**: Rust (`lang_rust.rs`), Python (`lang_python.rs`), JavaScript y TypeScript (`lang_js.rs`, incluidos JSX y TSX) y HTML (`lang_html.rs`). Las llamadas se aíslan por familias de lenguajes (`Web`, `Python`, `Rust`) para que `callers` no mezcle llamadas homónimas de stacks distintos.
- **Lo diferido a la fase 8f**: la base de datos (tablas, consultas por función, modelos ORM) y las rutas HTTP quedan para la 8f. La 8c preserva los decoradores de Python para que la 8f no requiera cambiar la versión del formato de índice.
- **Sin dependencias**: escáneres léxicos escritos a mano en `cthg-core`, sin parsers pesados ni ASTs externos.
- **Almacenamiento**: `.cthg/indice.tsv` (formato TSV plano `cthg-indice 2`, con escritura atómica mediante PID temporal `.cthg/indice.tsv.<pid>.tmp`). Se ignora en Git mediante `.cthg/.gitignore`.

#### Fases del grafo de código (8a - 8f)
- **8a (completada)**: Escáner de Rust, `.cthg/indice.tsv`, comandos CLI `cthg index`, `cthg map`, `cthg where` y `cthg callers`.
- **8b (completada)**: Enrutado por vecinos e identificadores de código con eval (el canal espacial propaga afinidad hacia importaciones directas a 1 salto e identificadores del prompt; actualización del índice en `session-start` y primer turno de Antigravity con timeout de 500 ms).
- **8c (completada)**: Escáneres políglotas (Python, JS/TS, HTML), firmas en todos los lenguajes, decoradores de Python, formato v2 y filtros de bundle/ruido.
- **8d (completada)**: Canal pedido: `cthg map` reordenado (reglas arriba, en vecinos, importaciones y símbolos abajo; soporte para archivos no indexados y archivos nuevos con sus reglas de carpeta y hermanos), skill `project-memory` («Antes de escribir código»), sugerencia de mapa en CORE al iniciar sesión si hay índice y hooks silenciosos ante pánicos.
- **8e**: Visor 3D (`memviz`): visualización interactiva del grafo de código y la memoria causal.
- **8f**: Base de datos y rutas HTTP: tablas y consultas por función, modelos ORM, decoradores de ruta; diseñarla tras verla en un proyecto real.

*(Nota de ordenación: la fase 8 se implementa antes de la fase 7 para acelerar la orientación de los agentes).*

---

## 6. Aprender

Reglas de plasticidad, una por eje:

- **Y** es inmutable: todo se escribe después del presente.
- **X** es plástico: asociaciones hebbianas, olvido por sesión, rutas que siguen a los renombres.
- **Z** solo se mueve con evidencia.

Al aprender una frase (`cthg learn`):

1. Si describe un **plan** pendiente (similitud ≥ 0,55, ruta compatible) → el plan **se cumple**.
2. Si repite un recuerdo visible (similitud ≥ 0,5, ruta compatible, sin `--supersedes`):
   - afirma algo no anclado → α + 1 («refuerza»);
   - niega algo no anclado → β + 1 («debilita»);
   - niega algo anclado → se guarda la negación como nodo aparte que contradice al anclado (queda dudoso; el anclado no cambia);
   - afirma algo que el repo ya rechazó → β + 1 («rechazado»).
3. Si no, **nodo nuevo**, con aristas opcionales:
   - `--cause "<texto>"`: se busca una causa parecida (≥ 0,35) o se crea un `incidente` justo antes;
   - `--cause-id`, `--contradicts`, `--supersedes`.

Similitud entre una frase `a` y un recuerdo `b` (conjuntos de tokens):

```
sim(a, b) = ½ · |a∩b| / √(|a|·|b|)  +  ½ · |a∩b| / |a|
```

(mitad coseno, mitad cobertura: una frase corta puede coincidir con una descripción larga).

Rutas compatibles: una es antecesora de la otra, o alguna está vacía.

Solo el usuario y el repo suben χ de verdad. Lo que el agente deduce entra con prior neutro (`--origin agente`).

La skill [skill/SKILL.md](skill/SKILL.md) le dice al agente cuándo hacer cada cosa.

### 6.1 Aprender las correcciones sin que el agente se acuerde (`cthg hook stop`)

Hasta aquí todo aprendizaje dependía de que el agente ejecutara `cthg learn`, y en la práctica no lo hace (lo reconoce en [docs/declaracion-claude.md](docs/declaracion-claude.md)). Pero la corrección ya está escrita: el siguiente mensaje del usuario dice que el turno anterior falló. Al terminar cada turno, el hook `Stop` lee el transcript entero (Claude Code: `transcript_path`; Antigravity: `transcriptPath`) y hace seis cosas, sin modelos: solo listas fijas y reglas deterministas.

**1. Turnos.** Un turno es un mensaje del usuario y los archivos que el agente leyó o escribió hasta el siguiente (Claude Code: `Read`, `Edit`, `MultiEdit`, `Write`, `NotebookEdit`; Antigravity: `view_file`, `write_to_file`, `replace_file_content`). No cuenta lo que no escribió una persona: recordatorios del sistema, notificaciones, salidas de comandos, preguntas laterales, mensajes de subagentes, bloques pegados (`<pasted_content>`) ni un mismo mensaje reenviado dos veces seguidas. El formato del transcript de Claude Code es interno y cambia entre versiones, así que el lector ignora lo que no reconoce.

**2. ¿Corrige el turno anterior?** El primer mensaje de una conversación nunca. De los demás se toman solo **las palabras propias del usuario**:

- sin lo que va entre comillas («…», "…", “…”, `` `…` ``). Citar «arréglalo» al hablar de correcciones, o pegar el informe de otro agente entre comillas, no corrige nada;
- una comilla que no se cierra, o que no se cierra en 300 caracteres, es texto pegado y llega hasta el final. Sin este límite, las comillas de dentro de un bloque pegado lo cierran antes de tiempo y dejan fuera lo que citaba;
- sin lo que sigue a «resultado: / dice: / dijo: / dio: / respondió: / says: …» («antigravity da este resultado: …»).

De esas palabras se miran las 60 primeras, plegadas (minúsculas, sin tildes), de la señal más fuerte a la más débil:

| Señal | Ejemplos | Cuenta |
|---|---|---|
| frase de corrección | «no lo había pedido así», «no es a lo que me refería», «te dije que», «se te olvidó», «te faltó», «rompiste», «fallaste», «no hizo nada», «mismo error», «arréglalo», «hazlo de nuevo», «devuelve los cambios»; «not what I asked», «I told you», «you broke», «nothing changed» | siempre |
| persistencia | «sigue / siguen / aún / todavía / still» con una palabra de fallo en las 4 siguientes («sigue sin funcionar», «aún están rotos», «still giving 0.1»); «otra vez / de nuevo / nuevamente / again» con un fallo explícito delante («allí es donde caes nuevamente», «falla otra vez») o una palabra de fallo detrás; «¿por qué borraste…?» | siempre |
| apertura | «no, …», «nope», «casi, …», «mal, …» (pero no «no, gracias», «no te preocupes», «no sé», ni «no, también…», que añade algo al pedido) | si no es una pregunta |
| veredicto | «no funciona», «no veo…», «tampoco funciona», «no es legible», «está muy mal», «son malísimos», «tienes un desastre», «is broken» | si no es una pregunta y está en las 12 primeras palabras |

- Las preguntas solo cuentan con señal fuerte: «¿te falta mucho?» no es una corrección; «¿por qué borraste el test?» sí.
- «Todavía no lo he probado» es un paso pendiente del usuario, no un fallo del agente.
- Las 60 palabras existen porque un prompt largo que cita «arréglalo» o un texto pegado en inglés con «again» son tareas nuevas, no correcciones. Las señales débiles exigen además el principio: más abajo suele empezar un informe de algo nuevo («… y deberíamos revisar los que están mal»).

**3. Emparejar.** La corrección se junta con:

- **El pedido que corrige**: el último mensaje anterior que no era, a su vez, una corrección. «siguen rotas» después de «no, así no» corrige el pedido original.
- **Los archivos**: los que escribió el turno corregido; si no escribió nada, los que escribe el turno de la corrección (el arreglo); si tampoco, los que leyó. Sin rutas fuera del repo ni ruido (`target/`, lockfiles…). Con varios archivos, la regla va a la carpeta más profunda que los contiene a todos (herencia λ, sección 3.1). Si solo coinciden en la raíz, que tiene afinidad 0 y nunca se recuperaría por ruta, va al archivo más editado.

**4. Aprender**, como si se hubiera escrito `cthg learn "<corrección>" --path <ruta> --cause "<pedido>"`:

- **Texto**: las palabras del usuario (hasta 280 caracteres). Si tienen menos de 3 palabras con contenido («siguen rotas»), no se entienden solas: llevan el pedido, «siguen rotas (al pedir «arregla las ventanas»)».
- **Causa**: un `incidente` «Corrección del usuario al pedir «…»». Si el pedido ya va en el texto, la causa cita la corrección («Corrección del usuario: «siguen rotas»»), para no repetirlo. El incidente queda **sin ruta**: ya se ve en la línea de la regla (`← …`), y anclado al mismo archivo se inyectaría otra vez como línea propia.
- **Repetición**: si repite un recuerdo visible (similitud ≥ 0,5, ruta compatible), `learn` lo **refuerza** (α + 1) en vez de crear otro, como en el punto 2 de arriba. La misma queja en dos conversaciones refuerza la primera.
- **Prior (3; 1)** en vez del (4; 1) de `usuario`, y flag **`PROPOSED`** en la regla y en su incidente: es una propuesta (punto 6).
- **Solo se funde con propuestas.** Un `cthg learn` deliberado nunca refuerza una propuesta oculta: crea su propia regla, que se inyecta como siempre. Si no, decir una regla a propósito podría dejarla escondida.

**5. Una sola vez.** `.cthg/correcciones.txt` guarda la huella de cada corrección aprendida (FNV-1a de turno, pedido y corrección, más un recorte legible). Un segundo `Stop` sobre el mismo transcript, una sesión reanudada o una bifurcada que copia el historial no la aprenden otra vez. Si la memoria está ocupada por otro proceso, se deja para el turno siguiente. El hook nunca bloquea: en Claude Code no escribe nada y en Antigravity responde `{}`.

**6. Propuesta, no regla.** Una heurística se equivoca: en la primera versión, las 3 detecciones de la conversación de diseño de este repo eran falsas (sección 12.8). Por eso lo capturado no se inyecta hasta que alguien lo confirma:

| Canal | Una propuesta (`PROPOSED`) |
|---|---|
| 0 · núcleo | nunca |
| 1 + 3 · mensaje (`prompt`, `cthg recall`) | nunca, aunque nombre la ruta o las palabras del mensaje |
| 2 · lectura (`read`) | solo al leer uno de sus archivos: anclada en ese archivo o en una carpeta que lo contiene. Sale en la banda «[sin evidencia]» |
| propagación | no entra arrastrada por sus vecinos |

- **`cthg confirm <id>`** es lo único que quita `PROPOSED`. A partir de ahí es una regla del usuario más (α + 1 → 0,30).
- **`cthg deny <id>`** la debilita y la deja oculta.
- **`cthg list`** la marca `[propuesta]`.
- **La misma corrección otra vez** la refuerza (α + 1), pero sigue oculta.
- **El presupuesto sí la cuenta.** Las propuestas entran en `S` (sección 4.4): se inyectan al leer sus archivos, y en una memoria pequeña sin ellas no habría sitio en esa lectura.

Con números (sin causas estructurales, E = 0):

- **Corrección capturada**: α = 3, β = 1 → A = 0,5; e = 2 → w = 0,6 → χ = 0,6·0 + 0,4·0,5 = **0,20** (sin evidencia).
- **Tras un `confirm`, o la misma corrección otra vez**: α = 4, β = 1 → **0,30**, lo mismo que una regla dicha con `learn` (ejemplo 1 de la sección 3.4).
- **Tras un `deny`**: α = 3, β = 2 → A = 0,2; e = 3 → w = 0,5 → **0,10**. Con dos, 0.
- **Una corrección que repite una regla del usuario** (4; 1) la sube a (5; 1): A = 0,67; e = 4 → w = 3/7 → χ = 4/7·0,67 = **0,38**.

Al leer sus archivos, el factor de confianza del ranking (0,6 + 0,4·χ, sección 4.4) vale 0,68 con χ = 0,20 y 0,72 con 0,30: compite casi igual que una regla dicha, pero en la banda «[sin evidencia]». La consolidación pide 3 de evidencia sobre el prior de `usuario`, (4; 1), así que a una corrección capturada le hacen falta 4.

Salida real en un repo de prueba (el pedido fue «pon el botón de guardar en la barra de arriba» y el agente editó `src/ui/boton.rs` y `src/main.rs`; luego el usuario escribió «no, te dije que los mensajes del CLI en español, no en inglés» y, tras otro cambio en `src/main.rs`, «siguen rotas»):

```
$ cthg list
#0    χ +0.38 regla       usuario    src [] Los mensajes del CLI van en español
#1    χ +0.20 regla       usuario    src/main.rs [propuesta] siguen rotas (al pedir «pon el botón de guardar en la barra de a…
#2    χ +0.20 incidente   usuario    / [propuesta] Corrección del usuario: «siguen rotas»
```

La primera corrección repetía la regla #0 y la reforzó de 0,30 a 0,38. La segunda es nueva y queda como propuesta. `cthg recall --prompt "siguen rotas las ventanas" --path src/main.rs` solo trae la #0. Al leer `src/main.rs`, el hook inyecta:

```
## Memoria relevante (src/main.rs)
[probable]
- #0 regla (src): Los mensajes del CLI van en español
[sin evidencia]
- #1 regla (src/main.rs): siguen rotas (al pedir «pon el botón de guardar en la barra de arriba») ← Corrección del usuario: «siguen rotas»
```

Y tras confirmarla:

```
$ cthg confirm 1
#1 χ +0.20 → +0.30
   era una corrección capturada: desde ahora se inyecta como las demás
```

Límites:

- Las heurísticas no entienden. Una corrección sin ninguna de esas palabras («le falta un montón») se pierde, y también la que solo pega el error del programa que acaba de hacer el agente. Un informe de un bug nuevo que empieza con «está mal» puede tomarse por corrección. Por eso entra oculta hasta que se confirma.
- La regla guarda lo que dijo el usuario, no una lección redactada. Para una regla limpia sigue valiendo `cthg learn`, que crea su propia regla visible (punto 4).
- Hablar *sobre* correcciones solo se ignora si la frase va entre comillas. Sin comillas («el sistema evita lo de no lo había pedido así») sigue contando.
- Una corrección capturada que repite una regla ya **visible** la refuerza aunque nadie la haya confirmado. Es el único efecto inmediato de lo capturado: sube χ, no cambia qué se inyecta.

---

## 7. Tiempo y ramas

- Lo aprendido en el tronco (`meta trunk`, detectado en `init`) tiene `branch = ""` y se ve desde todas partes.
- Lo aprendido en otra rama lleva su nombre y solo se ve desde ella.
- **Fase 7**: una rama fusionada se vuelve visible desde el tronco cuando el commit en que se aprendió es ancestro del tronco (`git merge-base --is-ancestor`). Las ramas abandonadas se archivan con `gc`.
- **Obsolescencia**: ruta borrada, símbolo inexistente o regla sustituida → baja de χ. Nada se borra.

### 7.1 Verificar contra el repo (`cthg verify`)

Qué se comprueba, en orden (la primera comprobación que aplica decide):

1. **Ruta**: el `path` existe.
2. **Símbolo**: el `symbol` aparece como identificador en el archivo.
3. **Dependencia**: solo si el recuerdo está anclado en un manifiesto (`Cargo.toml`, `package.json`) o habla de dependencias («dependencia», «crate», «paquete», «package»…). Así «el motor usa f64» no se confunde con una dependencia ausente.
4. **Comando**: solo con `--run-commands`, y nunca si lleva sintaxis de shell (`&`, `|`, `;`, `>`, `$`, comillas…). Sin el flag se listan y no se ejecutan.

Solo los **cambios de estado** mueven un recuerdo, así que `verify` dos veces seguidas no cambia nada:

| Resultado | Estado antes | Después |
|---|---|---|
| comprobado | anclado | sigue anclado («verificado») |
| comprobado | sin ancla, y es `hecho`, `comando` u origen `repo` | `ANCHORED` («anclado») |
| comprobado o ruta existente | `STALE` | se quita `STALE` («restaurado») |
| falla | anclado | pierde `ANCHORED`, gana `STALE`; si su origen es `repo`, (α, β) = (1,5; 4,5) («pierde ancla») |
| falla | sin ancla | gana `STALE` («obsoleto») |
| falla | `STALE` | nada |

`STALE` es **estructural**, no evidencia: pone E = −1 mientras está y desaparece si el repo vuelve a coincidir, sin tocar α ni β. Por eso es reversible.

Reglas, decisiones y convenciones **nunca** se anclan: que exista `resolve_contacts` demuestra dónde aplica una regla, no que la regla sea correcta. Que exista `Cargo.toml` tampoco demuestra «la versión es la 0.1».

Ejemplos (los mismos números que da el CLI):

- Regla del usuario cuyo archivo se borra: χ = 0,30 → E = −1, w = 0,5 → χ = −0,5 + 0,3 = **−0,20**. Al restaurar el archivo vuelve a **0,30**.
- Hecho del repo cuya dependencia desaparece: (α, β) = (1,5; 4,5) → A = −0,5; e = 4 → w = 3/7 → χ = −3/7 − 4/7·0,5 = **−0,71** (dudoso).

Además:

- **Solo en el tronco** cambia anclas y obsolescencia; en otra rama, `verify` solo informa (un archivo borrado en `feature/x` sigue existiendo en `main`).
- Antes de comprobar rutas, sigue los renombres ya confirmados en commits (`git log -M --diff-filter=R`), para no llamar «borrado» a un archivo movido.
- `learn --origin repo` se comprueba en el acto. Si el repo lo confirma, queda anclado; si lo contradice, pierde el ancla; si no hay nada que comprobar, pasa a ser una afirmación del `usuario`. «Lo dice el repo» deja de ser algo que se pueda afirmar sin prueba.

### 7.2 Reimportar un archivo editado (`cthg import`)

Cuando el `CLAUDE.md` / `AGENTS.md` cambia, se vuelve a importar el mismo archivo: no hace falta borrar `.cthg/` ni se pierde lo aprendido con `learn`, `confirm` o `deny`. Cada bloque del archivo se casa con los nodos que **ese mismo archivo** importó antes (origen `importado` y `key` `<archivo>:L…`), en este orden:

1. **Sin cambios**: el texto guardado (con su contexto de sección) es idéntico. El nodo se queda como está; solo su `key` sigue a la línea en la que está ahora, porque las líneas se desplazan cuando se añade algo encima.
2. **Editado**: similitud ≥ 0,65 con un nodo del archivo que no se casó en el paso 1, o, si no hay ninguno, el nodo que ocupaba su sitio (entre los mismos bloques sin cambios, que equivale a «la misma `key`» aunque lo de encima se haya movido). Se crea un nodo **nuevo** con el texto nuevo y una arista `sustituye` nuevo → viejo. El texto viejo no se reescribe: Y solo avanza, y el viejo queda en el histórico, hundido. Si el viejo era `CORE`, el `CORE` pasa al nuevo.
3. **Repetido**: se parece (≥ 0,65) a otro recuerdo visible, por ejemplo uno aprendido con `learn` o otro bloque del mismo archivo. No se crea nada, como en la primera importación. Lo que una edición ya sustituyó no cuenta.
4. **Nuevo**: nodo nuevo.

Los nodos del archivo que no se casaron con ningún bloque son **reglas borradas**: se marcan `STALE`, como hace `verify`, y pierden `CORE` (una regla que el archivo ya no dice no se inyecta en cada sesión). Es reversible: si el bloque vuelve, se quita `STALE` («restaurado») y el `CORE` se vuelve a proponer.

Límites:

- Lo que escribió `learn` (u otro origen que no sea `importado`) **nunca** se sustituye ni se marca, aunque el archivo diga lo mismo o deje de decirlo.
- Como en `verify`, las marcas sobre nodos ya existentes (`STALE`, mover `CORE`) solo cambian **en el tronco**. En otra rama, una edición crea su nodo sustituto en esa rama (el tronco no lo ve) y lo borrado solo se lista («ya no está»).
- Un nodo anclado no lo mueve `import`: si se borra, solo se lista; si se edita, el sustituto se crea, pero el anclado sigue en χ ≥ 0,9 hasta que `verify` le quite el ancla.
- Un `STALE` que puso `verify` porque falta su ruta no lo quita `import`. Si lo puso por otra comprobación (símbolo, dependencia) y el bloque sigue en el archivo, `import` lo quita y el siguiente `verify` lo vuelve a poner.
- La evidencia del nodo viejo (`confirm`) no pasa al nuevo, igual que con `learn --supersedes`: se confirmó otra redacción.

Importar dos veces el mismo archivo no cambia nada: todo cae en el paso 1, lo sustituido ya no cuenta y lo obsoleto ya estaba marcado.

Ejemplo con números (prior de `importado` (5; 1,5) → A = 0,54; e = 4,5 → w = 3/7,5 = 0,4):

- Un bloque sin cambios o recién importado: χ = 0,4·0 + 0,6·0,54 = **0,32**.
- El bloque viejo de una edición: lo sustituye un nodo visible con χ = 0,32 > 0, así que E = −1 → χ = −0,4 + 0,32 = **−0,08** (sin evidencia, ya sin peso frente al nuevo).
- Un bloque borrado: `STALE` → E = −1 → también **−0,08**. Si vuelve al archivo, otra vez **0,32**.

---

## 8. Estructura del repositorio

```
3D-Memory/
├─ Cargo.toml                 workspace
├─ GUIA.md                    este documento
├─ README.md
├─ crates/
│  ├─ cthg-core/              motor, sin dependencias
│  │  └─ src/
│  │     ├─ model.rs          nodos, aristas, hiperaristas, flags, orígenes y priors
│  │     ├─ score.rs          vista, visibilidad, χ (E, A, mezcla, bandas)
│  │     ├─ spatial.rs        eje X: normalizar, antecesores, afinidad, renombres
│  │     ├─ retrieve.rs       canales, semillas, propagación, ranking, presupuesto
│  │     ├─ learn.rs          aprender, planes, consolidación, feedback, olvido, hebb
│  │     ├─ import.rs         importar Markdown, deducir rutas, proponer CORE y seguir las ediciones
│  │     ├─ hook.rs           hooks de Claude Code, deduplicación por sesión
│  │     ├─ correction.rs     correcciones del usuario: leerlas del transcript, aprenderlas, contarlas
│  │     ├─ antigravity.rs    hooks de Google Antigravity: transcript y mensaje efímero
│  │     ├─ json.rs           lector JSON mínimo (sin dependencias)
│  │     ├─ verify.rs         verificar rutas, símbolos, dependencias y comandos contra el repo
│  │     ├─ eval.rs           medición: casos, 6 configuraciones, métricas, diagnóstico de fallos
│  │     ├─ index.rs          índice derivado del código: construcción, persistencia v2 y consultas
│  │     ├─ lang.rs           despacho multilingüe, familias de lenguajes y tipos compartidos
│  │     ├─ lang_rust.rs      escáner de Rust y resolución de módulos/crates
│  │     ├─ lang_python.rs    escáner de Python, decoradores, `__all__` e importaciones
│  │     ├─ lang_js.rs        escáner de JS/TS (JSX/TSX), clases, métodos e importaciones
│  │     ├─ lang_html.rs      escáner de HTML: scripts, eventos `onclick` e IDs
│  │     ├─ text.rs           tokens es/en, similitud, recortes, tokens aproximados
│  │     └─ binary.rs         formato .cthg (CRC32, guardado atómico)
│  └─ cthg-cli/               binario `cthg`
│     └─ src/main.rs
├─ eval/ejemplo/              CLAUDE.md sintético, rutas.txt, casos.md y resultado.md
├─ hooks/                     settings.example.json + README
├─ skill/SKILL.md             cuándo escribir en la memoria
└─ docs/formato-cthg.md       especificación byte a byte
```

### 8.1 Memoria global

Las reglas que valen para **todos** los proyectos del usuario viven en una memoria aparte: `~/.cthg/global.cthg` (o la ruta de `CTHG_GLOBAL`). Se edita con `--global` en `init`, `learn`, `confirm`, `deny`, `core`, `list` e `import`; no tiene rutas ni ramas, y `recall`, `verify`, `mv` y `eval` no aplican.

En `session-start` va primero el `CORE` global, con su propio tope de 600 tokens, y después el del proyecto, con el suyo (sección 4.4): ya no compiten. Una regla `CORE` del proyecto que repite una global (similitud ≥ 0,5) no se inyecta dos veces. Los ids globales se escriben `#g3`. `import` propone el `CORE` del proyecto dentro del presupuesto que tendrá la memoria al terminar de importar, y `core`/`list` avisan si no cabe.

Un proyecto se reconoce por `.cthg/memoria.cthg`, no por la carpeta `.cthg/` sola: `~/.cthg/` es antecesora de todos los proyectos y nunca debe tomarse por uno.

En cada proyecto que use la memoria: `<repo>/.cthg/memoria.cthg` (+ `.bak`), `<repo>/.cthg/sesion/<session_id>.json` para no repetir lo ya inyectado y `<repo>/.cthg/lock` mientras alguien la modifica. Si se versiona `.cthg/` o no se decide por proyecto (sección 14).

---

## 9. CLI `cthg`

| Comando | Estado | Qué hace |
|---|---|---|
| `cthg init` | ✅ | crea `.cthg/memoria.cthg` en la raíz git; detecta el tronco |
| `cthg learn <texto> [--kind K] [--path P] [--symbol S] [--cause T] [--cause-id N] [--contradicts N] [--supersedes N] [--origin O] [--core] [--plan] [--deny]` | ✅ | sección 6 |
| `cthg confirm N` / `cthg deny N` | ✅ | α + 1 / β + 1 (lo anclado no se mueve); `confirm` es lo único que convierte una corrección capturada (`[propuesta]`) en regla inyectable |
| `cthg core [<id>… [--unset]] [--budget N]` | ✅ | canal 0 tal como se inyecta (global y proyecto); con ids, marca o desmarca `CORE`. Avisa si el núcleo no cabe en el presupuesto |
| `cthg <init\|learn\|confirm\|deny\|core\|list\|import> --global` | ✅ | lo mismo sobre la memoria global (sección 8.1) |
| `cthg recall [--prompt T] [--path P]… [--git] [--budget N] [--limit N] [--sin-vecinos]` | ✅ | canales 1–3 (rutas, símbolos de `git diff`, renombres `-M`, vecinos del grafo de código e identificadores) |
| `cthg index [--rebuild]` | ✅ | actualiza o reconstruye `.cthg/indice.tsv` (formato v2); omite ruido y bundles |
| `cthg map [ruta] [--limit N] [--tests]` | ✅ | mapa estructurado de un archivo (reglas y vecinos arriba, dependencias y símbolos abajo con firmas truncadas); distingue archivo indexado, no indexado (con sus reglas) y archivo nuevo (reglas de carpetas y hasta 5 hermanos con su API pública) |
| `cthg where <símbolo>` | ✅ | busca declaraciones de un símbolo mostrando su ruta, línea, tipo y firma completa `{ruta}:{linea} {tipo} {nombre}{firma}` |
| `cthg callers <símbolo> [--tests]` | ✅ | busca quién llama a un símbolo en el grafo de código, respetando aislamiento entre familias de lenguajes (`Web`, `Python`, `Rust`) |
| `cthg list [--all]` | ✅ | todos los nodos con χ, flags (incluido `obsoleto`) y rama |
| `cthg mv <de> <a>` | ✅ | la memoria sigue a un archivo o carpeta movidos |
| `cthg import <archivo.md> [--yes]` | ✅ | parte el archivo en nodos y propone un `CORE` ordenado que cabe en el presupuesto de núcleo (secciones 4.4 y 12.6); cada nodo guarda su origen en `key` (`CLAUDE.md:L12`) y su contexto (sección 12.5). Reimportar el archivo editado sustituye lo cambiado, marca `STALE` lo borrado y lista qué es nuevo, actualizado, obsoleto y restaurado (sección 7.2) |
| `cthg hook <session-start\|prompt\|read\|edit\|stop>`, `cthg hook antigravity`, `cthg hook antigravity-stop` | ✅ | lee el JSON del hook por stdin, deduplica por sesión en `.cthg/sesion/<session_id>.json`; `stop` y `antigravity-stop` aprenden las correcciones del usuario (sección 6.1) |
| `cthg verify [--run-commands]` | ✅ | sección 7.1 |
| `cthg eval <carpeta> [--markdown] [--repo DIR] [--file CLAUDE.md] [--core-auto]` | ✅ | sección 12; `--core-auto` ignora los `core:` y usa la propuesta de `import` |
| `cthg corrections [<transcript.jsonl\|carpeta>…] [--desde AAAA-MM-DD] [--detalle]` | ✅ | correcciones del usuario por conversación, con y sin memoria (sección 12.8); sin rutas, los transcripts de Antigravity |
| `cthg gc`, `cthg view` | fase 7 | archivar lo muerto; visor 3D con X = árbol de carpetas |

---

## 10. Hooks de Claude Code

| Evento | Comando | Canal |
|---|---|---|
| `SessionStart` | `cthg hook session-start` | 0 (global y proyecto), y un paso de olvido |
| `UserPromptSubmit` | `cthg hook prompt` | 1 + 3 |
| `PostToolUse` (`Read`) | `cthg hook read` | 2 (`hookSpecificOutput.additionalContext`) |
| `PostToolUse` (`Edit\|MultiEdit\|Write`) | `cthg hook edit` | 2 (`hookSpecificOutput.additionalContext`): reglas de ese archivo y sus carpetas (si no se inyectaron antes), y sube la activación |
| `Stop` | `cthg hook stop` | aprende las correcciones del usuario del transcript (sección 6.1); no escribe nada y nunca impide terminar |

Plantilla y reglas en [hooks/](hooks/README.md). En resumen: nunca bloquear (si falla, código 0 y sin salida), no repetir dentro de una sesión y respetar los presupuestos dinámicos (sección 4.4). Si la memoria está bloqueada por otro proceso más de 1,5 s, el hook responde igual pero no guarda. Si el núcleo no cabe en su presupuesto, `session-start` lo dice en el propio contexto, en vez de recortarlo en silencio. Al iniciar sesión, si el proyecto tiene índice de código, el bloque `CORE` incluye un recordatorio para pedir mapas con `cthg map` y buscar símbolos con `where`/`callers` en vez de usar grep; además, los hooks apagan stderr en caso de pánico para no romper los protocolos JSON. Se instalan en el `.claude/settings.json` **del proyecto**, no en el global. Llaman a `cthg` por su nombre: se instala en el PATH con `cargo install --path crates/cthg-cli --locked` (repetir tras cada cambio). Para ver la memoria, la app Memoria 3D (`memviz`, en `Memory-Handler/3D-Memory-Visualizer`) abre cualquier repo con memoria.

Los `CLAUDE.md` anidados por carpeta que Claude Code ya carga son la **línea base** contra la que medir.

### 10.1 Google Antigravity

Mismo motor, otro contrato de hooks. En Antigravity solo `PreInvocation` puede añadir contexto, y lo hace como `ephemeralMessage`. No recibe el prompt, pero sí la ruta del transcript. Lo inyectado **queda en la conversación**: el transcript lo guarda como pasos `EPHEMERAL_MESSAGE`, y el modelo lo sigue viendo en el turno siguiente. `cthg hook antigravity` (en `.agents/hooks.json`) lee las líneas nuevas del transcript en cada llamada:

| Paso nuevo en el transcript | Equivale al canal | Qué inyecta |
|---|---|---|
| primera llamada de la conversación | 0 | núcleo (global y proyecto, o memoria entera si `S ≤ WHOLE_MAX`), una sola vez |
| `USER_INPUT` o `SYSTEM_MESSAGE` de agente (turno nuevo) | 1 + 3 | lo que recuerdan el mensaje y `git status`, sin repetir nada ya inyectado |
| `view_file` | 2 | las reglas de ese archivo, una vez por turno |
| `write_to_file` / `replace_file_content` | 2 | las reglas de ese archivo (si no se inyectaron antes) y sube la activación |

La posición en el transcript y el turno se guardan en `.cthg/sesion/<conversationId>.json`. El olvido corre una vez por conversación.

Al terminar cada turno, el evento `Stop` llama a `cthg hook antigravity-stop`, que aprende las correcciones del usuario igual que en Claude Code (sección 6.1). Responde siempre `{}`: sin `decision: "continue"`, el agente se detiene como siempre.

**Archivos que el agente ya carga.** Un recuerdo importado de un archivo de reglas que el agente lee por su cuenta no se vuelve a inyectar. Se reconoce por su `key`: `AGENTS.md:L12` o `GEMINI.md:L3` en Antigravity, `CLAUDE.md:L5` en Claude Code. Así no llega dos veces. Para que un proyecto ahorre tokens de verdad, conviene que ese archivo sea corto y que el resto viva en la memoria. Detalles en [hooks/README.md](hooks/README.md#google-antigravity).

---

## 11. Formato binario

Un solo archivo (versión 3; lee también la 1 y la 2; se escribe como 2 si ningún nodo es una propuesta, para que los lectores de la 2 sigan abriéndolo) con secciones `META`, `NODE`, `EDGE`, `HYPE` y CRC32 final; las secciones desconocidas se saltan.

Escritura segura: toda modificación se hace con `.cthg/lock`; cada escritor usa su propio `.tmp` y renombra (el anterior queda en `.bak`); si el archivo está corrupto, se carga el `.bak`. Un candado de más de 30 s se considera abandonado. χ no se guarda: se calcula al cargar para la vista activa. Especificación completa: [docs/formato-cthg.md](docs/formato-cthg.md).

Coste: con cientos o pocos miles de nodos, una puntuación completa es O(N log N + E) y tarda milisegundos. Lo caro son los tokens inyectados, no la CPU.

---

## 12. Medición (`cthg eval`)

### 12.1 Qué se mide

Para cada caso se importa el `CLAUDE.md` en una memoria **nueva y en RAM**, con reloj fijo, sin tocar el `.cthg/` del proyecto ni llamar a git. Así el resultado es determinista. Seis configuraciones, todas con los presupuestos reales (los dinámicos de la sección 4.4):

| Configuración | Qué inyecta |
|---|---|
| `completo` | el archivo entero (recall 100 % por definición) |
| `anidado ideal` | reglas sin ruta + reglas cuya ruta es antecesora de lo tocado o leído, sin presupuesto: los `CLAUDE.md` por carpeta hechos a la perfección |
| `solo léxico` | canal 0 + canal 3 |
| `espacial` | canal 0 + canal 1 |
| `espacial + lectura` | canal 0 + canal 1 + canal 2 |
| `todo` | canales 0–3 en el orden de los hooks (session-start → prompt → un read por archivo), con deduplicación de sesión |
| `todo + vecinos` | canales 0–3 enriquecidos con vecinos de importación e identificadores del grafo de código |

Métricas por caso y configuración: tokens inyectados (`approx_tokens` del texto renderizado), recall = relevantes inyectadas / relevantes, precisión = relevantes inyectadas / inyectadas, y críticas perdidas.

Cada regla relevante **no** inyectada recibe una sola causa, la primera que aplica:

1. `no importada`: el parser saltó ese bloque (o la línea no es la primera del bloque).
2. `fusionada al importar (con Ln)`: import la juntó con otra regla parecida. Cuenta como fallo aunque la otra sí se inyecte, porque una fusión puede perder contenido (sección 14).
3. `χ bajo`: invisible (un plan) o dudosa / descartada.
4. `fuera de presupuesto`: sin presupuesto sí habría salido.
5. `sin ruta y no CORE`: ningún canal puede encontrarla salvo por palabras. Solución: `CORE` o darle ruta.
6. `ruta no coincide`: está anclada fuera de lo que se tocó o leyó.
7. `sin coincidencia léxica`: no había rutas con las que comparar y las palabras no coinciden.

### 12.2 Formato

Una carpeta por repo evaluado, con el `CLAUDE.md` (o `AGENTS.md`), `casos.md` y, si el repo no está a mano, `rutas.txt` (una ruta existente por línea). Si no hay `rutas.txt`, se usa `--repo DIR` o el repo actual.

```text
core: L7, L8                         (CORE confirmado; si falta, se usan las propuestas de import)

## caso: arreglar el colisionador
prompt: arréglalo
git: src/core/collider.rs            (modificados: canal 1)
lee: src/core/collider.rs            (leídos: canal 2)
relevantes: L7!, L15, L16!           (líneas del CLAUDE.md; ! = crítica; vacío = sin etiquetar)
core: L7                             (opcional: otro CORE para este caso)
```

Las líneas se refieren al archivo gracias a `node.key = "<archivo>:L<línea>"`, que `import` rellena con la primera línea de cada bloque (y actualiza si el bloque se desplaza al reimportar, sección 7.2).

Las etiquetas de un repo real las pone una persona. Un caso con `relevantes:` vacío se lista como «sin etiquetar» y no cuenta.

### 12.3 Resultado del ejemplo sintético

`cthg eval eval/ejemplo`: 25 bloques, 5 reglas `CORE`, 10 casos medidos (completo en [eval/ejemplo/resultado.md](eval/ejemplo/resultado.md)).

| Configuración | Tokens medios | Ahorro | Recall | Precisión | Críticas perdidas |
|---|---:|---:|---:|---:|---:|
| completo | 491 | 0 % | 100 % | 17 % | 0 |
| anidado ideal | 183 | 63 % | 97 % | 42 % | 1 |
| solo léxico | 136 | 72 % | 74 % | 50 % | 4 |
| espacial | 124 | 75 % | 73 % | 54 % | 3 |
| espacial + lectura | 152 | 69 % | 93 % | 59 % | 1 |
| todo | 171 | 65 % | 93 % | 56 % | 1 |

Cómo leerlo:

- **El canal justo a tiempo es el que más aporta**: el recall pasa de 73 % (`espacial`) a 93 % (`+ lectura`). Sin él, todo caso cuyo archivo no está modificado todavía («añade un mensaje al protocolo») se pierde.
- **El léxico solo, no basta** (74 %): «arréglalo» no nombra nada. Lo que sí aporta en `todo` son las preguntas sin archivo.
- **No cumple el criterio** en este ejemplo, por dos motivos concretos:
  - Todos los fallos de `todo` están en «desplegar»: «despliego» no coincide con «despliegue» (el stemming es demasiado ligero) y «avisa en el canal de operaciones» no tiene ruta ni es `CORE`. Son exactamente los huecos de la sección 5, ahora medidos.
  - El ahorro (65 %) está limitado por el tamaño: con un `CLAUDE.md` de 491 tokens, el núcleo fijo (~100 tokens) pesa mucho. En un archivo real de miles de tokens el coste del núcleo es el mismo y el ahorro crece; hay que confirmarlo con repos reales.
- **`anidado ideal` es la línea base a batir**: aquí iguala a `todo` en recall con menos tokens, porque es perfecto por construcción (sabe de antemano qué archivos se tocarán). Lo que 3D-Memory añade sobre él son χ, causas, tiempo y ramas, no recall puro.

### 12.5 Primera prueba real: el `AGENTS.md` de este repo

`eval/3d-memory/` (conjunto de **ajuste**) y `eval/3d-memory-prueba/` (conjunto de **prueba**, escrito antes de tocar el código y no usado para decidir nada). Ambos usan el mismo `AGENTS.md` congelado (36 bloques, 1265 tokens) y el mismo `CORE` de 7 reglas, con 10 casos cada uno sacados de tareas reales de las fases 2–6. Las etiquetas las puso el agente que hizo el trabajo: es una prueba inicial, no una medición independiente.

Importar un archivo real destapó cuatro problemas de `import`, y se corrigieron:

- **A · contexto de sección**: los encabezados `##`/`###` se descartaban, así que «Importar dos veces no duplica nodos» perdía que es de la fase 2 (`cthg import`). Ahora el nodo guarda `[Sección › Introducción] texto`, y eso cuenta para el canal léxico.
- **B · nombres sueltos**: `binary.rs` no se reconocía como `crates/cthg-core/src/binary.rs`. Ahora un nombre de archivo cuenta si hay exactamente un archivo con ese nombre (`git ls-files`).
- **C · mencionar no es aplicar**: «actualiza GUIA.md» anclaba la regla a `GUIA.md`, y «Lee primero GUIA.md» también. Un documento (`.md`, `docs/…`) solo ancla si el bloque trata de él («En `README.md` …»); si no, gana la ruta de código o la regla queda sin ruta.
- **D · introducciones**: «pasa las dos comprobaciones» quedaba huérfana de «Al terminar una fase:». Las sub-viñetas heredan la línea que las introduce (por sangría). También se corrigió que «encargo» contuviera «cargo» y se clasificara como comando.

`todo`, antes → después:

| Conjunto | Tokens | Ahorro | Recall | Críticas perdidas |
|---|---|---|---|---|
| ajuste | 432 → 536 | 66 % → 58 % | 73 % → 78 % | 2 → 1 |
| prueba | 468 → 513 | 63 % → 59 % | 77 % → 79 % | 1 → 1 |
| ejemplo sintético | 171 → 171 | 65 % → 65 % | 93 % → 93 % | 1 → 1 |

Decisiones tomadas **solo con el conjunto de ajuste**:

- Con el prefijo de sección en cada línea inyectada, `todo` subía a 634 tokens. Ahora el prefijo se usa para buscar, pero al inyectar solo se muestra en fragmentos de menos de 40 caracteres, que sin él no se entienden. `todo` queda en 536.
- Sin encabezados (solo B + C + D): 441 tokens, 75 % y 2 críticas perdidas. Con encabezados: 536, 78 % y 1. Se quedan, porque el criterio pone las críticas por delante. Cuestan ~20 % más tokens.

Qué enseña:

- **En el conjunto de prueba la mejora es pequeña** (+2 puntos de recall, +10 % de tokens). Los arreglos de `import` eran necesarios, pero no son la palanca principal.
- **Todos los fallos que quedan son `sin ruta y no CORE`**: «resume antes de escribir código», «imita el estilo», «no reescribas módulos enteros», «las matemáticas están en la guía». Un `AGENTS.md` de *proceso* apenas nombra rutas, así que el canal espacial no tiene dónde agarrarse. Lo que decide es **qué reglas son `CORE`**, y eso lo elige una persona (sección 14).
- **`anidado ideal` pasa a 100 % de recall, pero ahorra solo un 12 %**: con C, las reglas que solo mencionaban `GUIA.md` quedan sin ruta, y en un esquema por carpetas acaban todas en el archivo raíz. Para este tipo de documento, los `CLAUDE.md` anidados no ahorran casi nada.

### 12.6 Elegir el `CORE` automáticamente

Antes, `import` proponía como `CORE` toda regla sin ruta con una palabra normativa. En este repo eso eran 18 reglas que no cabían en 500 tokens, y `session-start` las recortaba por orden de aparición: 707 tokens, 71 % y **5 críticas perdidas** (conjunto de ajuste).

Ahora la propuesta es una **selección dentro del presupuesto**:

```
candidata  = sin ruta, no es un plan, su sección no trata de un comando o módulo
             (encabezado sin `código`) y vincula algo
puntuación = vínculo (prohibición 2 · obligación 1,5 · otra norma 1) + 1 si el autor la puso en negrita
coste      = approx_tokens(texto) + 5
selección  = por puntuación, luego la más barata, luego orden del documento,
             mientras quepa en el presupuesto de núcleo − 12 tokens (cabecera) − lo que ya es CORE
```

- Las reglas de una sección sobre un comando («Fase 4 · `cthg hook`») no son candidatas: las encuentra el canal léxico cuando la tarea nombra ese comando.
- «tiene que», «hay que», «must» y «have to» cuentan como obligación.
- Por construcción, `--yes` nunca desborda el núcleo.

Decidido con el conjunto de ajuste (la primera versión restaba 1,5 a esas secciones en vez de excluirlas, y dejaba fuera «tienen que»: 703 tokens, 73 % y 7 críticas perdidas). Comprobado una vez en el de prueba:

| `todo` | CORE humano (7 reglas) | CORE automático (12 reglas) |
|---|---|---|
| ajuste | 536 tokens · 78 % · 1 crítica perdida | 696 tokens · 87 % · **0** |
| prueba | 513 tokens · 79 % · 1 crítica perdida | 662 tokens · 85 % · **0** |
| ejemplo sintético | 171 tokens · 93 % · 1 | 188 tokens · 97 % · 1 (la de «despliego», sección 14) |

- **El `CORE` automático gana al humano en lo que más importa**: cero críticas perdidas en los dos conjuntos reales y +6 a +9 puntos de recall.
- **Lo paga en tokens** (+30 %). Con un `AGENTS.md` corto (1265 tokens) y de proceso, el núcleo es casi la mitad de lo que se inyecta, y el ahorro baja al 45–48 %. El criterio de ≥ 70 % de ahorro no era alcanzable con un núcleo fijo de 500 tokens en un archivo de este tamaño. Ahora el núcleo escala con el proyecto (sección 4.4): en archivos pequeños entra casi todo, y en archivos de varios miles de tokens se estabiliza, así que el ahorro crece. Esto hay que medirlo con un archivo largo.
- **Los fallos que quedan** son reglas de proceso sin palabra normativa («Cada pieza nueva lleva tests…», «Imita el estilo existente…»): ni ruta ni norma explícita.

### 12.7 Observación de campo: Antigravity con Gemini (anecdótica)

Datos del usuario usando la memoria de verdad en otros repos, no medidos con `cthg eval`:

- **Benchmark de Antigravity en Clay-Myrrh** (tres `.md` que suman ~12,8 k tokens): con `cthg recall`, cada propiedad costó entre 210 y 370 tokens si se conoce el archivo, y unos 780 buscando solo por texto. Leer los `.md` cuesta 3,4–8,9 k, y buscar a mano 0,9–1,5 k más 2–3 rondas de herramientas. La memoria además devolvió la decisión vigente (`PING_GRACE` = 180 s) con la antigua (30 s) marcada como sustituida, mientras el `CLAUDE.md` seguía diciendo 30 s.
- **Uso diario con Gemini Flash (medium):** con prompts poco específicos, en el turno 6 la memoria costaba 5,5 k tokens de entrada frente a los 3,3 k del `.md`. Aun así, el agente pensó menos, tardó menos y no cometió errores en la tarea que más contexto consume (manejo directo de información). Gastó menos del 10 % de la cuota de 5 horas.
- **Coste de operación:** afinar este proyecto consumió la mayor parte de la cuota semanal de Gemini 4 Argon. En cambio, generar nodos para 3 proyectos, actualizar repos y reescribir el `.md` de la página de GitHub del usuario costó menos del 5 %.

Lectura: más entrada barata a cambio de menos pensamiento y menos correcciones parece compensar. Pero son ejecuciones sueltas, sin la misma tarea repetida con y sin memoria, y el porcentaje de cuota mezcla entrada y salida. La acumulación por conversación que se ve en el turno 6 es la que corrigen el tope por conversación y el factor de especificidad (sección 4.4).

### 12.8 Correcciones por conversación (`cthg corrections`)

El objetivo de fondo es que haya menos turnos de corrección («arréglalo», «yo no lo había pedido así»): que la memoria complete lo que el prompt no dice. `cthg corrections` lo mide sobre transcripts reales con el mismo detector que el hook (sección 6.1):

- **Entrada**: por defecto, `~/.gemini/antigravity*/brain/*/.system_generated/logs/transcript.jsonl`. También acepta archivos o carpetas, por ejemplo una de `~/.claude/projects/` (sin los transcripts de subagentes).
- **Con memoria**: un hook inyectó algo en esa conversación, es decir, hay un `attachment` de Claude Code o un `EPHEMERAL_MESSAGE` de Antigravity con «## Memoria relevante», «## Reglas del proyecto» o «## Reglas globales». Si esos encabezados solo aparecen en la salida de una herramienta (un `cthg recall` a mano, un archivo leído), la conversación no cuenta como «con memoria».
- **Métricas por grupo**: correcciones por conversación y % de respuestas corregidas. El % es correcciones / mensajes que no son el primero de su conversación.
- `--desde AAAA-MM-DD` añade el corte antes / después de una fecha (la de instalar los hooks, por ejemplo). `--detalle` lista cada corrección con la señal que la detectó, para revisar falsos positivos.

Primera pasada sobre los transcripts de Antigravity del usuario (17-10-2026, `cthg corrections --desde 2026-10-16`):

| Grupo | Conversaciones | Mensajes | Correcciones | Por conversación | % de respuestas corregidas |
|---|---:|---:|---:|---:|---:|
| con memoria | 1 | 5 | 0 | 0,00 | 0 % |
| sin memoria | 79 | 562 | 95 | 1,20 | 20 % |
| antes del 2026-10-16 | 67 | 516 | 92 | 1,37 | 20 % |
| desde el 2026-10-16 | 13 | 51 | 3 | 0,23 | 8 % |
| total | 80 | 567 | 95 | 1,19 | 20 % |

Cómo leerlo:

- **Es la línea base, no el resultado.** Solo una conversación tuvo el hook inyectando. Las demás usaban `cthg recall` a mano o son anteriores. La comparación de verdad llega con semanas de conversaciones con los hooks instalados.
- **El corte por fecha no demuestra causa.** Desde el 16-10 casi todo son conversaciones cortas sobre la propia memoria, no tareas largas de código, que es donde salen las correcciones.
- **Precisión revisada a mano**: unas 85 de las 95 se quejan del resultado del turno anterior («sigue roto», «no hizo nada», «te dije que…»). El resto son informes de bugs del mismo programa que pueden ser previos. La primera versión contaba 104: los falsos positivos encontrados (prompts largos que citaban «arréglalo», un «again» en texto pegado, reenvíos, un «…no los tiles, de nuevo funcionará…») llevaron a la ventana de 60 palabras, a ignorar los reenvíos y a exigir un fallo explícito delante de «de nuevo». Los arreglos posteriores (comillas, citas, «no, también», «nuevamente», «desastre») no cambian ninguna de las 95.

Revisión por proyecto, a mano (18-10-2026):

| Conjunto | Mensajes | Detecciones | Correcciones reales | Correcciones perdidas |
|---|---:|---:|---:|---|
| conversación de diseño de este repo (Claude Code), primera versión | 46 | 3 | **0** | 2 |
| la misma, versión actual | 46 | 2 | **2** | 0 |
| Clay-Myrrh (4 conversaciones de Antigravity, sin tocar el detector) | 12 | 1 | **1** | 1 |

- **Diseño, primera versión**: las 3 detecciones eran falsos positivos. «no, también está este manga» añade algo al pedido. «…los prompts innecesarios de "arréglalo" "yo no lo había pedido así"…» cita ejemplos. «pregunta esto, opiniones? "La memoria ya aprende sola…"» cita un informe. Y se le escaparon las dos correcciones reales: «tienes un desastre en la estructura… eso no es legible… borra el gráfico» y «allí es donde caes nuevamente».
- **Diseño, versión actual**: detecta esas dos y ninguno de los tres falsos positivos. Un arreglo intermedio dejó escapar otro: un bloque pegado entre comillas con comillas dentro. De ahí sale el límite de 300 caracteres. **Esta conversación es ya conjunto de ajuste**: los arreglos se hicieron mirándola, así que su 2 de 2 no demuestra nada (sección 12.4).
- **Clay-Myrrh**: la única detección, «se te olvidó algo sutil… no añadiste el gitignore ni los workflows», es una corrección real. Se pierde una: un mensaje que solo pega el error del programa recién hecho («Error durante la ejecución: No Windows console found…»). La muestra es muy pequeña (12 mensajes) para dar una tasa.
- **Claude Code en conjunto**: este repo no tiene todavía un grupo sin memoria con el que comparar.

### 12.9 Medición de la Fase 8b (Enrutado por grafo de código)

En la Fase 8b se añade la configuración `todo + vecinos` a `cthg eval`, que evalúa los canales 0–3 enriquecidos con la expansión a vecinos de importación directos a 1 salto ($W_{\text{vecino}}$) e identificadores exactos del mensaje ($W_{\text{símbolo}}$).

#### Resultados comparativos (con $W_{\text{vecino}} = 0,5$ y $W_{\text{símbolo}} = 0,9$)

| Conjunto | `todo` (sin vecinos) | `todo + vecinos` (8b) | Variación |
|---|---|---|---|
| **Ajuste** (`eval/3d-memory`, 17 casos) | 472 tok · 79 % recall · 4 críticas perdidas | 460 tok · 83 % recall · **2 críticas perdidas** | −12 tok · +4 % recall · **−2 críticas perdidas** |
| **Prueba** (`eval/3d-memory-prueba`, 17 casos) | 435 tok · 81 % recall · 3 críticas perdidas | 444 tok · 85 % recall · **1 crítica perdida** | +9 tok · +4 % recall · **−2 críticas perdidas** |
| **Ejemplo sintético** (`eval/ejemplo`, 10 casos sin índice) | 167 tok · 93 % recall · 1 crítica perdida | 167 tok · 93 % recall · 1 crítica perdida | Idéntico (0 regresiones) |

#### Desglose de canales (críticas perdidas)

| Conjunto | Base (`todo`) | Solo vecinos (`--w-simbolo 0`) | Solo identificadores (`--w-vecino 0`) | Ambos (`todo + vecinos`) |
|---|---:|---:|---:|---:|
| **Ajuste** (`eval/3d-memory`, 17 casos) | 4 | 2 | 3 | 2 |
| **Prueba** (`eval/3d-memory-prueba`, 17 casos) | 3 | 1 | 1 | 1 |

#### Aportación de cada canal y calibración de pesos

- **La mejora en prueba no viene solo de identificadores**: cada canal por sí solo recupera los dos casos (`decode` y `crc32` llegan a `binary.rs` por identificador y también por importación al estar referenciados en los archivos involucrados).
- **En ajuste, los vecinos aportan más**: el canal de vecinos por sí solo reduce 2 críticas perdidas (de 4 a 2), mientras que el de identificadores por sí solo reduce 1 (de 4 a 3). Ambos combinados ofrecen la máxima cobertura manteniendo 2 críticas perdidas.
- **Invarianza en la cuadrícula de pesos**: al barrer $W_{\text{vecino}} \in [0,1, 0,7]$ y $W_{\text{símbolo}} \in [0,1, 0,9]$ con las opciones `--w-vecino` y `--w-simbolo` añadidas a `cthg eval`, las métricas globales de `eval/3d-memory` con ambos canales activos se mantienen constantes (tokens = 460, recall = 83 %, críticas = 2). Al no haber saturación presupuestaria entre reglas del mismo archivo, estos conjuntos no distinguen pesos intermedios. Por ello, $W_{\text{vecino}} = 0,5$ y $W_{\text{símbolo}} = 0,9$ se fijan como **valores de partida**.

### 12.4 Siguientes mediciones

1. Uno o dos repos reales con un `CLAUDE.md` largo, 15–30 casos de sesiones pasadas, etiquetados a mano.
2. No cambiar constantes para mejorar los números del mismo conjunto: si hay que ajustar, separar casos de ajuste y de prueba.

Criterio de éxito: ≥ 70 % menos tokens que `completo`, **cero** críticas perdidas y ≥ 95 % de recall en `todo`. Si no se cumple, el entregable es la lista de fallos con su diagnóstico.

---

## 13. Fases

| Fase | Entrega | Estado |
|---|---|---|
| 0 | Esta guía | ✅ |
| 1 | `cthg-core` + `init`, `learn`, `confirm`, `deny`, `core`, `recall`, `list`, `mv` | ✅ (12 tests) |
| 2 | `import CLAUDE.md` | ✅ (14 tests) |
| 3 | Canal espacial completo: símbolos desde los hunks de `git diff`, renombres automáticos (`-M`), ranking de rutas por tamaño del diff | ✅ (16 tests) |
| 4 | `cthg hook` + deduplicación por sesión + instalar en un proyecto de prueba | ✅ (18 tests) |
| 5 | `verify` (anclar y desanclar contra el repo), más arreglos: símbolo del método editado, aviso de núcleo desbordado, renombres solo en el tronco, candado de escritura, `core --unset` | ✅ (27 tests) |
| 6 | `cthg eval` + conjunto sintético (sección 12); falta medir repos reales | ✅ (32 tests) |
| 6b | La memoria captura sola las correcciones del usuario, como propuestas que no se inyectan hasta `confirm`: hook `Stop` en Claude Code y Antigravity (sección 6.1) y `cthg corrections` para medirlas (sección 12.8). Formato v3 (flag `PROPOSED`) | ✅ (56 tests) |
| 8a | Grafo del código: escáner Rust, `.cthg/indice.tsv`, `index`, `map`, `where`, `callers` | ✅ (77 tests) |
| 8b | Enrutado por vecinos e identificadores de código con eval (canal espacial extendido a importadores y dependencias a 1 salto, atribución de vecino, desempate directo) | ✅ (89 tests) |
| 8c | Escáneres para más lenguajes (Python, JS/TS con JSX/TSX, HTML), firmas en todos los lenguajes, decoradores de Python, índice v2, aislamiento por familias y robustez ante pánicos | ✅ (109 tests) |
| 8d | Canal pedido: `cthg map` reordenado (reglas arriba, archivos nuevos y no indexados), skill actualizada, línea en CORE de sesión y hooks silenciosos | ✅ (117 tests) |
| 8e | Visor 3D (`memviz`): proyección del grafo de código junto al hipergrafo de memoria | pendiente |
| 8f | Base de datos y rutas HTTP: tablas y consultas por función, modelos ORM, decoradores de ruta; diseñarla tras verla en un proyecto real | pendiente |
| 7 | Ramas fusionadas visibles en el tronco, `gc`, visor 3D | pendiente (la fase 8 va antes que la 7) |

---

## 14. Riesgos abiertos

- **Clasificar `CORE`** era el punto único de fallo; con la selección automática (sección 12.6) las críticas perdidas bajan a 0 en los dos conjuntos reales. Queda: reglas de proceso sin palabra normativa, que no son candidatas, y el coste en tokens del núcleo en archivos cortos. Mitigación: `import` propone, el usuario confirma, `core`/`list`/`import`/`session-start` avisan si el núcleo no cabe, y la medición lista cada fallo.
- **Stemming demasiado ligero**: «despliego» no coincide con «despliegue» ni con «desplegar» (medido en `eval/ejemplo`, caso «desplegar»). Opciones: alias por regla, o un stemmer español algo más fuerte; decidirlo con datos de repos reales.
- **Reformulaciones que cambian una palabra clave**: «depende del crate tokio» se toma como repetición de «depende del crate serde» (similitud 0,75 ≥ 0,5) y refuerza el recuerdo equivocado; lo mismo pasa con `git status` y `git status && git push`. Pendiente: exigir que las palabras que no comparten no sean nombres propios, identificadores o rutas, o subir el umbral cuando la frase es corta.
- **Ruido espacial** en árboles sucios. Mitigación: ruido, ranking por tamaño del diff, máximo de rutas, pesos, presupuesto.
- **Reglas del usuario empiezan en «probable» (0,30)**, no en «verificado». Es intencionado (una sola afirmación no es prueba), pero si en la medición eso hace que se ignoren, se sube el prior de `usuario`.
- **Versionar `.cthg/` o no**: compartirlo con el equipo frente a mezclar correcciones personales. Se decide por proyecto.
