# Arsenal: presets por provider y lanzamientos — contrato de diseño y de datos

Estado: **APROBADO por el dueño el 2026-11-11** (pantalla, grupos, textos y datos). Nada de esto está construido todavía. Hereda
sin excepciones el [contrato de interfaz](interface.md) (tokens, componentes compartidos, idioma).

Decisiones del dueño (2026-11-11), que este contrato no reabre:

1. **Preset + Lanzamiento.** Un preset es un kit de un provider. Un lanzamiento es agente + carpeta + permisos + un preset, se guarda desde Workspace y aparece en **+ Lanzar** (antes **Launch ▾**).
2. **Clasificación por reglas** deterministas sobre nombre y descripción, con corrección manual guardada.
3. **Panel de composición** fijo que muestra cómo queda armado el preset y cómo arranca la terminal.
4. En Arsenal **todo se elige por provider**.

## Flujo del dueño

1. Arsenal → elige provider → **New preset** → marca capacidades por grupo mientras el panel de composición muestra el resultado → **Save preset**.
2. Workspace → elige agente, carpeta, permisos y **Preset** → **Start** (inicio suelto) o **Save as launch…**.
3. - Lanzar → el lanzamiento guardado → arranca con ese preset aplicado.

## Modelo de datos

| Concepto                 | Qué es                                                                                                             | Dónde vive                                                                       |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- |
| Preset                   | Nombre, provider, capacidades (con revisión fija o actual), composites que se mantienen.                           | El registro `selection` que ya existe (tabla y cuerpo de sync **sin cambios**).  |
| Preset sin provider (v1) | La lista `preset` actual. Deja de mostrarse y de poder crearse.                                                    | Tabla `preset`: se conserva sin tocar; no se borra ni se deja de sincronizar.    |
| Grupo de una capacidad   | Uno de los grupos de la tabla de abajo. Regla, o corrección del dueño si existe.                                   | Reglas: código (`groups.ts`). Corrección: archivo `groups.json` junto a la base. |
| Lanzamiento              | `name`, `directory`, `[session] agent`, `profile`, `preset`. Sin `commands` ni `before` cuando lo crea la ventana. | Un archivo `~/.config/wyss-console/launch/<slug>.toml` (ADR 0059).               |

Reglas de datos:

- **El vocabulario cambia, el registro no.** `selection` pasa a llamarse _preset_ en la ventana, en los comandos nuevos y en los docs. El nombre de tabla, el cuerpo de sync y `arsenal_selection_*` no se renombran: la otra PC puede seguir en la versión anterior y sincronizar igual.
- **El esquema de `arsenal.sqlite` no cambia** (sigue en la versión 2). Una versión anterior de Wyss Console, o un host de terminales que siga corriendo la build previa, tiene que poder seguir abriendo la base; lo protege `the_library_schema_is_unchanged_so_an_older_version_still_opens_it`. Por eso lo nuevo vive en un archivo aparte.
- **`groups.json`**, en la carpeta privada de Arsenal, `0600`, escrito con archivo temporal + rename: `{"schema_version":1,"presets_migrated":bool,"groups":{"<identity>":"<group_id>"}}`. Un archivo ausente, ilegible, con otra forma o que no es un archivo regular se lee como vacío y no se modifica hasta el próximo cambio. Tope de 4096 correcciones.
- **Migración de los presets v1** (`Store::migrate_presets`, una vez por computadora: la marca es `presets_migrated`): por cada `preset` y por cada provider que aparezca entre sus capacidades, si no existe una `selection` con ese nombre y ese provider, se crea una con las capacidades de ese provider. El id se deriva del id del preset y del provider, así dos computadoras con el mismo preset crean la misma `selection` y la sincronización no la duplica. Nunca modifica ni borra una `selection` existente ni un `preset`. Tests: por provider, sin tocar existentes, mismo id en dos bases, repetida sin crear nada más.
- **Corrección de grupo**: una entrada de `groups.json` por `identity` de capacidad. Un `group_id` que esta versión no conoce se muestra en **Other** y se conserva.
- **Sync de la corrección**: fuera de esta entrega. La corrección queda en cada computadora y la ventana lo dice ("Saved on this computer"). Sincronizarla pide un registro de journal nuevo y comprobar antes qué hace la versión anterior con un tipo que no conoce; va al roadmap.
- **Plan de un borrador**: comando nuevo `arsenal_preset_plan` (provider, capacidades, composites) que responde lo mismo que `arsenal_preview` sin guardar nada. El panel de composición solo afirma lo que ese comando responde.
- **Guardar un lanzamiento**: comando nuevo `launch_save { name, directory, agent, profile, preset }`. El escritorio valida todo de nuevo (nombre ≤ 60, sin caracteres de control; carpeta absoluta existente; `agent` ∈ `claude_code | codex`; perfil y preset existentes para ese agente), deriva el nombre de archivo del nombre (minúsculas, `[a-z0-9-]`, ≤ 40), no sigue enlaces, escribe con archivo temporal + rename y permisos `0600`. Si el archivo existe responde `exists` y solo lo reemplaza con `replace: true`. La ventana **nunca** envía `commands` ni `before`. `launch_delete { id }` borra solo archivos que no tienen `commands` ni `before`.
- `SessionSpec` acepta `preset` (nombre del preset). Al lanzar, el preset nombrado gana sobre el recordado para la carpeta; si no existe para ese agente, el lanzamiento se rechaza con el motivo y no arranca nada. Una versión anterior de Wyss Console lista ese archivo como rechazado (campo desconocido): es el comportamiento aceptado mientras la otra PC no se actualice.

