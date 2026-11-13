# Contrato de interfaz

Superficies opacas y estados honestos en toda la ventana principal. Fuente de tokens: `src/app.css`. La aceptación visual, de accesibilidad y del dueño sigue el roadmap.

## Idioma

- Preferencia `interface.language` (`en` | `es`, esquema de preferencias 3; un archivo de esquema 2 se lee como `en` sin reescribirlo). Por defecto `en`: la ventana en inglés es idéntica a la anterior.
- La frase en inglés es la clave. En un componente se escribe `t('Sentence {place}', { place })`; fuera de un componente (tablas de etiquetas, presentadores) se marca con `msg('Sentence')` y se traduce con `t()` donde se dibuja. Módulo: `src/lib/i18n/`.
- `src/lib/i18n/es.ts` es el único diccionario. `tests/i18n.test.ts` falla si una frase de `src/` no tiene entrada o si los lugares `{…}` no coinciden.
- Solo se traduce el texto propio de la ventana. Lo que redacta el escritorio (Rust), la autoridad o un proveedor se muestra como llega.
- No se traducen nombres de producto ni de proveedor, ni las palabras que el producto usa como nombres (Inference, Local Tools, Native, Managed).

## Design System

Fuente única: tokens en `src/app.css` (`:root`, `[data-theme='light']`, `[data-theme='high_contrast']`).
Ningún componente usa hex, `rgba()`, `backdrop-filter`, `blur` ni `gradient` para una superficie.

| Token                                                  | Uso                                                 |
| ------------------------------------------------------ | --------------------------------------------------- |
| `--chrome`                                             | barra lateral, barra de la Consola, barras de abajo |
| `--bg`                                                 | lienzo de la vista                                  |
| `--surface`                                            | tarjetas, paneles, diálogos, filas                  |
| `--surface-raised`                                     | campos, filas anidadas, menús                       |
| `--surface-hover`                                      | hover de filas y botones                            |
| `--surface-strong`                                     | presionado, pestaña activa                          |
| `--accent-soft`                                        | seleccionado (sólido, nunca degradado)              |
| `--line`                                               | divisores y bordes de tarjeta                       |
| `--line-strong`                                        | bordes de controles (≥ 3:1 sobre toda superficie)   |
| `--focus`                                              | anillo de foco de 2 px con 2 px de separación       |
| `--ok/-soft/-line`, `--warn…`, `--error…`, `--pending` | estados                                             |
| `--shadow-pop`                                         | solo menús, diálogos y toasts flotantes             |

Ventana siempre opaca, sin transparencia, blur de KWin ni atributo `data-glass`.

Componentes compartidos (clases globales, no por vista): `.action` (+ `.primary`, `.danger`,
`.quiet`), `.badge[data-tone]`, `.state-stripe` (`[data-tone]` en la fila), `.field` (label +
control + ayuda), `.panel`, `.empty-state`, `.notice[data-tone]`.

## Spacing

Escala de 4 px: 4, 8, 12, 16, 24, 32. Padding de panel 16 px (12 px bajo 720 px). Separación entre
filas de lista 6–8 px. Separación mínima entre controles 8 px.

## Typography

IBM Plex Sans (`--font-sans`) y IBM Plex Mono (`--font-mono`, también en la terminal) para rutas, comandos, ids, hashes y versiones. Las fuentes van incluidas en `src/assets/fonts/` (SIL OFL 1.1, `OFL.txt` al lado): la ventana nunca las pide a la red. `main.ts` espera la mono, con tope de 1,5 s, antes de montar, así la terminal mide sus celdas con la fuente final.

| Token       | Tamaño       | Uso                        |
| ----------- | ------------ | -------------------------- |
| `--text-xs` | 12 px / 1.4  | insignias, metadatos, pies |
| `--text-sm` | 13 px / 1.45 | texto secundario, ayudas   |
| `--text-md` | 14 px / 1.5  | cuerpo, controles, filas   |
| `--text-lg` | 16 px / 1.4  | títulos de sección         |
| `--text-xl` | 18 px / 1.3  | título de vista            |

Ningún texto bajo 12 px. Pesos: 400 cuerpo, 500 etiquetas, 600 títulos y nombres.

## Color

Tema oscuro **grafito** (dueño, 2026-11-13): sin azul. El acento es blanco cálido y el color queda solo para estados: verde ok, ámbar pendiente, rojo error. `.action.primary` es el acento sólido con texto `--text-on-accent`. Radios de 4 a 6 px, sin cápsulas. Claro y alto contraste conservan sus superficies y pierden el azul del acento.

Los tests de `tests/appearance.test.ts` comprueban que todo
token de texto ≥ 4,5:1 sobre **todas** las superficies opacas de su tema; `--line-strong` y
`--focus` ≥ 3:1 sobre todas. Los tonos `-soft` llevan su texto ≥ 4,5:1.

