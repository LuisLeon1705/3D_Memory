---
name: project-memory
description: Write to this repository's CTHG memory (`cthg`) every time the user corrects you, states a rule, confirms or rejects something, decides a plan or design, or when something breaks and gets fixed — and review the corrections the memory captured on its own (`[propuesta]`). Reading is automatic (hooks); writing is your job.
---

# Memoria del proyecto (`cthg`)

Los hooks ya te inyectan lo relevante: núcleo, reglas del archivo que lees o escribes y reglas de lo que cambió. Esta skill es para **escribir**, y escribir es parte de la tarea, no un extra: si el usuario corrige algo y no lo guardas, mañana otro agente repetirá el error.

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
