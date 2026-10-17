# 3D-Memory — guía de diseño

Memoria de proyecto para agentes de código (Claude Code primero). Objetivo: sustituir la mayor parte de un `CLAUDE.md` / `AGENTS.md` por **un núcleo pequeño que se inyecta siempre + los recuerdos que aplican a lo que se está tocando**, con confianza, causas y tiempo explícitos.

Motor: **CTHG** (*Causal-Temporal HyperGraph*): un grafo de recuerdos en un archivo binario, con tres ejes.

Estado: `cthg-core` completo y probado (27 tests); CLI con `init`, `learn`, `confirm`, `deny`, `core`, `recall`, `list`, `mv`, `import`, `hook` y `verify`. Falta la medición y las herramientas de mantenimiento de la fase 7 (ver [Fases](#13-fases)).

---

## 1. Qué problema resuelve

| Un `CLAUDE.md` | 3D-Memory |
|---|---|
| Se carga entero en cada sesión, venga o no al caso | Núcleo de ~300–500 tokens + lo que aplica a la tarea y a los archivos tocados |
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
| **0 · núcleo** | al iniciar la sesión | nodos `CORE` visibles | reglas transversales sin lugar en el árbol |
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
| relacion | 0,25 + 0,5 · hebb | lo que suele ir junto |
| contradice, sustituye | 0,3 | traer lo que **ya se probó** y se descartó |
| menciona | 0,2 | |
| precede | 0,15 | |

### 4.4 Ranking y presupuesto

```
R(n) = ( r(n) + 0,3·activación(n) ) · ( 0,6 + 0,4·χ(n) )        solo si r(n) > 0,05 y n no es CORE
```

El factor de confianza va de 0,2 (χ = −1) a 1 (χ = +1): lo descartado puede aparecer, pero abajo.

Después se recorta con un presupuesto de tokens (≈ caracteres/4), en orden de R, saltando lo que no cabe: núcleo ≤ 500, prompt ≤ 800, por archivo ≤ 300. La salida se agrupa por bandas de χ, cada línea con su ruta y su causa (`← …`).

### 4.5 Olvido y asociación

Por sesión: `activación ← 0,92 · activación`, `hebb ← 0,995 · hebb`. Cuando un nodo se inyecta: `activación = 1`. Dos nodos inyectados juntos refuerzan su arista `relacion`: `hebb ← hebb + 0,15·(1 − hebb)`.

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
│  │     ├─ import.rs         importar Markdown, deducir rutas y proponer CORE
│  │     ├─ hook.rs           hooks de Claude Code, deduplicación por sesión
│  │     ├─ verify.rs         verificar rutas, símbolos, dependencias y comandos contra el repo
│  │     ├─ text.rs           tokens es/en, similitud, recortes, tokens aproximados
│  │     └─ binary.rs         formato .cthg (CRC32, guardado atómico)
│  └─ cthg-cli/               binario `cthg`
│     └─ src/main.rs
├─ hooks/                     settings.example.json + README
├─ skill/SKILL.md             cuándo escribir en la memoria
└─ docs/formato-cthg.md       especificación byte a byte
```

En cada proyecto que use la memoria: `<repo>/.cthg/memoria.cthg` (+ `.bak`), `<repo>/.cthg/sesion/<session_id>.json` para no repetir lo ya inyectado y `<repo>/.cthg/lock` mientras alguien la modifica. Si se versiona `.cthg/` o no se decide por proyecto (sección 14).

---

## 9. CLI `cthg`

| Comando | Estado | Qué hace |
|---|---|---|
| `cthg init` | ✅ | crea `.cthg/memoria.cthg` en la raíz git; detecta el tronco |
| `cthg learn <texto> [--kind K] [--path P] [--symbol S] [--cause T] [--cause-id N] [--contradicts N] [--supersedes N] [--origin O] [--core] [--plan] [--deny]` | ✅ | sección 6 |
| `cthg confirm N` / `cthg deny N` | ✅ | α + 1 / β + 1 (lo anclado no se mueve) |
| `cthg core [<id>… [--unset]] [--budget N]` | ✅ | canal 0; con ids, marca o desmarca `CORE`. Avisa si el núcleo no cabe en el presupuesto |
| `cthg recall [--prompt T] [--path P]… [--git] [--budget N] [--limit N]` | ✅ | canales 1–3 (rutas, símbolos de `git diff`, renombres `-M`) |
| `cthg list [--all]` | ✅ | todos los nodos con χ, flags (incluido `obsoleto`) y rama |
| `cthg mv <de> <a>` | ✅ | la memoria sigue a un archivo o carpeta movidos |
| `cthg import <archivo.md> [--yes]` | ✅ | parte el archivo en nodos, deduce rutas existentes y propone cuáles son `CORE` |
| `cthg hook <session-start\|prompt\|read\|edit>` | ✅ | lee el JSON del hook por stdin, deduplica por sesión en `.cthg/sesion/<session_id>.json` |
| `cthg verify [--run-commands]` | ✅ | sección 7.1 |
| `cthg gc`, `cthg view` | fase 7 | archivar lo muerto; visor 3D con X = árbol de carpetas |

---

## 10. Hooks de Claude Code

| Evento | Comando | Canal |
|---|---|---|
| `SessionStart` | `cthg hook session-start` | 0, y un paso de olvido |
| `UserPromptSubmit` | `cthg hook prompt` | 1 + 3 |
| `PostToolUse` (`Read`) | `cthg hook read` | 2 (`hookSpecificOutput.additionalContext`) |
| `PostToolUse` (`Edit\|MultiEdit\|Write`) | `cthg hook edit` | sube la activación de esa zona |

Plantilla y reglas en [hooks/](hooks/README.md). En resumen: nunca bloquear (si falla, código 0 y sin salida), no repetir dentro de una sesión y respetar el presupuesto (500 / 800 / 300 tokens). Si la memoria está bloqueada por otro proceso más de 1,5 s, el hook responde igual pero no guarda. Si el núcleo no cabe en 500 tokens, `session-start` lo dice en el propio contexto, en vez de recortarlo en silencio. Se instalan en el `.claude/settings.json` **del proyecto**, no en el global.

Los `CLAUDE.md` anidados por carpeta que Claude Code ya carga son la **línea base** contra la que medir.

---

## 11. Formato binario

Un solo archivo (versión 2; lee también la 1) con secciones `META`, `NODE`, `EDGE`, `HYPE` y CRC32 final; las secciones desconocidas se saltan.

Escritura segura: toda modificación se hace con `.cthg/lock`; cada escritor usa su propio `.tmp` y renombra (el anterior queda en `.bak`); si el archivo está corrupto, se carga el `.bak`. Un candado de más de 30 s se considera abandonado. χ no se guarda: se calcula al cargar para la vista activa. Especificación completa: [docs/formato-cthg.md](docs/formato-cthg.md).

Coste: con cientos o pocos miles de nodos, una puntuación completa es O(N log N + E) y tarda milisegundos. Lo caro son los tokens inyectados, no la CPU.

---

## 12. Medición (antes de afirmar que «pesa menos y funciona igual»)

1. Elegir uno o dos repos reales con un `CLAUDE.md` largo.
2. Juntar 15–30 prompts de tarea reales y **marcar a mano** qué reglas eran relevantes en cada uno.
3. Comparar por prompt:
   - tokens inyectados: `CLAUDE.md` completo · `CLAUDE.md` anidados · 3D-Memory por canal;
   - recall de reglas relevantes con presupuesto fijo: léxico · espacial · espacial + justo a tiempo · todo;
   - lista de reglas relevantes **no** recuperadas, cada una con su causa (¿debía ser `CORE`? ¿mal anclada?).
4. Opcional: repetir tareas reales con cada configuración y contar reglas violadas.

Criterio de éxito propuesto: ≥ 70 % menos tokens que el `CLAUDE.md` completo, **cero** reglas críticas perdidas y ≥ 95 % de recall sobre las relevantes.

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
| 6 | Medición (sección 12) | pendiente |
| 7 | Ramas fusionadas visibles en el tronco, `gc`, visor 3D | pendiente |

---

## 14. Riesgos abiertos

- **Clasificar `CORE`** es el punto único de fallo restante. Mitigación: `import` propone, el usuario confirma, `core`/`list`/`import`/`session-start` avisan si el núcleo no cabe, y la medición lista cada fallo.
- **Reformulaciones que cambian una palabra clave**: «depende del crate tokio» se toma como repetición de «depende del crate serde» (similitud 0,75 ≥ 0,5) y refuerza el recuerdo equivocado; lo mismo pasa con `git status` y `git status && git push`. Pendiente: exigir que las palabras que no comparten no sean nombres propios, identificadores o rutas, o subir el umbral cuando la frase es corta.
- **Ruido espacial** en árboles sucios. Mitigación: ruido, ranking por tamaño del diff, máximo de rutas, pesos, presupuesto.
- **Reglas del usuario empiezan en «probable» (0,30)**, no en «verificado». Es intencionado (una sola afirmación no es prueba), pero si en la medición eso hace que se ignoren, se sube el prior de `usuario`.
- **Versionar `.cthg/` o no**: compartirlo con el equipo frente a mezclar correcciones personales. Se decide por proyecto.
