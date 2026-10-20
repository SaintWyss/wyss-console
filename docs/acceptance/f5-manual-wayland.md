# F5: prueba manual en el escritorio Wayland de Santiago

GUI instalada: revisión `2f503902946a`. **Guía reproducible.** La ejecución observada y sus límites se registran en
[f5-native-wayland-2026-10-20.md](f5-native-wayland-2026-10-20.md). Instalar/abrir no acredita aceptación. Usar la ventana nativa recién abierta, HOME y wallet normales. No abrir otro proveedor
ni mandar prompts. No cerrar pestañas de terminal, el host ni la autoridad.

Se prepararon dos carpetas reales nuevas, privadas y vacías:

- A: `~/wyss-console-f5-acceptance-20261020/A`
- B: `~/wyss-console-f5-acceptance-20261020/B`
- Exportaciones: `~/wyss-console-f5-acceptance-20261020/exports`
- Enlace deliberado para rechazo: `~/wyss-console-f5-acceptance-20261020/A-link`

No son otra instalación ni otro HOME: sólo proyectos de prueba dentro de la cuenta real.
Los archivos originales de los proveedores no se editan en esta secuencia.

## 1. Ventana y selección de proyecto

1. Abrí **Configuration**. Comprobá que aparece **Choose project…**.
2. Pulsalo; cancelá el selector nativo. Debe seguir sin proyecto y sin cambios escritos.
3. Volvé a abrirlo y elegí A. Debe mostrar **Project: A** y leer el inventario.
4. Probá elegir tu HOME, `~/.codex`, `~/.ssh`, `/tmp` y `A-link`, de a uno.
   Cada elección debe ser rechazada (`project_refused`) y conservar A seleccionado.
   No cambies permisos para lograr que una ruta pase.
5. Elegí B y después A. **No project** debe quitar la selección; elegí A nuevamente.

Resultado a registrar: diálogo nativo visible, cancelación sin efecto, A/B correctos y cada rechazo.

## 2. Crear, editar, eliminar y Undo exacto

Usá el filtro **Claude Code** y archivos del **proyecto A**, nunca **Your account**.

1. **New instruction file…** → ámbito **Project “A”** → **Claude Code CLAUDE.md** →
   **Continue…**. En el editor nativo escribí exactamente `F5-A-v1` seguido de Enter.
   Aceptá el editor. Todavía no debe existir el archivo: sólo hay un plan.
2. Cancelá ese plan. A debe seguir vacío. Repetí los pasos y esta vez pulsá
   **Continue to confirm** (o **Apply**, según el plan) y aceptá la confirmación nativa.
   Debe aparecer `A/CLAUDE.md`, con `F5-A-v1` y un salto de línea.
3. Seleccioná ese ítem → **Change it…**. Cambiá a `F5-A-v2` y Enter. Cancelá el editor:
   debe seguir v1. Repetí, aceptá el editor y confirmá el plan: ahora debe ser v2.
4. **History** → **Undo** del cambio v1→v2; revisá/confirmá el plan y el diálogo que corresponda.
   Deben volver los bytes de v1, no sólo un texto equivalente.
5. **Items** → seleccioná el archivo → **Delete…**. Cancelá el plan: debe seguir existiendo.
   Repetí y confirmá: debe desaparecer. **History** → **Undo** de esa eliminación:
   deben volver exactamente v1, propietario y permisos previos.
6. **History** → **Undo** de la creación original. A debe volver a estar vacío.
7. Repetí creación y Undo para **Claude Code rule (rules/…)**, nombre `f5-acceptance.md`,
   texto `F5-rule` y Enter. Luego **Claude Code skill**, nombre `f5-acceptance`, con este contenido válido (texto sin front matter se rechaza):

   ```markdown
   ---
   name: f5-acceptance
   description: Acceptance fixture only
   ---

   F5-skill
   ```

   Confirmá y deshacé la creación. Tras Undo deben desaparecer también las carpetas `.claude` creadas,
   siempre que sigan vacías. No borres manualmente nada para obtener el resultado esperado.

8. Probá **Codex AGENTS.md** en A con `F5-agents` y Enter; Undo debe restaurar ausencia.
   OpenCode comparte ese mismo `AGENTS.md`: probalo después del Undo, no simultáneamente.

Para comparar bytes y metadatos sin imprimir contenido, en una terminal distinta de la sesión
original, cuando exista el archivo:

```sh
sha256sum ~/wyss-console-f5-acceptance-20261020/A/CLAUDE.md
stat -c '%u:%g %a' ~/wyss-console-f5-acceptance-20261020/A/CLAUDE.md
```

