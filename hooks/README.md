# Hooks

`settings.example.json` es el bloque que se copia al `.claude/settings.json` **del proyecto** (no al global) para activar `cthg hook`.

Todos los hooks llaman al mismo binario; los scripts no tienen lógica, así funcionan igual en Windows y en Unix.

Los hooks llaman a `cthg` por su nombre, sin rutas fijas. Para eso tiene que estar en el PATH:

```bash
cargo install --path crates/cthg-cli --locked
```

Eso lo deja en `~/.cargo/bin`. Después de cambiar el código de `cthg`, repite la instalación: los hooks de todos los proyectos usan ese binario.

| Evento | Subcomando | Lee de stdin (JSON) | Escribe |
|---|---|---|---|
| `SessionStart` | `cthg hook session-start` | `session_id`, `cwd` | Canal 0 (núcleo). Arranca la sesión: olvido (decaimiento de activación) |
| `UserPromptSubmit` | `cthg hook prompt` | `prompt`, `session_id`, `cwd` | Canal léxico + canal espacial de `git status` / `git diff` |
| `PostToolUse` · `Read` | `cthg hook read` | `session_id`, `cwd`, `tool_input.file_path` | Canal espacial justo a tiempo (`hookSpecificOutput.additionalContext`): reglas de ese archivo y sus carpetas |
| `PostToolUse` · `Edit\|MultiEdit\|Write` | `cthg hook edit` | `cwd`, `tool_input.file_path` | Nada. Sube la activación de esa zona del árbol |

Reglas de los hooks:

1. **Nunca bloquear.** Si algo falla, salir con código 0 y sin salida.
2. **No repetir.** `.cthg/sesion/<session_id>.json` guarda los ids ya inyectados en la sesión.
3. **Presupuesto.** Núcleo ≤ 500 tokens, prompt ≤ 800, por archivo ≤ 300.
4. **Sin carreras.** El hook espera hasta 1,5 s por `.cthg/lock`; si otro proceso la tiene, responde igual pero no guarda (se pierde un poco de activación, nunca un recuerdo).
5. **Núcleo completo o avisado.** Si las reglas `CORE` no caben en 500 tokens, `session-start` añade una línea diciendo cuántas faltan.
6. `hook read` emite `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"..."}}` para que Claude Code inyecte el contexto tras la lectura.
