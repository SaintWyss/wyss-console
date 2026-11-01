# Wyss Console — contrato de UI global (superficies opacas)

Estado: vigente desde 2026-10-31 · rama `feat/global-ux-overhaul` · alcance: toda la ventana principal
(shell, navegación, Console, Workspace, delegaciones, Arsenal, Sessions, Agents, Runtimes,
Configuration, Settings y el resto de las vistas que heredan los componentes compartidos).

## Skills consultadas

UXP-3c8a6f

- `frontend-design` — `~/.claude/skills/frontend-design/SKILL.md`. Informó: la consola es una
  herramienta de trabajo; el carácter sale de la jerarquía de superficies, del acento cian ya
  reconocible y de estados honestos, no de decoración. La apuesta única es la **franja de estado**:
  una barra de 3 px a la izquierda de cada fila/tarjeta con estado (activo, verificado, fallido,
  en curso), que se repite igual en terminales, delegaciones y Arsenal. Falta validar: lectura en frío
  por el dueño.
- `ui-ux-pro-max` — `~/.claude/skills/ui-ux-pro-max/SKILL.md`. Informó: tema oscuro de
  herramienta de desarrollo con paleta pizarra, contraste AA calculado, foco visible, estado nunca
  solo por color, `prefers-reduced-motion`, escala tipográfica fija y espaciado de 4 px. Se descarta
  su patrón de landing (Hero + CTA), que no aplica a una app de escritorio.
- Estilo único de la capa 3: `minimalist-ui` — `~/.claude/skills/minimalist-ui/SKILL.md`.
  Se toma: superficies planas, borde de 1 px, radios ≤ 8 px, sin gradientes ni vidrio, sombras solo
  en capas flotantes, mono para metadatos, `kbd`. Se descarta: serif editorial, macro-espaciado,
  animaciones de entrada por scroll y manchas ambientales (chocan con densidad y opacidad).
- Capa 4 (motion): se omite `gsap-react` — el stack es Svelte sin React; el motion es CSS
  (150 ms, solo color/borde/opacidad) y se apaga con `prefers-reduced-motion`.
- Capa 0: sin referencia externa. La referencia es la propia consola antes del cambio
  (capturas en el scratchpad de la sesión) y este contrato.

## Referencia visual

Antes (medido en capturas del preview con un escritorio a rayas detrás): la ventana se crea
transparente en KDE y el marco usa `rgba(34,38,46,.7)`; todas las superficies son blancos al
3–12 % de opacidad. El wallpaper y las ventanas de atrás atraviesan barra lateral, barra superior,
tarjetas, campos y botones. En modo opaco los "orbes" de color se transparentan igual a través de
las tarjetas.

Después: qué domina → el contenido de la vista sobre una superficie sólida; qué se lee después →
el estado (insignias con forma y texto, franja de estado); qué hace reconocible la marca → el
acento cian `#5fd4f5` (oscuro) / `#005a80` (claro) solo en lo activo, seleccionado y en foco.

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

Ventana: siempre opaca. Se elimina la creación transparente y el pedido de blur a KWin
(`src-tauri/src/desktop/glass.rs`); el atributo `data-glass` deja de existir.

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

Contrastes calculados (script `contrast.py` en el scratchpad y `tests/appearance.test.ts`): todo
token de texto ≥ 4,5:1 sobre **todas** las superficies opacas de su tema; `--line-strong` y
`--focus` ≥ 3:1 sobre todas. Los tonos `-soft` llevan su texto ≥ 4,5:1.

| Tema           | chrome    | bg        | surface   | texto     | acento    |
| -------------- | --------- | --------- | --------- | --------- | --------- |
| Oscuro         | `#0b0f14` | `#10151c` | `#171e28` | `#e8edf4` | `#5fd4f5` |
| Claro          | `#e8ecf1` | `#f2f4f7` | `#ffffff` | `#121a25` | `#005a80` |
| Alto contraste | `#000000` | `#000000` | `#000000` | `#ffffff` | `#00ffff` |

## Copywriting

Inglés, como el resto de la app; sentence case; verbos que dicen lo que pasa. Se conservan los
textos existentes salvo donde la tarea es aclarar un estado (secciones de navegación, insignias de
compatibilidad, encabezados de grupos).

## Contenido

| Elemento                    | Valor                                                  | Fuente                                          | Aprobado                            |
| --------------------------- | ------------------------------------------------------ | ----------------------------------------------- | ----------------------------------- |
| Secciones de navegación     | Terminals · Agents & activity · Capabilities · System  | orden actual de `nav.ts` agrupado sin reordenar | sí (decisión delegada en el pedido) |
| Insignias de compatibilidad | Supported / Unverified / Unsupported con símbolo ✓ ? ✕ | `compatibility.status` real                     | sí                                  |
| Datos de vistas             | los que devuelve el desktop                            | contratos `src/lib/contracts/*`                 | sí; nada inventado en la app        |

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
