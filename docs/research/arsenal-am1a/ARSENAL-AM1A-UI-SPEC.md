# Arsenal AM1a — contrato de UI

Estado: vigente para AM1a · 2026-10-30 · alcance: biblioteca (descubrir, importar, revisiones, compuestos, presets, selecciones, vista previa) y plan por terminal en Workspace. La exclusión por sesión se aplica solo en un build calificado; en cualquier otro el plan se muestra y se registra como `NOT APPLIED`.

## Skills consultadas

UXP-3c8a6f

- `frontend-design` — `/home/saintwyss/.claude/skills/frontend-design/SKILL.md`. Informó: la vista es utilitaria y densa; el carácter lo da la jerarquía de la biblioteca y los estados honestos, no decoración nueva. Falta validar: captura real en tema claro y oscuro.
- `ui-ux-pro-max` — `/home/saintwyss/.claude/skills/ui-ux-pro-max/SKILL.md`. Informó: objetivos táctiles de 44 px, foco visible, texto de cuerpo de 16 px en ancho de teléfono, estados vacío/carga/error, `aria-live` para errores y `prefers-reduced-motion`. Falta validar: recorrido completo por teclado.
- Estilo único de la capa 3: `kpi-dashboard-design` — `/home/saintwyss/.claude/skills/kpi-dashboard-design/SKILL.md`. Motivo: vista de datos densa con conteos, estados y tablas legibles. No cambia la paleta: se sostienen los tokens existentes de la consola.

## Referencia visual

La referencia es la propia consola: `src/app.css`, tokens en `:root` (tema oscuro) y `:root[data-theme='light']`. Lo que domina la pantalla es la lista de capacidades; lo que se lee después es el estado de cada una (`no importada`, `importada`, `cambió en origen`); la marca reconocible es el acento cian sobre superficies translúcidas. No se agregan tokens, degradados ni sombras nuevas.

## Design System

Se usan solo tokens que ya existen en `src/app.css`. Ningún valor hexadecimal ni píxel suelto en componentes nuevos.

- Superficies: `--surface`, `--surface-raised`, `--surface-hover`, `--surface-strong`.
- Texto: `--text`, `--muted`, `--faint`. Estado: `--ok`, `--warn`, `--error`, con sus variantes `-soft` y `-line`.
- Acento: `--accent`, `--accent-soft`, `--accent-line`, `--text-on-accent`.
- Radios: `--radius-sm` (8 px) para filas y campos, `--radius-md` (10 px) para paneles, `--radius-lg` (14 px) solo para el contenedor de la vista.

## Spacing

Escala de 4 px: 4, 8, 12, 16, 24. Separación entre filas de la lista: 8 px. Entre secciones de un panel: 16 px. Padding de paneles: 16 px en escritorio, 12 px bajo 768 px. Espacio mínimo entre objetivos táctiles: 8 px.

## Typography

Familia: la del sistema (`system-ui, sans-serif`); mono solo para rutas, hashes y contenido de archivos (`--font-mono`).

| Rol                          | Escritorio (>= 768 px)            | Teléfono (< 768 px)               |
| ---------------------------- | --------------------------------- | --------------------------------- |
| Título de la vista           | 20 px, peso 650                   | 18 px, peso 650                   |
| Título de sección            | 15 px, peso 600                   | 16 px, peso 600                   |
| Cuerpo y filas               | 14 px, peso 400, interlineado 1,5 | 16 px, peso 400, interlineado 1,5 |
| Etiquetas, badges, metadatos | 12 px, peso 500                   | 13 px, peso 500                   |
| Hashes y rutas               | 12 px mono                        | 13 px mono                        |

La regla por breakpoint es una sola por rol: nada de cuerpo queda por debajo de 14 px en escritorio ni de 16 px en teléfono.

## Color

Pares texto/fondo calculados por par real (WCAG, luminancia relativa). Todos superan 4,5:1.

| Par                | Oscuro | Claro |
| ------------------ | ------ | ----- |
| texto / fondo      | 16,59  | 15,24 |
| muted / fondo      | 11,04  | 8,08  |
| faint / fondo      | 8,95   | 6,52  |
| accent / fondo     | 11,47  | 7,12  |
| ok / fondo         | 12,79  | 6,17  |
| warn / fondo       | 11,80  | 7,47  |
| error / fondo      | 7,65   | 6,91  |
| texto sobre accent | 11,47  | 7,57  |

Ninguna información depende solo del color: cada estado lleva palabra y badge.

## Copywriting

Inglés, como el resto de la consola. Frases cortas, verbos directos, sin relleno. Un control dice lo que hace («Import», «Restore», «Save preset»); un mensaje dice qué pasó y qué hacer.

## Contenido

