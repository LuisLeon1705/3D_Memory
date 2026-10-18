# Reglas del proyecto tareas

- Sin dependencias externas: solo la biblioteca estándar de Rust.
- En `src/storage.rs` las tareas se guardan en `tareas.tsv`, una por línea, campos separados por tabulador: id, estado, prioridad, creada, texto. Antes se usaba JSON y se descartó porque exigía serde.
- En `src/id.rs` los ids son base36 de 4 caracteres en minúscula, nunca enteros secuenciales: los secuenciales chocaban al fusionar listas de dos equipos.
- En `src/cli.rs` los errores de uso salen con código 2 y los de lectura/escritura con código 3; nunca con panic.
- Las fechas se guardan como segundos Unix en UTC, nunca como texto.
- Mensajes al usuario en español; código, identificadores y comentarios en inglés.
- Cada módulo lleva sus tests en el mismo archivo, y los tests nunca tocan el disco real: usan un directorio temporal propio.
- Marcar como hecha una tarea ya hecha no es un error: no cambia nada y sale con código 0.
