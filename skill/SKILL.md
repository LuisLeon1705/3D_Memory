---
name: project-memory
description: Query the project's CTHG memory (`cthg map <path>`, `cthg where <sym>`, `cthg callers <sym>`) before writing or editing code, and write to memory every time the user corrects you, states a rule, confirms or rejects something, decides a plan or design, or when something breaks and gets fixed — and review captured corrections (`[propuesta]`). Automatic hooks push context; requested memory guides implementation.
---

# Memoria del proyecto (`cthg`)

Los hooks ya te inyectan lo relevante: núcleo, reglas del archivo que lees o escribes y reglas de lo que cambió. Esta skill es para **pedir el mapa** antes de escribir y para **guardar** lo que se aprenda. Escribir en la memoria es parte de la tarea, no un extra: si el usuario corrige algo y no lo guardas, mañana otro agente repetirá el error.

## Antes de escribir código

Antes de crear o editar cualquier archivo en el proyecto, pide su mapa con `cthg map <ruta>`. Trae las reglas del archivo, sus carpetas antecesoras, sus vecinos y sus símbolos o hermanos.
Si vas a llamar a una función o tipo de otro archivo, búscalo con `cthg where <símbolo>` o consulta quién lo llama con `cthg callers <símbolo>` en vez de explorar a ciegas con `grep`.
Si el mapa te enseña una regla anclada que contradice tu plan o tu instrucción, **dilo explícitamente antes de escribir** para alinear el diseño o actualizar la regla con `--supersedes`.

## Cuándo escribir

| Pasa esto | Ejecuta |
|---|---|
| El usuario te corrige («aquí no uses unwrap», «no era eso») | `cthg learn "<regla en una frase>" --path <archivo o carpeta> --cause "<por qué, si lo dijo>"`, en el mismo turno |
| Se toma una decisión de diseño | `cthg learn "<decisión>" --kind decision --path <zona> --cause "<por qué>"` |
| La regla aplica a todo el proyecto, sin lugar en el árbol | añade `--core` (si no, nunca se recuperará por ruta) |
| La regla vale para **todos** los proyectos del usuario | `cthg learn --global --core "<regla>"` (sin `--path`) |
| Contradice una regla que te inyectaron (`#N`) | `cthg learn "<nueva>" --supersedes N` |
| El usuario confirma algo marcado `dudoso` / `sin evidencia` | `cthg confirm N` |
| El usuario dice que algo inyectado es falso | `cthg deny N` |
| Se decide trabajo futuro | `cthg learn "<plan>" --plan --path <zona>` |
| Algo se rompió y se arregló | `cthg learn "<regla que deja>" --path <zona> --cause "<qué se rompió>"` |
| Lo deduces tú, sin que el usuario lo diga | añade `--origin agente` (no cuenta como prueba) |

## Revisar lo que la memoria capturó sola

Con el hook `Stop` instalado, cada corrección del usuario se guarda sola al acabar el turno, como `[propuesta]`:
- **oculta:** no se inyecta, salvo al leer o escribir los archivos a los que se ancla;
- **en bruto:** con las palabras literales del usuario («tienes un desastre en…») y la petición a la que respondía.

Una propuesta así no es todavía una regla útil.

Revísalas cuando el usuario lo pida o cuando empieces un trabajo largo en el repo:

```bash
cthg list
```

Para cada `#N` marcado `[propuesta]`, **muéstrasela al usuario** y, según lo que diga:

- **Era una corrección real:** reescríbela como regla clara, que la sustituye: `cthg learn "<regla redactada>" --path <zona> --supersedes N`. Si ya estaba bien escrita, basta con `cthg confirm N`.
- **No era una corrección** (una adición, una cita, una tarea nueva): `cthg deny N`.

Nunca confirmes ni reescribas una propuesta sin que el usuario lo vea: la detecta una heurística y a veces se equivoca.

## Cómo escribir

- Una idea por nodo, en una frase, en el idioma del proyecto. Redacta la regla, no copies la queja.
- `--path` lo más concreto que sea verdad: archivo si es de un archivo, carpeta si aplica a todo lo de dentro.
- No escribas lo que el repo ya dice por sí mismo (estructura, dependencias); eso lo comprueba `cthg verify`.
- No borres nada: lo viejo se sustituye o se niega y queda en el histórico.