## Grupos

Cada capacidad pertenece a un solo grupo. Primera regla que coincide gana; se evalúa sobre `name` en minúsculas y, donde se indica, sobre la descripción. La corrección del dueño gana sobre toda regla.

| Orden | `group_id` | Etiqueta (en)        | Etiqueta (es)             | Regla                                                                                                                                                                               |
| ----- | ---------- | -------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | `models`   | Models and rules     | Modelos y reglas          | `kind = rule`; o agente llamado `astra_*`, `luna_*`, `sol_*`.                                                                                                                       |
| 2     | `security` | Security             | Seguridad                 | nombre contiene `security`, `cso`, `guard`, `careful`, `freeze`.                                                                                                                    |
| 3     | `debug`    | Debugging            | Depuración                | nombre contiene `debug`, `investigate`.                                                                                                                                             |
| 4     | `content`  | Content and SEO      | Contenido y SEO           | nombre empieza con `seo-`; o contiene `content-strategy`, `social-media`.                                                                                                           |
| 5     | `motion`   | Animation and video  | Animación y video         | nombre contiene `gsap`, `anime`, `animation`, `lottie`, `three`, `typegpu`, `waapi`, `hyperframes`, `remotion`, `contribute-catalog`.                                               |
| 6     | `design`   | Design and interface | Diseño e interfaz         | nombre contiene `design`, `-ui`, `ui-`, `ux`, `frontend`, `tailwind`, `shadcn`, `accessibility`, `image-to-code`, `taste`, `dashboard`.                                             |
| 7     | `study`    | Study and knowledge  | Estudio y conocimiento    | nombre contiene `tutor`, `quiz`, `cursos`, `second-brain`, `inbox`, `learn`.                                                                                                        |
| 8     | `decide`   | Decisions            | Decisiones                | nombre contiene `devils-advocate`, `fool`, `junta`, `council`, `ceo-review`, `advisor`.                                                                                             |
| 9     | `research` | Research             | Investigación             | nombre contiene `research`, `mapper`, `graphify`, `scrape`, `discovery`, `analyzer`, `intel`, `profiler`.                                                                           |
| 10    | `review`   | Review and testing   | Revisión y pruebas        | nombre contiene `review`, `verif`, `checker`, `auditor`, `qa`, `test`, `eval`, `health`, `benchmark`, `canary`, `playwright`.                                                       |
| 11    | `docs`     | Docs and writing     | Documentación y escritura | nombre contiene `doc-`, `document`, `markdown`, `humanizer`, `pdf`, `prompt`, `skill-creator`, `skillify`, `retro`, `report`.                                                       |
| 12    | `planning` | Planning and method  | Planificación y método    | nombre contiene `plan`, `roadmap`, `spec`, `office-hours`, `framework-selector`, `synthesizer`, `curator`; o es `arsenal`, `combo`, `loops`, `orient`.                              |
| 13    | `build`    | Building             | Construcción              | nombre contiene `executor`, `fixer`, `ship`, `deploy`, `fastapi`, `solid`, `typescript`, `react`, `vite`, `pair-agent`; o la descripción contiene `implement`, `build`, `scaffold`. |
| 14    | `system`   | System and setup     | Sistema y entorno         | nombre contiene `cachyos`, `workstation`, `doctor`, `setup`, `upgrade`, `ios-`, `context-`, `sync-`, `browser`.                                                                     |
| 15    | `other`    | Other                | Otros                     | lo que no coincide con nada.                                                                                                                                                        |

**Familia** es un segundo dato, independiente del grupo, tomado del prefijo del nombre: `gsd-` → GSD, `gstack-` → gstack, `seo-` → SEO, `hyperframes` → HyperFrames; sin prefijo conocido, ninguna. Se muestra como etiqueta y sirve de filtro.

