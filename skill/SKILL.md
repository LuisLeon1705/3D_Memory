---
name: project-memory
description: Write to this repository's CTHG memory (`cthg`) when the user corrects you, states a rule, confirms or rejects something, decides a plan, or when something breaks and gets fixed. Use it to learn; reading is done automatically by hooks.
---

# Memoria del proyecto (`cthg`)

Los hooks ya te inyectan lo relevante (núcleo, reglas del archivo que lees, reglas de lo que cambió). Esta skill es para **escribir**.

## Cuándo escribir

| Pasa esto | Ejecuta |
|---|---|
| El usuario te corrige («aquí no uses unwrap») | `cthg learn "<regla en una frase>" --path <archivo o carpeta> --cause "<por qué, si lo dijo>"` |
| La regla aplica a todo el proyecto, sin lugar en el árbol | añade `--core` (si no, nunca se recuperará por ruta) |
| Contradice una regla que te inyectaron (`#N`) | `cthg learn "<nueva>" --supersedes N` |
| El usuario confirma algo marcado `dudoso` / `sin evidencia` | `cthg confirm N` |
| El usuario dice que algo inyectado es falso | `cthg deny N` |
| Se decide trabajo futuro | `cthg learn "<plan>" --plan --path <zona>` |
| Algo se rompió y se arregló | `cthg learn "<regla que deja>" --path <zona> --cause "<qué se rompió>"` |
| Lo deduces tú, sin que el usuario lo diga | añade `--origin agente` (no cuenta como prueba) |

## Cómo escribir

- Una idea por nodo, en una frase, en el idioma del proyecto.
- `--path` lo más concreto que sea verdad: archivo si es de un archivo, carpeta si aplica a todo lo de dentro.
- No escribas lo que el repo ya dice por sí mismo (estructura, dependencias); eso lo comprueba `cthg verify`.
- No borres nada: lo viejo se sustituye o se niega y queda en el histórico.
