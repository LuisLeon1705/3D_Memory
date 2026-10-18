# Casos de evaluación — proyecto tareas (ajuste)

Imitan los encargos cortos a los workers del experimento `exp-memoria`: escriben los
archivos directamente sin leerlos antes (`lee:` vacío). Las líneas (`L3`–`L10`) son del
`AGENTS.md` de esta carpeta (`exp-memoria/reglas.md`). `!` marca una regla crítica.

## caso: worker 1 — id y almacenamiento
prompt: Construye src/id.rs y src/storage.rs con sus tests unitarios.
relevantes: L3!, L4!, L5!, L7!, L8, L9!

## caso: worker 2 — interfaz de línea de comandos
prompt: Construye src/cli.rs y src/main.rs para los subcomandos add, list, done y rm.
relevantes: L3!, L6!, L8!, L9!, L10!

## caso: worker 3 — tests de integración
prompt: Construye los tests de integración del ejecutable tareas en el directorio tests/.
relevantes: L3!, L4, L5, L6!, L9!, L10!

## caso: encargo sin nombrar archivos — persistencia
prompt: Implementa la carga y el guardado de la lista de tareas en disco con su formato e identificadores.
relevantes: L3!, L4!, L5!, L7!, L9!

## caso: encargo sin nombrar archivos — errores del ejecutable
prompt: Gestiona los argumentos de entrada y devuelve el código de salida adecuado ante fallos de uso o de disco.
relevantes: L3!, L6!, L8!, L10