**Excepción: `--faint`.** El tenue del dueño, `#737373`, da 4,07:1 sobre `--bg` y entre 3,03 y 3,85:1 sobre las demás superficies del grafito. Se usa solo para metadatos (horas, rutas secundarias, contadores), nunca como único portador de información. Piso 3:1, comprobado en el mismo test.

| Tema             | chrome    | bg        | surface   | texto     | acento    |
| ---------------- | --------- | --------- | --------- | --------- | --------- |
| Oscuro (grafito) | `#151515` | `#0e0e0e` | `#151515` | `#ececec` | `#e8e3d8` |
| Claro            | `#ebebeb` | `#f5f5f5` | `#ffffff` | `#111111` | `#4a4337` |
| Alto contraste   | `#000000` | `#000000` | `#000000` | `#ffffff` | `#ffffff` |

Grafito, resto: controles `#1e1e1e`, hover `#232323`, presionado `#262626`, líneas `#2a2a2a`, bordes de control `#808080`, secundario `#a3a3a3`, tenue `#737373`, seleccionado `#232220`, texto sobre acento `#121212`, pendiente `#f5c044` (ámbar).

## Barra lateral, barras y Consola

- **Barra lateral:** columna de iconos de 52 px, fija, sin colapsar ni ensancharse, que ocupa todo el alto. Siete entradas: Consola, Workspace, Arsenal, Distribuidores, Actividad, Ajustes y **Más** (al pie). El nombre de cada una va en tooltip (`title`) y en texto oculto para lectores. La activa lleva `--accent-soft` y una barra de 3 px de acento.
- **Más** es un desplegable, no un menú ARIA: botón con `aria-expanded` y `aria-controls`, y una lista de botones en el orden de tabulación. Al abrirse el foco va al primer ítem; Escape lo cierra y devuelve el foco al botón, y un clic afuera también lo cierra. Lista cada vista de `navItems()` que la barra no abre: Sesiones, Agentes, Runtimes (solo con el protocolo 2 activo), Pipelines, PCs remotas, Aprobaciones, Eventos, Configuración, Modelos, Proveedores, Perfiles, Credenciales, Ajustes y Proyectos (solo si están activados). Una vista abierta desde Más ilumina Más.
- **Mientras se rediseñan**, Distribuidores abre Proveedores y Actividad muestra Aprobaciones arriba y Eventos abajo, sin cambios en esas vistas. El contador de aprobaciones pendientes va en Actividad y en Aprobaciones.
- **Sin barra superior.** Cada vista que no es la Consola lleva solo su nombre (`.context-title`), sin subtítulo. El nombre y la versión del producto quedan para lectores de pantalla.
- **Barra de estados** (`.status-bar`, abajo): Terminal, Agente y Autoridad en una línea, en toda vista menos la Consola. En la Consola queda montada y oculta, y la reemplaza el pie de sesión.
- **Barra de la Consola** (40 px): las pestañas a la izquierda; a la derecha, dividir al costado, dividir arriba/abajo, atajos (la lista de `keymap.ts`) y, tras un separador, **+ Lanzar**. Su menú lista solo los lanzamientos guardados, en el orden que el dueño les dio con **Order** (`localStorage`, `wyss-console.launch-order`), y los archivos rechazados con su motivo. Una terminal nueva se abre con Ctrl+Shift+T o con el botón del estado vacío.
- **Pie de sesión** (`.session-bar`), del panel enfocado: carpeta (como `~/…`, la ruta completa en el tooltip) · rama (o "HEAD suelta") · "✓ todo commiteado" / "● N cambios sin commitear" (N+ si la salida se cortó) · "↑N sin subir" (solo si la rama tiene upstream y N > 0) · "sin git" fuera de un repositorio. A la derecha: "Terminal", o "Nativa · {agente}" con el modelo de su perfil, o el nombre del perfil si no fija modelo. Si git no se pudo leer, muestra solo la carpeta. Se lee al cambiar el foco y cada 5 s mientras la Consola está a la vista, con una consulta por vez. Una respuesta que llega después de que cambió el foco se descarta.
- **De dónde sale el git:** `terminal_git_status { terminal_id }`. El escritorio pide al host la terminal y usa la carpeta actual de su proceso (`/proc/<pid>/cwd`) o, si no la puede leer, la de arranque; nunca una ruta de la ventana. Corre `git status --porcelain=v2 --branch --ignore-submodules=dirty` con `GIT_OPTIONAL_LOCKS=0`, `--no-optional-locks`, sin fsmonitor, hooks, pager, prompt ni lazy fetch, stdin cerrado, en su propio grupo de procesos y con 2 s de tope; al vencer, mata el grupo entero (`src-tauri/src/git_status.rs`). Si la configuración del repositorio (no la del dueño) define un filtro de contenido (`filter.*.clean|smudge|process`), no corre status y el pie muestra solo la carpeta. Si git se vence, la ventana espera 60 s antes de volver a preguntar en esa terminal. Con esto quedan cerradas las vías conocidas para que una carpeta haga correr sus comandos; frente a un `.git/config` hostil git no da garantías, y la exposición es la misma que la de un prompt que muestra git.
- **Botones:** 28 px, borde `--line`. `.action.primary` usa el acento sólido.
- **Pestañas de terminal:** texto con subrayado de 2 px en la activa.
- **Estado vacío de la Consola:** el mensaje y **Terminal nueva** centrados, sin recuadro.

