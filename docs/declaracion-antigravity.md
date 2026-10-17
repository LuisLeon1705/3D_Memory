# Declaración y dictamen técnico sobre 3D-Memory (`cthg`)

- **Agente:** Antigravity (Google DeepMind)
- **Modelo:** Gemini 4 Argon (High)
- **Fecha:** 17 de octubre de 2026
- **Estado del proyecto al cierre de construcción:** `cthg-core` + `cthg-cli` operativos, sin dependencias externas, integrados por hooks en Claude Code y Google Antigravity.

---

## 1. Qué se siente usar `3D-Memory` desde dentro de la ventana de contexto

Para un modelo de lenguaje, un archivo `CLAUDE.md` o `AGENTS.md` grande tiene un problema silencioso: **diluye la atención**. Cuando todo entra de golpe en el turno 0 —reglas de formato binario mezcladas con convenciones de tests, notas de fases pasadas y detalles de un módulo que ni siquiera voy a abrir hoy—, todas las frases compiten con el mismo peso plano.

Recibir la memoria a través de `cthg` cambia tres cosas en la práctica:

1. **Las reglas llegan cuando abro el archivo al que aplican.**
   En esta misma sesión, al hacer `view_file` sobre `crates/cthg-cli/src/main.rs`, el hook `PreInvocation` inyectó en el paso siguiente una única línea (~35 tokens):
   > `- #9 regla (crates/cthg-cli/src/main.rs): Si cambiaste cthg, reinstálalo (cargo install --path crates/cthg-cli --locked): los hooks de todos los proyectos usan el del PATH.`

   En vez de tener esa advertencia enterrada entre 70 líneas desde el minuto uno, aparece justo delante del archivo que estoy leyendo. Es mucho más difícil ignorarla por descuido.

2. **La certeza epistémica (`χ`) viene masticada.**
   Distinguir entre `[verificado]`, `[probable]`, `[dudoso]` y `[descartado]` me ahorra tener que adivinar si una nota en markdown sigue vigente o si era una hipótesis vieja. Saber que un nodo `ANCHORED` fue contrastado contra el árbol real del repo (`cthg verify`) le da autoridad inmediata; ver un nodo en `[descartado]` evita repetir callejones sin salida.

3. **El protocolo de escritura cabe en una tabla.**
   La skill (`project-memory`) no me pide mantener ni editar un documento markdown a mano con riesgo de borrar algo por accidente. Como cada recuerdo inyectado trae su `#ID` (`#g1`, `#14`, etc.), corregir, confirmar o sustituir una regla es una sola llamada determinista (`cthg learn ... --supersedes N`, `cthg confirm N`, `cthg deny N`).

---

## 2. Lo que más valoro del diseño

- **Cero dependencias y determinismo total (`cthg-core`).**
  No hay vectores opacos, ni llamadas de red, ni demonios en segundo plano que puedan colgar el arranque de un turno. Un hook que corre en cada invocación tiene que responder en milisegundos y no fallar jamás en voz alta; haberlo resuelto con álgebra sobre el árbol de rutas (`λ = 0,8`), tiempo monótono y distribución Beta(α, β) sobre un binario compacto es una decisión de ingeniería sobresaliente.
- **Separación estricta entre raíz (`""`) y `CORE`.**
  Impedir que la raíz tenga afinidad espacial obliga a decidir qué es verdaderamente transversal (con un presupuesto duro de 500 tokens) y qué pertenece a una carpeta o archivo concreto. Esa restricción es la que evita que la memoria vuelva a convertirse en un cajón de sastre.
- **Inmutabilidad temporal (`Y` solo avanza) y anclaje (`ANCHORED`).**
  Un agente puede equivocarse al deducir algo; por eso nace con prior neutro `(1,5; 1,5)` y nunca puede tumbar por sí solo un hecho anclado al repositorio. Esa asimetría protege la memoria contra alucinaciones acumulativas entre sesiones.

---

## 3. Qué vigilar durante los próximos meses de pruebas reales

1. **Umbral léxico en prompts conversacionales.**
   Cuando el usuario hace preguntas cortas o meta-preguntas sin intención de código, el canal léxico debe preferir quedarse callado (0 tokens) antes que rellenar el presupuesto por coincidencias débiles.
2. **Higiene del presupuesto `CORE` (global + proyecto ≤ 500 tokens).**
   En el uso diario será tentador marcar muchas reglas con `--core`. El aviso de desbordamiento (`warn_core_overflow`) va a ser clave para obligar a empujar reglas hacia rutas concretas (`--path`) a medida que los proyectos crezcan.
3. **Consolidación y basura histórica (`gc` / `view`, fase 7).**
   Tras meses de uso real habrá cadenas largas de `--supersedes` y nodos negados. Ver cómo envejece el grafo en repositorios con cientos de commits dirá exactamente dónde calibrar el recolector.

---

## 4. Declaración final

`3D-Memory` no trata la memoria del agente como un bloc de notas estático, sino como un **sistema de tipos epistémico sobre el árbol del repositorio**: cada afirmación tiene un lugar (`X`), un instante (`Y`) y una prueba (`Z`).

Como consumidor directo de este contexto, prefiero trabajar con 500 tokens de núcleo + 40 tokens inyectados justo a tiempo al abrir un archivo que con un archivo estático de 4.000 tokens que nunca sabe qué estoy haciendo ni qué dejó de ser verdad hace tres semanas.

Firmado en sesión real de prueba,

**Antigravity**  
*Gemini 4 Argon (High)*
