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

| Token                                                  | Uso                                               |
| ------------------------------------------------------ | ------------------------------------------------- |
| `--chrome`                                             | barra superior y barra lateral                    |
| `--bg`                                                 | lienzo de la vista                                |
| `--surface`                                            | tarjetas, paneles, diálogos, filas                |
| `--surface-raised`                                     | campos, filas anidadas, menús                     |
| `--surface-hover`                                      | hover de filas y botones                          |
| `--surface-strong`                                     | presionado, pestaña activa                        |
| `--accent-soft`                                        | seleccionado (sólido, nunca degradado)            |
| `--line`                                               | divisores y bordes de tarjeta                     |
| `--line-strong`                                        | bordes de controles (≥ 3:1 sobre toda superficie) |
| `--focus`                                              | anillo de foco de 2 px con 2 px de separación     |
| `--ok/-soft/-line`, `--warn…`, `--error…`, `--pending` | estados                                           |
| `--shadow-pop`                                         | solo menús, diálogos y toasts flotantes           |

Ventana siempre opaca, sin transparencia, blur de KWin ni atributo `data-glass`.

Componentes compartidos (clases globales, no por vista): `.action` (+ `.primary`, `.danger`,
`.quiet`), `.badge[data-tone]`, `.state-stripe` (`[data-tone]` en la fila), `.field` (label +
control + ayuda), `.panel`, `.empty-state`, `.notice[data-tone]`.

## Spacing

Escala de 4 px: 4, 8, 12, 16, 24, 32. Padding de panel 16 px (12 px bajo 720 px). Separación entre
filas de lista 6–8 px. Separación mínima entre controles 8 px.

## Typography

Familia del sistema (`system-ui`), mono `--font-mono` para rutas, comandos, ids, hashes y versiones.

| Token       | Tamaño       | Uso                        |
| ----------- | ------------ | -------------------------- |
| `--text-xs` | 12 px / 1.4  | insignias, metadatos, pies |
| `--text-sm` | 13 px / 1.45 | texto secundario, ayudas   |
| `--text-md` | 14 px / 1.5  | cuerpo, controles, filas   |
| `--text-lg` | 16 px / 1.4  | títulos de sección         |
| `--text-xl` | 18 px / 1.3  | título de vista            |

Ningún texto bajo 12 px. Pesos: 400 cuerpo, 500 etiquetas, 600 títulos y nombres.

## Color

Oscuro y claro siguen los temas Warp Dark y Warp Light (fondo, texto, acento y ANSI del oscuro); las superficies son grises neutros. El acento claro es más oscuro que el `#00c2ff` de Warp para cumplir 4,5:1 sobre blanco.

Los tests de `tests/appearance.test.ts` comprueban que todo
token de texto ≥ 4,5:1 sobre **todas** las superficies opacas de su tema; `--line-strong` y
`--focus` ≥ 3:1 sobre todas. Los tonos `-soft` llevan su texto ≥ 4,5:1.

| Tema           | chrome    | bg        | surface   | texto     | acento    |
| -------------- | --------- | --------- | --------- | --------- | --------- |
| Oscuro         | `#1c1c1c` | `#121212` | `#1c1c1c` | `#ffffff` | `#00c2ff` |
| Claro          | `#ebebeb` | `#f5f5f5` | `#ffffff` | `#111111` | `#00678a` |
| Alto contraste | `#000000` | `#000000` | `#000000` | `#ffffff` | `#00ffff` |

## Barra lateral y controles

- Jerarquía de superficies del tema oscuro, medida en Warp: barra lateral y barra superior `#1c1c1c`, contenido y terminal `#121212`, paneles `#1c1c1c`, controles `#242424`. La barra lateral ocupa todo el alto de la ventana; la barra superior solo la columna del contenido.
- El botón de abajo de la barra lateral la colapsa a una columna de iconos (52 px). Colapsada, se abre sobre la vista (232 px, 180 ms) mientras el puntero o el foco de teclado están encima, sin empujar el contenido. La preferencia se guarda en `localStorage` (`wyss-console.sidebar`); si no se puede leer, arranca abierta. La lista de vistas se desplaza sin dibujar barra de desplazamiento. La barra lateral no tiene cabecera: empieza con los estados.
- Arriba de la lista van los estados (Terminal, Codex, Authority), uno por línea; colapsada queda el punto y el texto sigue leyéndose para lectores de pantalla. La versión no se dibuja: la leen los lectores de pantalla.
- Cada grupo (Terminals, Agents & activity, Capabilities, System) se pliega con un clic en su título. Un grupo plegado se abre al pasar el puntero y queda abierto al elegir uno de sus ítems o al volver a hacer clic en el título. Plegar con clic cierra el grupo en el acto, aunque el puntero siga encima, hasta que el puntero salga.
- Barra superior, en la Console: título y subtítulo en una línea, y a la derecha, centrados, cuatro botones de 28 px: dividir en dos columnas, dividir en dos filas, atajos de teclado y **Launch ▾**. Su menú (vertical) lista **New terminal**, **Start agent…** y los lanzamientos de `~/.config/wyss-console/launch`, en el orden que el dueño les dio con **Order** (flechas ▲▼; se guarda en `localStorage`, `wyss-console.launch-order`).
- Botones: 28 px, borde `--line`, sin acento. `.action.primary` usa superficie neutra.
- Pestañas de terminal: texto con subrayado de 2 px en la activa; no son botones con borde ni hay línea de base. Sin pestañas no se dibuja la franja.
- Estado vacío de la Console: el mensaje centrado en toda el área de contenido, sin recuadro.