| Elemento                     | Valor                                                                                                              | Fuente                                                             | Aprobado |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------ | -------- |
| Título de la vista           | Arsenal                                                                                                            | `src/lib/shell/nav.ts`                                             | sí       |
| Pestañas                     | Library · Discover · Composites · Presets · Selections · Terminals                                                 | esta spec                                                          | sí       |
| Aviso de alcance             | Installed does not mean imported. A selection reaches a terminal only when you start one with it from Workspace.   | esta spec                                                          | sí       |
| Estado de importación        | Not imported · Imported · Source changed                                                                           | respuesta de `arsenal_discover`                                    | sí       |
| Estado de compatibilidad     | Supported · Unsupported · Unverified · Needs provider qualification · Conflicting · Missing dependency             | `compatibility` del backend (`arsenal/analysis.rs`)                | sí       |
| Estado de aplicación         | NOT APPLIED en la vista previa de una selección; por terminal: APPLIED o NOT APPLIED, y siempre «Use not observed» | `arsenal_preview`, `arsenal_terminal_bind` (estado leído del host) | sí       |
| Compuerta del plan           | Can be applied · Not qualified for this build · Build not identified · Installed skills not fully read             | `enforcement.state` de `arsenal_terminal_plan`                     | sí       |
| Estado de cada ítem del plan | prepared · rejected · not carried · missing                                                                        | `items[].state` del plan                                           | sí       |
| Dependencias                 | Tool (declared, not checked offline) · MCP server · File; present / missing / unknown                              | `dependencies` del backend                                         | sí       |
| Mensajes de error            | texto por código (`storage_unavailable`, `blocked`, …)                                                             | `src/lib/features/arsenal/messages.ts`                             | sí       |

## Estados

| Componente                | Default                             | Hover                   | Focus visible      | Active            | Disabled                                  | Loading                                                        | Vacío                                              | Error                                             | Resultado esperado                                                                                               |
| ------------------------- | ----------------------------------- | ----------------------- | ------------------ | ----------------- | ----------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Pestañas                  | texto muted                         | fondo surface-hover     | anillo accent 2 px | fondo accent-soft | —                                         | —                                                              | —                                                  | —                                                 | cambia la sección; `aria-selected`                                                                               |
| Buscador                  | borde glass-line                    | borde glass-line-strong | anillo accent      | —                 | —                                         | —                                                              | —                                                  | —                                                 | filtra en vivo, sin tocar el backend                                                                             |
| Fila de capacidad         | texto + badges                      | fondo surface-raised    | anillo accent      | fondo accent-soft | —                                         | —                                                              | —                                                  | —                                                 | abre el detalle; `aria-pressed`                                                                                  |
| Botón Import              | action                              | surface-raised          | anillo accent      | surface-strong    | mientras importa, o si está bloqueada     | «Importing…»                                                   | —                                                  | mensaje con motivo                                | importa o explica el bloqueo                                                                                     |
| Botón Restore             | action                              | surface-raised          | anillo accent      | surface-strong    | si ya es la revisión actual               | «Restoring…»                                                   | —                                                  | mensaje                                           | mueve el puntero; no crea revisión                                                                               |
| Lista de descubrimiento   | —                                   | —                       | —                  | —                 | —                                         | «Reading installed skills…»                                    | «No skills found in your Claude or Codex folders.» | «Discovery is unavailable. Refresh to try again.» | lista o estado                                                                                                   |
| Presets y selecciones     | action                              | —                       | —                  | —                 | guardar sin nombre                        | «Saving…»                                                      | «No presets yet. Create one from a capability.»    | error de validación debajo del campo              | guarda o indica el campo                                                                                         |
| Vista previa              | —                                   | —                       | —                  | —                 | —                                         | «Building preview…»                                            | «Add capabilities to see the configuration.»       | «The library no longer holds this capability.»    | siempre muestra `NOT APPLIED`                                                                                    |
| Pin de revisión           | «Follow current»                    | —                       | anillo accent      | —                 | —                                         | —                                                              | sin ítems: no se muestra                           | —                                                 | guarda `revision` del ítem                                                                                       |
| Compuestos (pestaña)      | tarjeta                             | —                       | —                  | —                 | —                                         | «Reading installed composites…»                                | «No composite collection is installed…»            | aviso de Discover                                 | identidad, versión, miembros, límites                                                                            |
| Terminals (pestaña)       | fila por terminal                   | —                       | anillo accent      | —                 | —                                         | —                                                              | «No terminal has started with a selection yet.»    | mensaje por código                                | APPLIED / NOT APPLIED + «Use not observed»                                                                       |
| Panel Arsenal (Workspace) | select «None — skills as installed» | —                       | anillo accent      | —                 | sin agente o proveedor sin soporte: texto | «Building the terminal plan…» / «Checking the launch preview…» | «No saved selection for this provider.»            | `role="alert"` con el motivo                      | Start habilitado solo con plan aplicable y preview que puede arrancar, o con «Start without applying it» marcado |

Ningún mensaje afirma algo que la UI no detecta. Un fallo de descubrimiento no dice «no hay skills»; dice que no se pudo leer.

## Breakpoints

| Ancho           | Cambio                                                                                                                                    |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 320 px          | Columna única. Pestañas en fila con desplazamiento horizontal contenido dentro de la barra. Filtros apilados. Detalle debajo de la lista. |
| 375 px          | Igual que 320; nombres largos truncados con elipsis y texto completo en `title`.                                                          |
| 768 px          | Dos columnas: lista y detalle. Filtros en una fila.                                                                                       |
| 1024 px y mayor | Tres zonas: filtros, lista, detalle.                                                                                                      |

Reglas comunes: `box-sizing: border-box`; objetivos táctiles de al menos 44 px de alto en botones y filas; sin pérdida de acciones con texto ampliado a 200%; nombres largos con `overflow-wrap: anywhere`; probado en tema claro y oscuro.