**Para qué sirve** (una línea por capacidad): la primera oración de su descripción, hasta 110 caracteres. Hoy 30 capacidades llegan con descripción vacía o `>` / `>-` (escalar plegado de YAML mal leído) y los 59 agentes llegan sin ninguna. La fase de datos corrige la lectura del escalar plegado y lee `description` de los agentes; si aun así no hay texto, la fila muestra **No description in its file** y nada inventado.

Medido el 2026-11-11 sobre la biblioteca real (207 entradas, 173 nombres distintos): 1 nombre en Other (0,6 %) y ningún grupo vacío.

Aceptación de las reglas (test con los 207 nombres reales de la biblioteca del 2026-11-11 como fixture): toda capacidad recibe exactamente un grupo; **Other ≤ 10 %**; ningún grupo vacío; mismo resultado en dos corridas.

## Design System

Tokens y componentes de `interface.md`, sin tokens nuevos, sin hex, sin degradados, sin sombras salvo `--shadow-pop` en menús. Componentes nuevos, locales a Arsenal y hechos solo con esos tokens:

| Componente        | Qué es                                                                                                                                                |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `provider-switch` | Control segmentado de dos opciones (`role="radiogroup"`): Claude Code, Codex. Arriba de la vista, vale para las tres pestañas.                        |
| `group`           | Sección plegable (`button[aria-expanded]`): casilla de tres estados, etiqueta del grupo, `elegidas/total` en mono, flecha.                            |
| `capability-row`  | Casilla + nombre (600) + para qué sirve (`--muted`, una línea) + etiquetas: tipo, familia, compatibilidad (símbolo + texto). Menú **Move to group…**. |
| `composition`     | Panel fijo: nombre, provider, medidor por grupo, totales, composites, conflictos, cómo arranca, Guardar.                                              |
| `meter`           | Cinco celdas de 8×8 px por grupo: llenas con `--accent`, vacías con borde `--line-strong`. Siempre con el número al lado; nunca es la única señal.    |

La firma visual es el panel de composición: una ficha de equipo en mono, con un medidor por grupo, que cambia en el acto al marcar. Todo lo demás se mantiene callado.

## Referencia visual

Qué domina: el panel de composición (derecha). Qué se lee después: los grupos con su cuenta. Qué hace reconocible la pantalla: los medidores de cinco celdas y los totales en mono.

```
Arsenal  [ Claude Code | Codex ]                                             [Refresh]
 Presets   Library   Activity          Installed does not mean imported. A selection…
┌───────────────────────────────────────────────────────────────────────────────────┐
│ Presets [+ New preset]   Estudio 6   frontend 1   gsap-react 2   [Duplicate][Delete]│
└───────────────────────────────────────────────────────────────────────────────────┘
 Choose                                                  ┌ How it is put together ──┐
 [Search…]  (All)(Rules)(Skills)(Agents)                 │ Name [Estudio          ] │
 Family: (All)(GSD)(gstack)                              │ ■■■□□ Design          3  │
 ▾ [–] Design and interface                        3/12  │ 6 on · 37 off            │
    [x] frontend-design                                  │ [Save preset] [Discard]  │
 ▸ [ ] Debugging                                    0/5  └──────────────────────────┘
```

Workspace, tarjeta de inicio (cambia solo la fila de skills y el pie):

```
Agent   (Claude Code) (Codex) (OpenCode) (Hermes)
Folder  [~/Data/20_Knowledge/UTN_FRRe ▾] [Choose…]     Permissions [Estudio ▾]
Preset  [Estudio ▾]   6 on · 37 off · GSD Core kept     [Edit in Arsenal]
                                              [Save as launch…]  [Start]
Saved launches
  Estudiar   Claude Code · UTN_FRRe · Estudio · preset Estudio      [Start] [Delete]
```

## Spacing

Escala de 4 px de `interface.md`. Cabecera en una fila (título, provider, Refresh); la nota de alcance va en la fila de pestañas. Presets en una franja horizontal arriba; debajo, selector `minmax(0, 1fr)` sin borde y composición 340 px, separación 16 px. Bajo 900 px: una columna, composición antes del selector. Filas de capacidad: `min-height: 44px`, padding 8×12 px, separación 6 px. Cabecera de grupo: `min-height: 40px`. Celdas del medidor: 8 px con 2 px entre ellas. `box-sizing: border-box` en todo; ninguna altura fija.

## Typography

La de `interface.md`. Nombre de capacidad `--text-md` 600; para qué sirve `--text-sm` 400 `--muted`, una línea con elipsis y `title` con el texto completo; cuentas, totales y revisiones en `--font-mono` `--text-xs` con cifras tabulares; título del panel `--text-lg` 600. Nada bajo 12 px.

## Color