## Copywriting

Inglés, como el resto de la app; sentence case; verbos que dicen lo que pasa. Los textos describen el estado real, la compatibilidad y la acción disponible.

## Contenido

| Elemento        | Valor                                                                      | Fuente                          |
| --------------- | -------------------------------------------------------------------------- | ------------------------------- |
| Navegación      | Consola · Workspace · Arsenal · Distribuidores · Actividad · Ajustes · Más | `RAIL_ITEMS` en `nav.ts`        |
| Compatibilidad  | Supported / Unverified / Unsupported con símbolo ✓ ? ✕                     | `compatibility.status`          |
| Datos de vistas | Los que devuelve el desktop                                                | Contratos `src/lib/contracts/*` |

## Estados

| Componente         | default                                                  | hover                        | focus visible                | active                                                                    | disabled                                | loading                                        | vacío                     | error                                 |
| ------------------ | -------------------------------------------------------- | ---------------------------- | ---------------------------- | ------------------------------------------------------------------------- | --------------------------------------- | ---------------------------------------------- | ------------------------- | ------------------------------------- |
| `.action`          | superficie + borde `--line-strong`                       | `--surface-hover`            | anillo `--focus`             | `--surface-strong`                                                        | opacidad .55 + `not-allowed`, sin hover | texto de la acción en curso (lo pone la vista) | —                         | —                                     |
| `.action.primary`  | `--accent-soft` + borde acento + texto `--accent-strong` | acento un paso más fuerte    | anillo                       | —                                                                         | igual a disabled                        | —                                              | —                         | —                                     |
| `.action.danger`   | `--error-soft` + borde error                             | más saturado                 | anillo                       | —                                                                         | igual a disabled                        | —                                              | —                         | —                                     |
| Ítem de navegación | icono `--muted`                                          | `--surface-hover` + `--text` | anillo interior              | `aria-current`: `--accent-soft` sólido + barra 3 px acento + icono acento | —                                       | —                                              | —                         | —                                     |
| Pie de sesión      | carpeta · rama · estado del git · agente                 | —                            | —                            | —                                                                         | —                                       | carpeta sola hasta que git responde            | sin sesión: no se dibuja  | git no leído: carpeta sola            |
| Campo / select     | `--surface-raised`, borde `--line-strong`                | borde `--muted`              | anillo                       | —                                                                         | fondo `--chrome`, texto `--faint`       | —                                              | placeholder `--faint`     | borde `--error-line` + mensaje debajo |
| Fila con estado    | franja según `data-tone`                                 | `--surface-hover`            | anillo                       | seleccionada: `--accent-soft` sólido                                      | —                                       | franja `pending`                               | `.empty-state` con acción | franja `error` + texto del error      |
| Insignia           | texto + símbolo + borde del tono                         | —                            | —                            | —                                                                         | —                                       | —                                              | —                         | —                                     |
| Terminal           | `--terminal-bg` sólido                                   | —                            | panel enfocado: borde acento | —                                                                         | —                                       | —                                              | `.terminal-empty`         | `.workspace-error`                    |

Ningún estado se comunica solo por color: siempre hay texto o símbolo.

## Breakpoints

Ventana de escritorio, `min-width: 320px`.

| Ancho        | Cambio                                                                                 |
| ------------ | -------------------------------------------------------------------------------------- |
| ≥ 1280 px    | barra lateral de iconos de 52 px en todo ancho; vistas a dos columnas donde ya lo eran |
| 1024 px      | igual; píldoras de estado pierden notas                                                |
| 768 px       | Workspace y Settings a una columna; filtros de Arsenal se envuelven                    |
| 640 px       | la barra de la Consola envuelve: pestañas en una fila, herramientas debajo             |
| 375 / 320 px | el pie de sesión y la barra de estados envuelven; sin scroll horizontal                |

Texto al 200 %: sin solapes; filas crecen en alto (`min-height`, nunca `height` fija). Rutas,
comandos y nombres largos usan `overflow-wrap: anywhere` o elipsis con `title`. Ambos temas en
cada ancho.