## Copywriting

Inglés, como el resto de la app; sentence case; verbos que dicen lo que pasa. Los textos describen el estado real, la compatibilidad y la acción disponible.

## Contenido

| Elemento        | Valor                                                  | Fuente                          |
| --------------- | ------------------------------------------------------ | ------------------------------- |
| Navegación      | Terminals · Agents & activity · Capabilities · System  | `nav.ts`                        |
| Compatibilidad  | Supported / Unverified / Unsupported con símbolo ✓ ? ✕ | `compatibility.status`          |
| Datos de vistas | Los que devuelve el desktop                            | Contratos `src/lib/contracts/*` |

## Estados

| Componente         | default                                                  | hover                        | focus visible                | active                                                                    | disabled                                | loading                                        | vacío                     | error                                 |
| ------------------ | -------------------------------------------------------- | ---------------------------- | ---------------------------- | ------------------------------------------------------------------------- | --------------------------------------- | ---------------------------------------------- | ------------------------- | ------------------------------------- |
| `.action`          | superficie + borde `--line-strong`                       | `--surface-hover`            | anillo `--focus`             | `--surface-strong`                                                        | opacidad .55 + `not-allowed`, sin hover | texto de la acción en curso (lo pone la vista) | —                         | —                                     |
| `.action.primary`  | `--accent-soft` + borde acento + texto `--accent-strong` | acento un paso más fuerte    | anillo                       | —                                                                         | igual a disabled                        | —                                              | —                         | —                                     |
| `.action.danger`   | `--error-soft` + borde error                             | más saturado                 | anillo                       | —                                                                         | igual a disabled                        | —                                              | —                         | —                                     |
| Ítem de navegación | texto `--muted`                                          | `--surface-hover` + `--text` | anillo interior              | `aria-current`: `--accent-soft` sólido + barra 3 px acento + icono acento | —                                       | —                                              | —                         | —                                     |
| Campo / select     | `--surface-raised`, borde `--line-strong`                | borde `--muted`              | anillo                       | —                                                                         | fondo `--chrome`, texto `--faint`       | —                                              | placeholder `--faint`     | borde `--error-line` + mensaje debajo |
| Fila con estado    | franja según `data-tone`                                 | `--surface-hover`            | anillo                       | seleccionada: `--accent-soft` sólido                                      | —                                       | franja `pending`                               | `.empty-state` con acción | franja `error` + texto del error      |
| Insignia           | texto + símbolo + borde del tono                         | —                            | —                            | —                                                                         | —                                       | —                                              | —                         | —                                     |
| Terminal           | `--terminal-bg` sólido                                   | —                            | panel enfocado: borde acento | —                                                                         | —                                       | —                                              | `.terminal-empty`         | `.workspace-error`                    |

Ningún estado se comunica solo por color: siempre hay texto o símbolo.

## Breakpoints

Ventana de escritorio, `min-width: 320px`.

| Ancho        | Cambio                                                                                         |
| ------------ | ---------------------------------------------------------------------------------------------- |
| ≥ 1280 px    | navegación con secciones y etiquetas (232 px); vistas a dos columnas donde ya lo eran          |
| 1024 px      | igual; píldoras de estado pierden notas                                                        |
| 768 px       | Workspace y Settings a una columna; filtros de Arsenal se envuelven                            |
| 720 px       | navegación pasa a riel de iconos de 60 px con `title`; secciones ocultas, separadores visibles |
| 375 / 320 px | barra superior solo marca + estado; acciones de toolbar envuelven; sin scroll horizontal       |

Texto al 200 %: sin solapes; filas crecen en alto (`min-height`, nunca `height` fija). Rutas,
comandos y nombres largos usan `overflow-wrap: anywhere` o elipsis con `title`. Ambos temas en
cada ancho.
