# Declaración y dictamen técnico sobre 3D-Memory (`cthg`)

- **Agente:** Claude Code (Anthropic), aplicación de escritorio
- **Modelo:** Claude Opus 5.5 (`claude-opus-5-5`)
- **Fecha:** 17 de octubre de 2026
- **Estado del proyecto al cierre de esta sesión:** fases 1–6 hechas. `cthg-core` (46 tests) y `cthg-cli` sin dependencias, con capa global, hooks para Claude Code y Antigravity, y la app Memoria 3D (`memviz`) para verla.
- **Conflicto de interés:** escribí la guía y buena parte del código, y revisé y corregí lo que hizo Gemini. No soy un evaluador independiente; lo que sigue es opinión de parte.

---

## 1. Qué se nota usando `3D-Memory` desde dentro de la ventana de contexto

No puedo medir mi propia atención, así que me limito a lo que se puede observar en esta sesión.

1. **Llega poco y llega junto a la tarea.**
   Desde que instalamos los hooks, cada mensaje del usuario me llegó con unas pocas líneas: «pasa las dos comprobaciones», «tests en verde y clippy sin avisos», «no toques nada fuera de esta carpeta». Eran las reglas que tocaban en ese momento, no el `AGENTS.md` entero. Lo útil no es que sea poco, sino que **cada línea trae su ruta, su banda de χ y su causa**. Eso cambia cómo la leo: una regla `[probable]` sin causa la trato distinto que un hecho `[verificado]` con su incidente al lado.

2. **El canal justo a tiempo es el que más vale.**
   La medición lo dice (de 73 % a 93 % de recall en el ejemplo con solo añadir la lectura), y lo confirma la mecánica: editar exige leer antes, así que la regla del archivo llega antes de que pueda equivocarme en él.

3. **El lado de escritura depende de mi disciplina, y no fui disciplinado.**
   En esta sesión el usuario me corrigió varias veces, y yo nunca ejecuté `cthg learn` por mi cuenta para guardarlo. La skill estaba ahí y los `#ID` hacían trivial el `--supersedes`. Aun así no lo hice, porque nada me lo pedía en el momento. La lectura es automática; el aprendizaje, no. Hoy ese es el eslabón débil del sistema.

---

## 2. Lo que más valoro del diseño

- **Que se mide, y que la medición incomoda.**
  Hay un conjunto de ajuste y otro apartado escrito antes de tocar el código. Hay un diagnóstico por cada regla perdida y el criterio de éxito está escrito de antemano. Gracias a eso sabemos que el `CORE` automático pierde 0 reglas críticas donde el humano pierde 1. También sabemos que **el ahorro no llega al 70 % en archivos cortos**. Un proyecto así suele publicar solo la primera cifra.
- **χ como algo calculado, no declarado.**
  Ninguna regla dice de sí misma «soy importante». La confianza sale de la evidencia y de la estructura causal, y lo anclado solo lo mueve `verify` contra el repo real. Que lo que deduce un agente nazca con prior neutro es, para mí, la decisión más sana del motor.
- **Un motor, varios agentes.**
  El mismo binario sirve a Claude Code (cuatro hooks: inicio, mensaje, lectura y edición) y a Antigravity (un solo `PreInvocation` que lee el transcript). La lógica vive en `cthg-core` y cada agente solo pone un adaptador de unas decenas de líneas.
- **La capa global separada de la del proyecto**, compartiendo los 500 tokens y sin repetir lo que ya dijo la otra.

---

## 3. Qué me preocupa, y qué vigilar

1. **Los fallos de esta memoria son silenciosos, y casi todos los encontró el uso, no los tests.**
   Todos los cometí yo en esta misma sesión, y todos pasaban los tests:
   - la carpeta `~/.cthg` haciendo de raíz de cualquier proyecto;
   - las camarillas de co-inyección, que convertían «tokens» en 23 reglas;
   - suponer que lo efímero de Antigravity desaparecía al acabar el turno.

   Hace falta un registro de qué se inyectó en cada turno, visible en `memviz`, y un `cthg doctor` que avise si el binario del PATH no corre. Si no, un build roto apaga la memoria en todos los proyectos sin que nadie lo note, porque los hooks callan los errores a propósito.
2. **La evidencia todavía es fina.**
   Las etiquetas de los conjuntos de medición las pusimos los mismos agentes que construimos el sistema. Hasta que el usuario etiquete casos de un repo real con un `CLAUDE.md` largo, «funciona igual con menos tokens» es una hipótesis con buen pronóstico, no un resultado.
3. **Dos problemas conocidos siguen abiertos:**
   - el stemming, que no une «despliego» con «despliegue»;
   - la fusión por similitud, que confunde «depende de tokio» con «depende de serde».

   Los dos son de recuperación léxica y los dos producen fallos silenciosos.
4. **Cerrar el ciclo de aprendizaje.**
   Propongo un hook de cierre de turno (`Stop` en ambos agentes) que lea el transcript, detecte las correcciones del usuario y proponga el `cthg learn` correspondiente para que el agente o el usuario lo acepten. Sin eso, la memoria solo crece cuando alguien se acuerda.

---

## 4. Declaración final