Solo tokens. El acento aparece en tres lugares: la opción activa de `provider-switch`, la fila de preset seleccionada (`--accent-soft` + barra de 3 px) y las celdas llenas del medidor. Compatibilidad y conflictos usan `.badge[data-tone]` con símbolo y texto. Contrastes: los cubre `tests/appearance.test.ts`; la fase de pantalla agrega al test el par celda llena / superficie (≥ 3:1) en los tres temas.

## Copywriting

Inglés como fuente, sentence case, a través de `t()`; español en `src/lib/i18n/es.ts`. Una acción conserva su nombre en todo el flujo (**Save preset** → "Preset saved"). Los textos dicen lo que el escritorio respondió, no lo que se supone.

## Contenido

| Elemento                          | Valor (en)                                                                                                                  | Valor (es)                                                                                                                                | Fuente                                       | Aprobado |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- | -------- |
| Pestañas                          | Presets · Library · Activity                                                                                                | Presets · Biblioteca · Actividad                                                                                                          | este contrato                                | sí       |
| Providers                         | Claude Code · Codex                                                                                                         | igual                                                                                                                                     | `PROVIDERS` en `contracts/arsenal.ts`        | sí       |
| Etiquetas de grupo                | tabla de Grupos                                                                                                             | tabla de Grupos                                                                                                                           | este contrato                                | sí       |
| Para qué sirve                    | primera oración de la descripción                                                                                           | no se traduce (texto del archivo)                                                                                                         | `revision.description`                       | sí       |
| Sin descripción                   | No description in its file                                                                                                  | Su archivo no trae descripción                                                                                                            | este contrato                                | sí       |
| Cuenta de grupo                   | `{chosen}/{total}`                                                                                                          | igual                                                                                                                                     | biblioteca + borrador                        | sí       |
| Totales                           | `{on} on · {off} off`                                                                                                       | `{on} activas · {off} apagadas`                                                                                                           | `arsenal_preset_plan`                        | sí       |
| Composite mantenido               | Keep {name} ({count} skills)                                                                                                | Mantener {name} ({count} skills)                                                                                                          | `discover.composites`                        | sí       |
| Cómo arranca                      | {agent} starts with {on} capabilities from this preset. The other {off} installed skills are switched off in that terminal. | {agent} arranca con {on} capacidades de este preset. Las otras {off} skills instaladas quedan apagadas en esa terminal.                   | `arsenal_preset_plan`                        | sí       |
| Cómo arranca, sin poder aplicarse | This preset is recorded but cannot be applied: {reason}                                                                     | Este preset queda registrado pero no se puede aplicar: {reason}                                                                           | `plan.enforcement` + motivo del escritorio   | sí       |
| Conflictos                        | `{count} conflict` / `{count} conflicts` + lista                                                                            | `{count} conflicto` / `{count} conflictos`                                                                                                | `plan.conflicts`                             | sí       |
| Acciones del preset               | New preset · Save preset · Discard changes · Duplicate · Delete                                                             | Nuevo preset · Guardar preset · Descartar cambios · Duplicar · Borrar                                                                     | este contrato                                | sí       |
| Corrección de grupo               | Move to group… · Back to automatic group                                                                                    | Mover a grupo… · Volver al grupo automático                                                                                               | este contrato                                | sí       |
| Workspace                         | Preset · None — all installed skills · Edit in Arsenal · Save as launch… · Saved launches                                   | Preset · Ninguno — todas las skills instaladas · Editar en Arsenal · Guardar como lanzamiento… · Lanzamientos guardados                   | este contrato                                | sí       |
| Nombre de lanzamiento sugerido    | `{agent} · {folder}`                                                                                                        | igual                                                                                                                                     | agente y carpeta elegidos                    | sí       |
| Preset con nombre repetido        | More than one preset is named “{name}” for this agent. Delete the extra ones in Arsenal, then start again.                  | Hay más de un preset llamado “{name}” para este agente. Borra los sobrantes en Arsenal y vuelve a iniciar.                                | este contrato (propuesto por el constructor) | no       |
| Lista de lanzamientos             | Saved launches · Reading launches… · No saved launches yet. · Terminal commands · {folder} · Preset {name}                  | Lanzamientos guardados · Leyendo lanzamientos… · Todavía no hay lanzamientos guardados. · Comandos de terminal · {folder} · Preset {name} | este contrato (propuesto por el constructor) | no       |
| Errores de lanzamientos           | The saved launches did not load. · The launch could not be deleted. · The launch could not be opened. · Starting…           | Los lanzamientos guardados no se cargaron. · No se pudo borrar el lanzamiento. · No se pudo abrir el lanzamiento. · Iniciando…            | este contrato (propuesto por el constructor) | no       |
| Preset sin presets para el agente | No presets for {agent}.                                                                                                     | No hay presets para {agent}.                                                                                                              | Estados, Preset en Workspace (este contrato) | sí       |

