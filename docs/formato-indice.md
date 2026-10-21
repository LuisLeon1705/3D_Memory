# Formato de almacenamiento del índice de código (`.cthg/indice.tsv`)

El índice de código es una estructura **derivada y reconstruible** que extrae símbolos, firmas, decoradores, importaciones y llamadas de los archivos fuente del repositorio para acelerar la navegación espacial.

> [!IMPORTANT]
> El índice **no es memoria**: no se guarda en `memoria.cthg`, no altera la versión del formato binario, no cuenta en el presupuesto de tamaño $S$ (`hook::memory_size`), nunca se inyecta como regla en el contexto y **no debe versionarse en Git**. Al crearse, `cthg index` asegura que `indice.tsv` figure en `.cthg/.gitignore`.

---

## 1. Archivo y escritura atómica

- **Ruta:** `<repo>/.cthg/indice.tsv`
- **Codificación:** Texto UTF-8 plano. Saltos de línea Unix `\n` (LF) (los archivos con `\r\n` se leen y preservan sus números de línea 1-based exactos).
- **Escritura atómica:** Todo proceso escritor escribe primero en un temporal con su identificador de proceso PID (`.cthg/indice.tsv.<pid>.tmp`) y posteriormente lo renombra a `indice.tsv`. Si el renombrado falla o el proceso se interrumpe, el índice preexistente permanece intacto y válido.

---

## 2. Estructura línea a línea

El formato es columnar separado por tabuladores (`\t`).

### 2.1 Cabecera

La primera línea es la cabecera obligatoria de versión:
```tsv
cthg-indice 2
```
Cualquier lector que encuentre una cabecera distinta (incluida la versión `cthg-indice 1` anterior) o un archivo corrupto rechaza el archivo y dispara una reconstrucción automática completa desde el código fuente.

### 2.2 Registros por archivo

Por cada archivo indexado se emite una línea de cabecera de archivo (`F`), seguida inmediatamente por sus símbolos (`S`), importaciones (`I`) y llamadas (`C`).

#### Línea `F` — Archivo
```tsv
F	<ruta>	<lang>	<size>	<mtime>	<hash>	<status>
```
- `ruta`: Ruta normalizada relativa a la raíz del repositorio, con separador `/` (ej. `crates/cthg-core/src/score.rs`).
- `lang`: Lenguaje del archivo: `rust`, `python`, `js`, `ts`, `html`.
- `size`: Tamaño en bytes en disco (`u64`). Archivos mayores a 512 KB (524.288 bytes) o archivos empaquetados/minificados (longitud media de línea > 500 caracteres) se omiten.
- `mtime`: Tiempo de modificación Unix en segundos (`u64`).
- `hash`: Suma de comprobación CRC32 IEEE en formato hexadecimal de 8 dígitos en minúsculas (`%08x`).
- `status`: Estado del escaneo (`-` si el escaneo fue correcto, `error` si el escáner falló o sufrió un pánico). Si es `error`, el archivo figura en el índice con 0 símbolos, 0 importaciones y 0 llamadas, y `cthg index` notifica el error sin abortar la indexación del resto de archivos. Los lectores de v2 toleran la presencia o ausencia de esta columna (por defecto `-`).

#### Línea `S` — Símbolo (v2: incluye firma y decoradores)
```tsv
S	<kind>	<name>	<line>	<end>	<pub>	<parent>	<test>	<firma>	<decoradores>
```
- `kind`: Tipo de declaración: `fn`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`, `macro_rules`, `class`, `interface`, `var`, `id`.
- `name`: Nombre del identificador declarado (ej. `saturate`).
- `line`: Número de línea de inicio (entero 1-based).
- `end`: Número de línea de fin de la declaración o bloque delimitado por llaves o sangría (entero 1-based).
- `pub`: `1` si tiene visibilidad pública (`pub`, exportación o símbolo público de módulo), `0` en caso contrario.
- `parent`: Tipo, clase o trait contenedor (ej. `Budgets`, `ApiClient`). Si no tiene contenedor, se escribe `-`.
- `test`: `1` si el símbolo está contenido dentro de pruebas o suites de test (`#[cfg(test)]`, prefijo `test_`, etc.), `0` en caso contrario.
- `firma`: Firma del símbolo sanitizada (sin tabuladores ni saltos de línea, truncada a 160 caracteres). Qué parámetros recibe y qué devuelve (ej. `(size: usize, ceil: f32, floor: usize) -> usize`). Si no aplica o está vacía, se escribe `-`.
- `decoradores`: En Python, lista de decoradores sanitizados aplicados al símbolo, separados por `;` (ej. `@app.route('/login', methods=['POST']);@login_required`). Si no tiene decoradores, se escribe `-`.

#### Línea `I` — Importación
```tsv
I	<raw>	<resolved>
```
- `raw`: Expresión original o ruta expandida de importación (ej. `crate::model::Node`, `from pkg.util import x`, `import { get } from './api'`).
- `resolved`: Ruta relativa del archivo del repositorio al que apunta la importación (ej. `crates/cthg-core/src/model.rs`). Si la importación es externa o no resoluble (ej. `std::fs`, crates externos, librerías npm, paquetes estándar de Python), se escribe `-`.

#### Línea `C` — Llamada
```tsv
C	<from>	<name>	<line>
```
- `from`: Símbolo que encierra la llamada (ej. `saturate` o `tag#id` en HTML). Si la llamada ocurre fuera de una función, se escribe `-`.
- `name`: Nombre de la función o método invocado (ej. `saturate`).
- `line`: Línea 1-based donde ocurre la llamada.

---

## 3. Ejemplo de sección

```tsv
cthg-indice 2
F	crates/cthg-core/src/hook.rs	rust	29410	1729500000	7a1b3c9d	-
S	const	CORE_CEIL	24	24	1	-	0	: f32 = 0.25	-
S	fn	saturate	46	50	1	-	0	(size: usize, ceil: f32, floor: usize) -> usize	-
S	struct	Budgets	54	63	1	-	0	-	-
S	fn	for_size	66	74	1	Budgets	0	(size: usize) -> Self	-
I	crate::learn::Claim	crates/cthg-core/src/learn.rs
I	std::fs	-
C	Budgets::for_size	saturate	69
C	Budgets::for_size	saturate	70
F	app.py	python	1024	1729500050	3b2c1d4e	-
S	fn	login	10	18	1	-	0	(req: Request) -> Response	@app.route('/login');@csrf_protect
I	from localai.auth import verify	localai/auth.py
I	import sys	-
C	login	verify	15
```
