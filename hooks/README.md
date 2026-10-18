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
| `PostToolUse` · `Edit\|MultiEdit\|Write` | `cthg hook edit` | `session_id`, `cwd`, `tool_input.file_path` | Canal espacial justo a tiempo (`hookSpecificOutput.additionalContext`): reglas de ese archivo y sus carpetas (si no se inyectaron antes), y sube la activación de esa zona |
| `Stop` | `cthg hook stop` | `cwd`, `transcript_path` | Nada (nunca bloquea el final del turno). Lee el transcript y aprende las correcciones del usuario (GUIA §6.1) |

Reglas de los hooks:

1. **Nunca bloquear.** Si algo falla, salir con código 0 y sin salida.
2. **No repetir.** `.cthg/sesion/<session_id>.json` guarda los ids ya inyectados en la sesión.
3. **Presupuesto.** Dinámico, según el tamaño de la memoria (GUIA §4.4): núcleo, mensaje (menos cuanto más específico es) y archivo, con un tope por conversación que nunca supera lo que costaría el archivo estático.
4. **Sin carreras.** El hook espera hasta 1,5 s por `.cthg/lock`; si otro proceso la tiene, responde igual pero no guarda (se pierde un poco de activación, nunca un recuerdo).
5. **Núcleo completo o avisado.** Si las reglas `CORE` no caben en su presupuesto, `session-start` añade una línea diciendo cuántas faltan.
6. `hook read` emite `{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"..."}}` para que Claude Code inyecte el contexto tras la lectura.
7. `hook stop` no escribe nada: salida vacía y código 0 dejan terminar el turno. Lo que captura queda como **propuesta** (`[propuesta]` en `cthg list`): no se inyecta hasta `cthg confirm <id>`, salvo al leer sus propios archivos. Cada corrección se aprende una vez (`.cthg/correcciones.txt` guarda su huella), así que un segundo `Stop` sobre el mismo transcript, o una sesión reanudada, no la duplican. El formato del transcript de Claude Code es interno y puede cambiar: el lector ignora lo que no reconoce.

## Google Antigravity

Antigravity tiene hooks propios (`.agents/hooks.json`) con otro contrato:

- **Solo `PreInvocation` puede añadir contexto.** Corre antes de cada llamada al modelo y usa `injectSteps: [{"ephemeralMessage": …}]`, que dura hasta el final del turno.
- **`PostToolUse` debe devolver `{}`.**
- **El prompt no llega por stdin**, pero sí `transcriptPath`, el registro de la conversación (una línea JSON por paso).

Por eso basta un hook, sin tocar los permisos de las herramientas: [antigravity.hooks.json](antigravity.hooks.json) → `.agents/hooks.json` del proyecto.

`cthg hook antigravity` lee solo las líneas nuevas del transcript (guarda por dónde iba en `.cthg/sesion/<conversationId>.json`):

| Pasa esto | Inyecta |
|---|---|
| La primera llamada de la conversación | núcleo global y del proyecto (y si la memoria es pequeña, entra entera al inicio), una sola vez |
| Un `USER_INPUT` nuevo o `SYSTEM_MESSAGE` de agente (turno nuevo) | lo que recuerdan el mensaje y `git status`, sin repetir lo ya inyectado |
| Un `view_file` nuevo en el turno | las reglas de ese archivo, una vez por turno |
| `write_to_file` / `replace_file_content` nuevo en el turno | las reglas de ese archivo (si no se inyectaron antes) y sube la activación |
| Nada nuevo | `{}` |

Al terminar cada turno, el evento `Stop` (estructura plana, sin `matcher`) llama a `cthg hook antigravity-stop`: lee el transcript entero (`transcriptPath`), aprende las correcciones del usuario igual que en Claude Code y responde siempre `{}` (sin `decision: "continue"`, así que nunca impide que el agente se detenga).

La skill va en `.agents/skills/project-memory/SKILL.md` (mismo archivo que [skill/SKILL.md](../skill/SKILL.md)). Antigravity además lee `AGENTS.md`/`GEMINI.md` por su cuenta, así que el hook no reinyecta lo importado de esos archivos (en Claude Code, lo mismo con `CLAUDE.md`).

Para que valga en todos los proyectos, el mismo bloque puede ir en `~/.gemini/config/hooks.json`; en un repo sin memoria el hook responde `{}`.