Ningún valor de esta pantalla es inventado: nombres, descripciones, versiones, cuentas y motivos salen del escritorio. Un texto nuevo que no esté en esta tabla se agrega acá antes de construirlo.

## Estados

| Componente               | default                                   | hover              | focus visible    | active / seleccionado                                                     | disabled                                                              | loading                                                         | vacío                                                                        | error                                                                                |
| ------------------------ | ----------------------------------------- | ------------------ | ---------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `provider-switch`        | dos opciones, una marcada                 | `--surface-hover`  | anillo `--focus` | `--accent-soft` + texto 600; cambia las tres pestañas                     | un provider no instalado: deshabilitado con "Not installed"           | —                                                               | —                                                                            | —                                                                                    |
| Lista de presets         | filas con nombre y cantidad               | `--surface-hover`  | anillo           | fila elegida: `--accent-soft` + barra 3 px                                | mientras se guarda                                                    | "Reading presets…"                                              | "No presets for {provider} yet." + **New preset**                            | `.notice[data-tone=error]` con el motivo y **Try again**                             |
| `group`                  | plegado, casilla según lo elegido, cuenta | `--surface-hover`  | anillo           | abierto: `aria-expanded=true`; casilla mixta si es parcial                | grupo sin capacidades del provider: no se dibuja                      | —                                                               | con búsqueda sin resultados: "No capabilities match."                        | —                                                                                    |
| `capability-row`         | sin marcar                                | `--surface-hover`  | anillo           | marcada: casilla + franja `ok`                                            | `unsupported` o de otro provider: no se puede marcar, dice por qué    | —                                                               | —                                                                            | compatibilidad `missing_dependency`/`conflicting`: franja `warn` + texto             |
| Mover a grupo            | menú con los 15 grupos, el actual marcado | `--surface-hover`  | anillo           | el elegido se aplica y la fila cambia de grupo                            | —                                                                     | "Saving…" en la fila                                            | —                                                                            | la fila vuelve a su grupo y muestra "Could not be saved. Nothing changed."           |
| `composition`            | medidores y totales del borrador          | —                  | —                | borrador con cambios: **Save preset** habilitado + "Unsaved changes"      | sin nombre: **Save preset** deshabilitado con ayuda "Name the preset" | "Checking…" en totales hasta que responde `arsenal_preset_plan` | sin capacidades: "Nothing chosen yet. Tick a group or a capability."         | el plan no respondió: totales ocultos + "Could not be checked." + **Try again**      |
| Guardar preset           | `.action.primary`                         | un paso más fuerte | anillo           | —                                                                         | sin nombre o sin cambios                                              | "Saving…"                                                       | —                                                                            | motivo del escritorio debajo del botón; el borrador se conserva                      |
| Borrar preset            | `.action` → pide confirmar en línea       | —                  | anillo           | "Delete {name}?" con **Delete** (`.danger`) y **Cancel** (foco inicial)   | —                                                                     | "Deleting…"                                                     | —                                                                            | motivo; el preset sigue en la lista                                                  |
| Preset en Workspace      | select con "None — all installed skills"  | borde `--muted`    | anillo           | elegido: resumen `{on} on · {off} off` al lado                            | agente sin Arsenal (OpenCode, Hermes): oculto                         | "Checking…"                                                     | sin presets para ese agente: "No presets for {agent}." + **Edit in Arsenal** | preset que no se puede aplicar: **Start** deshabilitado + motivo                     |
| Guardar como lanzamiento | `.action`                                 | `--surface-hover`  | anillo           | abre formulario en línea: nombre + resumen + **Save launch** / **Cancel** | sin agente o carpeta                                                  | "Saving…"                                                       | —                                                                            | `exists`: "A launch with that name exists." + **Replace** / **Cancel**; otro: motivo |
| Lanzamiento guardado     | fila con resumen                          | `--surface-hover`  | anillo           | **Start** lo arranca; **Delete** pide confirmar                           | preset o perfil que ya no existe: **Start** deshabilitado + motivo    | "Starting…"                                                     | "No saved launches yet."                                                     | motivo del escritorio en la fila                                                     |

Ningún estado se comunica solo por color. Todo cambio de totales se anuncia con `aria-live="polite"`. Cerrar la pestaña o cambiar de provider con cambios sin guardar pide confirmar (**Discard changes** / **Keep editing**).

## Breakpoints

| Ancho        | Cambio                                                                                                                                                                                                      |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ≥ 1280 px    | tres columnas: presets 220 px · selector · composición 320 px (fija, `position: sticky`).                                                                                                                   |
| 1024 px      | dos columnas: selector · composición 300 px; la lista de presets pasa a un select arriba del selector.                                                                                                      |
| 768 px       | una columna; la composición es una barra fija abajo con nombre, `{on} on · {off} off` y **Save preset**, que se despliega hacia arriba con los medidores.                                                   |
| 375 / 320 px | igual que 768; filtros de tipo y familia envuelven; etiquetas de la fila pasan debajo del nombre; sin scroll horizontal; la barra de composición no tapa la última fila (`padding-bottom` igual a su alto). |