Guardá el hash/modo de v1 antes de editar/eliminar y comparalo después de cada Undo.
Para comprobar ausencia final:

```sh
find ~/wyss-console-f5-acceptance-20261020/A -mindepth 1 -print
find ~/wyss-console-f5-acceptance-20261020/B -mindepth 1 -print
```

Ambos deberían no imprimir entradas tras deshacer todo. No usar `rm` como sustituto de Undo.

## 3. Cambiar proyecto con trabajo pendiente

1. En A prepará una creación de `CLAUDE.md`, pero no apliques el plan.
2. Elegí B. El plan de A debe descartarse; aplicar algo después no puede crear el archivo en B.
3. Volvé a A, creá el archivo y confirmá. Pasá a B y buscá Undo de ese cambio:
   debe rechazarlo (`not_supported`) o impedirlo. Volvé a A y deshacelo correctamente.
4. Durante una confirmación nativa abierta intentá cambiar proyecto sólo si el escritorio permite
   llegar a la ventana principal. Si está bloqueada o el botón deshabilitado, registrá eso:
   no acredita por sí solo la carrera concurrente. Si se admite el intento, B debe esperar y
   la escritura confirmada para A debe terminar únicamente en A. No inyectar IPC para simular clics.

## 4. Templates cifrados de hook y MCP

Esta secuencia verifica exportación, descifrado, prefilling y cancelación **sin aplicar cambios a
configuración global ni ejecutar un hook/servidor**. La aplicación confirmada de un template queda
separada y NOT_RUN si no se observa. Seleccionar A no vuelve project-scoped a hooks/MCP: esas altas
siguen escribiendo archivos de cuenta.

1. **Configuration** → seleccioná un hook existente de Claude Code que ofrezca
   **Export template…**. Exportá a `exports/hook.wyss-template` con una contraseña nueva escrita
   sólo en la ventana nativa. No uses ni compartas una contraseña real de otra cuenta.
2. **Add a hook…** → marcá **Start from a sealed template** → **Continue…**.
   Elegí el archivo exportado y primero probá una contraseña incorrecta: debe rechazarlo sin escribir.
3. Repetí con la contraseña correcta. Debe abrir el formulario nativo con evento, matcher, comando
   y timeout del template. Revisalos y **cancelá**. No debe aparecer una transacción aplicada.
4. Repetí cancelando, por separado, el selector de archivo y la petición de contraseña.
5. Elegí un MCP de Codex u OpenCode que ofrezca **Export template…**; exportá a
   `exports/mcp.wyss-template`. **Add an MCP server…** → nombre `f5-template-preview` →
   proveedor correspondiente → **Start from a sealed template** → **Continue…**.
   Probá contraseña incorrecta; después la correcta. El formulario debe mostrar programa/argumentos
   o dirección, según el template. Revisá y **cancelá**, sin iniciar ni conectar nada.
6. Intentá abrir el template MCP desde **Add a hook…**, y el de hook desde el alta MCP.
   Debe rechazar tipos cruzados sin aplicar nada. Un template de instrucciones tampoco es uno de hook/MCP.
7. Cancelá el diálogo nativo de guardado de una exportación: no debe crear un archivo nuevo.

Si no hay un hook/MCP exportable, registrá **NOT_RUN: no hay entrada compatible**. No crees una
entrada global mientras otros proveedores están activos para poder marcar una prueba como pasada.
Las pruebas completas de alta/aplicación requieren una tanda explícita con backup verificado,
coordinación con esos proveedores y Undo exacto de sus archivos originales.

## 5. Qué devolver

Anotá por paso **PASS**, **FAIL** o **NOT_RUN**, texto exacto del error, si se vio el diálogo nativo,
y hashes/permisos antes/después. Capturas opcionales sólo sin contenido secreto ni contraseñas.
Un PASS del operador se registra como observación manual de Santiago, distinto de una observación
automatizada. Cargar/ejecutar instrucciones, hooks o MCP en un proveedor sigue **NOT_PROVEN** salvo
que se observe por separado. No hace falta consumir cuota de modelos para estas pruebas.

La copia anterior está en `~/.local/opt/wyss-console.previous`; la copia que conserva el ejecutable
del host está en `~/.local/opt/wyss-console.retained-20261020-2f50390`. No ejecutar
`install.sh rollback`: detendría el host. Un rollback sólo de GUI debe reutilizar el instalador con
`install --keep-host`, conservando primero ambas copias; no requiere tocar la autoridad.