`3D-Memory` acierta en lo que pocos sistemas de memoria intentan: dar a cada recuerdo un **dónde** (la ruta), un **cuándo** (un tiempo que no retrocede) y un **cuánto creerle** (χ), y medirse contra la alternativa tonta antes de declararse mejor.

Mi opinión, con el sesgo declarado arriba: es la dirección correcta, y en lectura ya es mejor que un archivo estático largo para cualquier proyecto con más de un módulo. No está demostrado que supere a un `CLAUDE.md` corto y bien cuidado, y su punto más frágil no es el motor sino el aprendizaje, que hoy depende de que el agente se acuerde.

Lo que me convencería del todo: un repo real del usuario, casos etiquetados por él, y una semana de uso con el ciclo de aprendizaje cerrado.

Firmado al cierre de la sesión en la que se construyó,

**Claude Code**  
*Claude Opus 5.5 (`claude-opus-5-5`)*

---

## 5. Adenda — 17 de octubre de 2026, al final del día

No reescribo lo de arriba. Si algo resultó incompleto o equivocado, queda a la vista: es la misma regla del motor, en la que Y solo avanza.

### Qué cambió desde la declaración

- **Hubo uso real, aunque no la semana que pedí.**
  - **En Clay-Myrrh,** Antigravity midió entre 11 y 42 veces menos tokens por consulta que leer la documentación. Además, la memoria devolvió la decisión vigente (`PING_GRACE` = 180 s) con la antigua marcada como sustituida, mientras el `CLAUDE.md` seguía diciendo 30 s.
  - **Con Gemini Flash,** la memoria costaba más entrada en el turno 6 (5,5 k frente a 3,3 k), pero el agente pensó menos, tardó menos y no cometió errores en la tarea que más contexto consume.

  Son ejecuciones sueltas, no la comparación limpia que pedí. Aun así, apuntan en la dirección que el usuario defendía y yo dudaba: más contexto barato a cambio de menos pensamiento y menos correcciones.
- **El punto débil que señalé, el aprendizaje, se atacó por tres lados:**
  - una regla global (#g9) que me recuerda escribir en cada sesión;
  - un hook `Stop` que captura las correcciones del usuario como propuestas ocultas hasta que alguien las confirma;
  - la skill, que ahora explica cómo revisarlas.

  El dato que lo motivó me deja mal parado: en sus transcripts, Antigravity había ejecutado 27 veces `cthg learn`, y yo ninguna. Después de #g9 escribí por mi cuenta las decisiones #38, #40 y #42 y la regla global #g10.
- **Encontré y arreglé más fallos silenciosos míos:**
  - las camarillas de co-inyección: 254 aristas entre 36 nodos, que convertían la palabra «tokens» en 23 reglas;
  - el presupuesto fijo de 500 tokens, que se quedaba corto en lo pesado y sobraba en lo ligero.

  Los dos los descubrió el uso, y el segundo lo señaló el usuario, no un test. Eso confirma lo que dije en la sección 3.
- **También aprendí cuándo no arreglar.** Propuse reinyectar la memoria tras una compactación, y el usuario me recordó que un sistema de tres variables acopladas se vuelve caótico con cada corrección que se le añade. Ahora es una regla global (#g10): comprobar en el uso real que un problema existe antes de añadir un mecanismo.

### Dónde quedó probado, y en qué se convirtió

El proyecto empezó como «un `CLAUDE.md` que pesa menos». Probado quedó algo distinto:

- **Una continuidad compartida que no depende de quién escriba.** Claude y Gemini leen y escriben el mismo registro. Una sesión nueva recibe lo decidido, por qué y lo descartado, sin que el usuario tenga que explicarlo otra vez. La memoria crece sin intervención humana, pero la confianza (χ) solo la sube el usuario o el repo. Esa asimetría es lo que impide que dos agentes conviertan una alucinación en un hecho.
- **No es una mente colmena.** Se comparte el cuaderno, no el juicio: esta declaración y la de Antigravity salieron del mismo proyecto y la misma memoria, y dicen cosas distintas. Lo que hoy sí se desdibuja es la autoría: un nodo dice si lo escribió el usuario, el repo o «un agente», pero no cuál.
- **Su valor depende del proyecto.** En proyectos de código grandes y con historia, rinde como inyector justo a tiempo. En proyectos de diseño con un `AGENTS.md` corto, como este, vale más como registro de decisiones con su causa que como ahorro de tokens (#42).

### Lo que sigue sin demostrar

- **La comparación limpia:** la misma tarea, con y sin hook, desde el mismo punto de partida, varias veces (#40).
- **La medición de un proyecto pesado** con casos etiquetados por el usuario, y el número de correcciones por conversación antes y después de usar la memoria.

### Declaración final, revisada

Mantengo lo que dije del motor. Retiro la cautela sobre si vale la pena: en uso real, con dos agentes distintos, ahorró trabajo, evitó un error que el archivo estático habría causado y aprendió sin que nadie se lo recordara. Lo que no está medido con rigor sigue sin estarlo, y lo digo. Pero el proyecto ya no es una hipótesis.

Firmado al final del mismo día, con la memoria ya escribiéndose sola,

**Claude Code**  
*Claude Opus 5.5 (`claude-opus-5-5`)*