Texto al 200 %: las filas crecen en alto; el para qué sirve pasa a dos líneas; las cuentas no se cortan. Nombres largos: `overflow-wrap: anywhere`. Objetivos táctiles ≥ 44 px. Los tres temas en cada ancho. `prefers-reduced-motion`: sin transición al plegar grupos ni al llenar celdas.

## Motion

Solo CSS (la app es Svelte, no aplica la capa de motion para React): plegar grupo 150 ms, llenado de celda 120 ms, `transform`/`opacity` únicamente. Nada más se anima.

## Ampliación del 2026-11-12 (BORRADOR, para charlar con el dueño)

Decisiones del dueño del 2026-11-12. Reabren lo que el contrato del 2026-11-11 decía sobre hooks, MCP y grupos:

1. **Ocho tipos en Elegir**, en este orden: Todos · Reglas · Skills · Agentes · Hooks · MCP · Plugins · Comandos. Al pasar el mouse sobre cada tipo aparece qué es.
2. **Todo se puede elegir**, incluidos los hooks de seguridad (`r3-gate`, control de ruteo). No hay piso fijo. Un preset puede abrir una terminal sin ningún hook: **decisión del dueño**, que este contrato registra y no discute.
3. **Cada terminal arranca con su propia configuración.** El global no se reescribe en cada lanzamiento: Arsenal le pasa a cada Claude Code los hooks y MCP del preset (`--settings`, `--mcp-config --strict-mcp-config`). Decidido el 2026-11-12 a propuesta del agente, en lugar de escribir el global antes de cada lanzamiento.
4. **GSD no se rompe.** Sigue siendo un paquete que va entero. En Claude, sus 35 agentes (links a `~/.local/share/wyss/gsd-core/<versión>/`) se muestran en Agentes con la etiqueta _parte de GSD Core_, y se prenden o apagan con "Mantener GSD Core".
5. **Categorías en 5 secciones**, todas visibles en cada tipo aunque estén vacías (con `0`).
6. **Categorías del dueño:** se pueden crear, renombrar y borrar. Las básicas no se borran.

### Secciones y categorías básicas

| Sección            | `group_id`                                          | es                                                                                     | en     | Cambio                                                                 |
| ------------------ | --------------------------------------------------- | -------------------------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------- |
| Cómo trabaja       | `rules`                                             | Reglas                                                                                 | Rules  | nueva: sale de `models` (`kind = rule`)                                |
|                    | `models`                                            | Modelos                                                                                | Models | queda solo con `astra_*`, `luna_*`, `sol_*`                            |
| Antes de construir | `research` · `decide` · `planning`                  | Investigación · Decisiones · Planificación                                             |        | igual                                                                  |
| Construir          | `build` · `design` · `motion` · `content` · `docs`  | Programar · Diseño e interfaz · Animación y video · Contenido y SEO · Docs y escritura |        | igual                                                                  |
| Después            | `review` · `debug` · `security`                     | Revisar y testear · Depurar · Seguridad                                                |        | igual                                                                  |
| Entorno            | `system` · `study` · `integrations` · `automations` | Sistema · Estudio · Integraciones · Automatizaciones                                   |        | nuevas: `integrations` = todo MCP; `automations` = hook sin otra regla |
| —                  | `other`                                             | Otros                                                                                  | Other  | igual, siempre al final                                                |

Una corrección guardada con `group_id = models` sobre una regla se lee como `rules`.

### Categorías del dueño

- Se guardan en `groups.json`: `"custom":[{"id":"u-<uuid>","name":"…","section":"before|build|after|env|how"}]`, con un tope de 64. `schema_version` pasa a 2, y si el archivo trae la versión 1 se lee igual.
- El nombre lleva de 1 a 40 caracteres, sin caracteres de control, y no puede repetirse dentro de la misma sección.
- "+ Nueva categoría" va al final de cada sección y abre una línea de texto en el mismo lugar. Renombrar y borrar están en el menú de la categoría y solo aparecen en las del dueño.
- Al borrar una categoría, sus correcciones se eliminan y las capacidades vuelven a la categoría que les da su regla. Antes se confirma: "Delete {name}? Its {n} capabilities go back to their default category."

### Hooks y MCP elegibles

> **Reemplazado el 2026-11-13** por [Decisión del 2026-11-13: home por lanzamiento](#decisión-del-2026-11-13-home-por-lanzamiento). La migración al catálogo y el `--settings` que se describen abajo ya no se implementan; queda solo lo que se elige y la regla de no copiar credenciales.

- **Qué se elige:** cada hook es una entrada `evento + matcher + comando`, y cada MCP es un servidor. El preset guarda la referencia (provider, tipo, clave estable = hash de evento+matcher+comando, o nombre del servidor), nunca una copia del comando ni de las variables del MCP, porque pueden tener credenciales.
- **Migración, una sola vez y con el OK del dueño:** respaldo con fecha de `~/.claude/settings.json` y `~/.claude.json` → todos los hooks y MCP pasan al catálogo maestro `~/.local/share/wyss-console/arsenal/catalog-claude.json` (`0600`) → en el global quedan solo los que el dueño marque como **default**, que por defecto son los que hoy están activos (fuera de la Console nada cambia). Antes de mover nada se muestra la lista.
- **Al lanzar:** Arsenal escribe en la carpeta privada de la terminal `hooks.json` (`{"hooks":…}` con lo que eligió el preset) y `mcp.json` (`{"mcpServers":…}`), y lanza `claude --settings hooks.json --mcp-config mcp.json --strict-mcp-config`. El global no se toca.
- **Límite:** `--settings` suma hooks al global y no puede sacar los que están ahí. Todo lo que se quiera poder apagar por preset tiene que quedar fuera del global (solo en el catálogo).
- **Probado el 2026-11-12 con Claude Code 2.1.323:** un hook `SessionStart` que llega por `--settings` corre, y con `--strict-mcp-config` y un `mcp.json` vacío la sesión arranca con 0 servidores MCP (sin el flag arranca con 18, contando plugins y conectores de claude.ai).
- **Lo que agregues a mano al global** lo cargan todas las terminales. Arsenal lo muestra en el panel como "en el global (siempre activo)" y ofrece pasarlo al catálogo.
- **Panel de composición:** lista los hooks activos. Si el preset deja afuera `r3-gate`, el panel muestra "Sin freno ante comandos destructivos" como dato, no como bloqueo.
- **Codex:** fuera de esta ampliación. Sus hooks están apagados a propósito.

### Comandos

- En Claude Code, los `/comando` propios hoy son skills (`~/.claude/commands` no existe en esta PC). Codex usa `~/.codex/prompts/*.md`, que tampoco existe todavía.
- El tipo Comandos escanea `~/.claude/commands/*.md` y `~/.codex/prompts/*.md`. Se importan y se eligen igual que una regla (un archivo). Si no hay ninguno, el estado vacío dice: "No custom commands. Your /commands are skills here."

## Decisión del 2026-11-13: home por lanzamiento

Decisión del dueño, a partir de la investigación [`docs/research/arsenal-home-por-lanzamiento.md`](../research/arsenal-home-por-lanzamiento.md) (Claude Code 2.1.323, codex-cli 0.167.0).

- **Aprobado como arquitectura de H:** cada lanzamiento tiene su propio home de configuración (`CLAUDE_CONFIG_DIR`; en Codex, `CODEX_HOME` con el `HOME` real). La composición por sección se regenera en cada lanzamiento desde el preset más los cambios volátiles. El global (`~/.claude`, `~/.codex`) no se modifica: quedan descartadas las alternativas A (filtro por hook) y B (mover hooks al catálogo).
- **El estado de sesión persiste:** el home vive en `~/.local/state/…`, no en `/tmp`, y no se borra al cerrar, porque ahí están las sesiones para retomar.
- **Claude Code:** validado como primer provider. **Codex:** validado salvo el cálculo de confianza de hooks; registrar esa confianza entra en H (dueño, 2026-11-13).
- **Criterio de aceptación:** una capacidad desmarcada no puede aparecer como aislada si el provider todavía la carga. Lo que no se puede aislar se muestra como "siempre carga": fuentes del repo, `cc-plugin-*`, `/etc/codex`.
- **Controles obligatorios del modo:** ningún preset los desactiva. Los hooks se clasifican como opcionales u obligatorios antes de implementar H.
- **Implementación de H: todavía no autorizada.** Pendientes: renovación de credenciales y cómo obtener el `trusted_hash` de un hook de Codex.
- **`r3-gate` queda apagado por decisión del dueño (2026-11-13).** No es un control obligatorio de ningún modo y Arsenal no lo ofrece por defecto.

## Fases de construcción

| Fase | Entrega                                                                                                                               | Verificación                                                                                                          |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| A    | Datos: migración de presets v1, `groups.json`, `arsenal_preset_plan`, comandos de grupo, descripción de escalar plegado y de agentes. | tests de Rust: migración, archivo de grupos, esquema de la base sin cambios; plan de borrador = preview del guardado. |
| B    | Reglas de grupo y familia (`groups.ts`) con el fixture de 207 nombres.                                                                | test del criterio de aceptación de Grupos.                                                                            |
| C    | Pantalla de Arsenal (este contrato), con `t()` desde el inicio.                                                                       | tests de vista por estado; capturas a 1280 / 1024 / 768 / 375 en los tres temas contra la referencia.                 |
| D    | Workspace: selector de preset, **Save as launch…**, lanzamientos guardados; `launch_save`, `launch_delete`, `preset` en el TOML.      | tests de Rust de validación y de no escribir `commands`; test de vista; un lanzamiento real de punta a punta.         |

| E | Ampliación: los 8 tipos en Elegir (hooks, MCP, plugins y comandos solo para ver) + agentes GSD en Claude. | test de vista por tipo; test del escaneo de comandos. |
| F | Las 5 secciones y las categorías nuevas, siempre visibles. | test de reglas con el fixture (Other ≤ 10 %); test de vista con categorías vacías. |
| G | Categorías del dueño en `groups.json` v2. | tests de Rust: v1→v2, tope, nombre repetido, borrado que devuelve a la regla. |
| H1 | Home por lanzamiento en Claude Code: el host arma `launch-homes/<preset>` (links a agents/skills/plugins/output-styles/hooks, copia de `settings.json` y `.claude.json`, nunca `.credentials.json`) y lanza con `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_OAUTH_TOKEN` (credencial _Anthropic_ del llavero de la Console). Sin elección de entorno, el preset sigue lanzando como hoy. | tests de Rust del armado (allowlist, 0600, sin credencial, idempotente); calificación 2.1.323: un home "con todo" iguala el inventario del global (165 skills, 45 agentes, 10 plugins, 18 MCP, 6 hooks de inicio) y arranca interactivo sin pedir login. |
| H2 | Entorno del preset (`environment.json` junto a la base, sin cambiar el esquema): instrucciones (CLAUDE.md + `rules/` por archivo), agentes, skills de la cuenta (`anthropic-skills:*`), hooks (clave = sha256 de evento+matcher+comando), MCP (`--strict-mcp-config` + `mcp.json` 0600 con los elegidos), plugins apagados, memoria. | tests de Rust: filtro de hooks, otras claves de `settings.json` intactas, MCP solo elegidos; calificación: home mínimo ≤ 14,5k tokens iniciales (medido 14.002 contra 68.063 del global, 2026-11-13, sin skills, agentes, MCP, hooks, plugins ni skills de la cuenta) y cada desmarcado ausente del inventario. |
| H3 | Pantalla: hooks, MCP, plugins y reglas pasan de "Todavía no se elige" a elegibles; el panel de composición muestra "siempre carga" (repo, `cc-plugin-*`). | tests de vista; una terminal real con preset mínimo y otra con preset completo abiertas a la vez. |
| H4 | Codex: `CODEX_HOME` por lanzamiento con el `HOME` real, skills copiadas, `[[skills.config]]` por skill no elegida, `[features]` apps/plugins/remote_plugin apagados (plugins de Codex: todo o nada), hooks con confianza registrada. | calificación codex 0.167.0 con los casos de la investigación; token de Codex sin renovar desde el home (pendiente de probar su escritura). |

Cierre: una revisión externa del conjunto, con foco en `launch_save` (escritura de archivos nombrados por la ventana), en la migración y, para H, en el paso del token del llavero a la terminal y la copia de `.claude.json`.

## Skills consultadas

Clave de lectura: `UXP-3c8a6f`.

| Skill             | Ruta                                        | Qué decisión informó                                                                                                                                                                                                               | Qué falta validar                                            |
| ----------------- | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `frontend-design` | `~/.claude/skills/frontend-design/SKILL.md` | Una sola firma (el panel de composición con medidores) y el resto callado; nombres desde el lado del dueño (preset, lanzamiento); vacíos y errores que dicen qué hacer.                                                            | Que el medidor de cinco celdas se lea bien con 15 grupos.    |
| `ui-ux-pro-max`   | `~/.claude/skills/ui-ux-pro-max/SKILL.md`   | Divulgación progresiva (grupos plegados), una acción primaria por pantalla, confirmación antes de borrar, error junto al control, `aria-live` en totales, objetivos ≥ 44 px, sin color como única señal.                           | Lista de 100+ filas sin virtualizar: medir antes de decidir. |
| `minimalist-ui`   | `~/.claude/skills/minimalist-ui/SKILL.md`   | Superficies planas con borde de 1 px, color como recurso escaso, sin degradados ni sombras. Se descartó lo que es de página de marketing: serif, macro-espaciado, animaciones de scroll, paleta crema (chocan con `interface.md`). | Nada: el estilo ya es el de la app.                          |

Capas omitidas: referencia robada (no hay sitio ni mockup externo; la referencia es el esquema de arriba) y motion con librería (stack Svelte, motion en CSS).
