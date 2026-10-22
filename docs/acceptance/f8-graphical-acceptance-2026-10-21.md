# F8: aceptación gráfica en KDE Wayland, 2026-10-21

## Resultado

**PASS en la parte gráfica de F8** sobre el ejecutable Tauri realmente instalado
(`~/.local/opt/wyss-console`), en la sesión KDE Wayland real, con estado aislado. Quedan fuera de
este informe, y siguen **NOT_PROVEN**, lo que nunca se autorizó: proveedor autenticado, refresco
de credenciales multiproceso, migración V7→V8 instalada y activación de F8.

- SHA inicial de esta etapa: `88cdd1c06a7500fe1863ae5e31835923616b3f31` (`feat/f8-production-runtime`).
- Un defecto de aplicación confirmado y corregido (contadores del encabezado); ver más abajo.
- El instalado contiene esa corrección: autoridad `01375cf3…c454b`, escritorio `747ed39c…72fc3`
  (SHA-256 completos al final). Se instaló con `--keep-host`; el rollback `.previous` se conserva.

## 1. Diagnóstico de las capturas negras

| Hipótesis                                          | Evidencia                                                                                                                                                                                                                          | Veredicto      |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| WebKit del producto no renderiza                   | El mismo binario instalado, en la sesión Wayland real, expone su árbol AT-SPI y devuelve contenido por `org.kde.KWin.ScreenShot2.CaptureWindow` (28 capturas, media 0,22–0,31, desvío 0,13–0,21; un frame negro tendría media ≈ 0) | **Descartada** |
| El binario anterior también falla                  | En el KWin virtual anidado ambos binarios dieron negro (`closeout/*-rendering-not-proven.png`); el entorno, no la versión, es el factor común                                                                                      | Entorno        |
| La captura falla al leer superficies Wayland       | `import`/captura X11 sobre Xwayland no ve superficies Wayland nativas, y en el KWin virtual sin GPU la composición de WebKitGTK no llegó a la captura                                                                              | **Causa**      |
| Cuál de las dos cosas pasa dentro del KWin anidado | No se aisló si el frame negro nace en el render o en la captura de ese entorno                                                                                                                                                     | NOT_PROVEN     |

Conclusión: el negro es propiedad del camino de captura/composición anidado, no del producto. El
camino válido es la captura de **una ventana** por su identificador con `CaptureWindow` (el
compositor real), pidiendo la interfaz restringida mediante un helper con
`X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`. Nunca se capturó pantalla completa ni
ventanas ajenas; el helper comprueba que el PID de la ventana sea el del ensayo. El helper y su
entrada de escritorio se retiraron al terminar.

## 2. Entorno y aislamiento

- Sesión Plasma Wayland real (`wayland-0`), ventana nativa Wayland (`GDK_BACKEND=wayland`) para los
  diálogos y los flujos; una pasada por Xwayland para poder escribir texto (`07-xwayland-real.png`).
- `HOME`, `XDG_*_HOME` y `XDG_RUNTIME_DIR` propios bajo `/tmp/wyss-f8v-*`; autoridad
  `wyss-control --protocol 2` propia con un runtime `codex` sintético (fixture Python) y una
  credencial sintética. No se usó Codex autenticado, red ni la base del dueño.
- Automatización: AT-SPI (`a11y.py`) para navegar y accionar controles; KWin scripting acotado al PID
  del ensayo para dar foco; teclado virtual `ydotool` (daemon con socket privado) **solo** para
  tipear el texto de los turnos, porque WebKit no expone edición por AT-SPI. Esa inyección de teclas
  ocurrió sobre la sesión real con el foco verificado en la ventana del ensayo.
- Procesos del dueño (autoridad 255925, host 256074, Codex 256140) no se tocaron; no se reinició
  ninguno ni se migró V7.

## 3. Resultados por punto

Capturas en `evidence/f8-graphical-2026-10-21/`.

| #   | Punto                                        | Resultado | Evidencia                                                                                                                                                                                                                         |
| --- | -------------------------------------------- | --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Navegación a Runtimes, protocolo 2 conectado | PASS      | `01-runtimes-wayland.png`: “Managed runtimes · 0 prepared · 0 closed · protocol 2”, Connected                                                                                                                                     |
| 2   | Crear sesión y runtime                       | PASS      | `02-session-runtime.png`, `03-runtime-ready-lease.png` (runtime Ready, modo Local Tools)                                                                                                                                          |
| 3   | Lease con confirmación nativa GTK (W1)       | PASS      | `04`/`05`: “Let this runtime ask to act?” con sesión, workspace, alcances y duración; “Don’t allow” no concede; “Allow” solo se habilita tras leer el texto completo                                                              |
| 4   | Workspace atado                              | PASS      | `06-bound-workspace.png`: Attached, Allowed ×3                                                                                                                                                                                    |
| 5   | Turno y aprobación de comando                | PASS      | `11`, `14`: tarjeta “Codex wants to run a command” con comando y directorio exactos, Accept/Decline/Cancel action                                                                                                                 |
| 6   | W1 del comando                               | PASS      | `12-command-w1.png`: “A runtime asks to run a command”; aceptar llegó una sola vez al fixture                                                                                                                                     |
| 7   | Rechazo de comando                           | PASS      | Decisión `decline` entregada al fixture sin subir el contador de ejecuciones; turno siguiente Completed (`text/declined-tree.txt`, `text/fixture-decisions.txt`). Texto de accesibilidad: no hay captura del instante del rechazo |
| 7b  | Id de aprobación repetido por el fixture     | PASS      | `13-wayland-decline.png`: la autoridad cerró la generación (sesión Failed, turno Indeterminate “Codex stopped unexpectedly”) en vez de aceptar un duplicado; el fixture se corrigió, la aplicación no se tocó                     |
| 8   | Aprobación de archivo y W1 de archivos       | PASS      | `15`, `16`: diff `+WYSS_FIXTURE_PATCH`, W1 “A runtime asks to change files”; solo se creó el archivo autorizado                                                                                                                   |
| 9   | Cancelación (acción y turno)                 | PASS      | `17-action-cancelled.png`: acción Cancelled, turno Interrupted (“Stopped at your request”), sin ejecuciones nuevas                                                                                                                |
| 10  | Revocar lease con aprobación pendiente       | PASS      | `18-lease-revoked.png`: “No longer applies — the execution lease of its turn was revoked”; campo bloqueado “There is no execution lease”                                                                                          |
| 11  | Catálogo de modelos                          | PASS      | `20-model-catalog.png`: lista del runtime sin afirmar elegibilidad                                                                                                                                                                |
| 12  | Caída de la autoridad                        | PASS      | `21-authority-offline.png`: Offline, acciones deshabilitadas, “no action is offered”                                                                                                                                              |
| 13  | Recovery con aprobación realmente pendiente  | PASS      | `24`, `25`: “The authority restarted or was replaced… nothing from before is acted on”; turno Indeterminate; sesión Stopped; sin proceso del runtime anterior                                                                     |
| 14  | Cierre de sesión y contadores                | PASS      | `27-close-confirm.png` → `28-session-closed.png`: “1 prepared · 1 closed”, sesión Closed, permisos “The session is closed”; con 2 sesiones el encabezado mostró “2 prepared · 0 closed” antes de cerrar                           |
| 15  | Limpieza                                     | PASS      | Harness `resume.py` detuvo GUI, host y autoridad propios; PIDs sobrantes de ensayos previos (2 hosts aislados, `ydotoold`) terminados por PID exacto verificando `exe` y `HOME`; entrada de escritorio retirada                   |

Decisiones recibidas por el fixture, en orden: `accept` (comando), `decline`, `accept` (archivo),
`cancel`; ejecuciones reales del comando: **1**. Ninguna decisión se duplicó. Textos de apoyo en
`evidence/f8-graphical-2026-10-21/text/`.

Observaciones que **no** son defectos del producto:

- Banner “The Wyss authority could not start — no user service manager answered” y chip
  “Authority: Failed to start”: el ensayo usa `--f8-authority` con un runtime dir sin bus de
  usuario, de modo que el arranque por el service manager del login no existe. La ventana Runtimes
  sí estaba “Connected”. Con la instalación real activada este camino no se ejercita aquí:
  **NOT_PROVEN**.
- `CRITICAL … ATK_IS_TEXT` en `gui.log`: aparece solo al correr `a11y.py`, que pide `GetText` a
  nodos sin interfaz de texto; coincide por hora con las lecturas del harness.

## 4. Defecto de aplicación corregido

Los contadores del encabezado (“N prepared · M closed”) se quedaban en los valores iniciales aunque
la lista de sesiones cambiara. `F8Controller.refresh()` leía sesiones pero no el estado de la
autoridad. Corrección de una línea (`observeState(await this.client.state())` antes de leer
sesiones, `src/lib/features/f8/controller.ts`).

- Regresión: `tests/f8-controller.test.ts` › “refreshes authoritative session counts after prepare
  and close”. **Roja sin la corrección** (`expected 1 to be 2`), verde con ella.
  El test de refrescos concurrentes añade `await settle()` porque `refresh()` ahora tiene un
  `await` más antes de leer sesiones.
- Confirmación gráfica: “0 prepared → 1 prepared → 2 prepared → 1 prepared · 1 closed” en vivo.

## 5. Validación tras el cambio

`npx vitest run tests/f8-controller.test.ts` 47 PASS; `npm run format:check`, `npm run lint`,
`npm run typecheck` (0 errores, 0 avisos) PASS. La suite frontend completa (913) y el paquete se
ejecutaron con exactamente este código antes de instalar; el cambio es solo frontend y no se repiten
las suites Rust, que no dependen de él.

## 6. Sin completar

- Proveedor autenticado, cuota y refresco multiproceso: NOT_PROVEN, no autorizado.
- Migración V7→V8 del estado instalado y activación de F8: no realizadas, no autorizadas.
- Escritura de texto sin teclado virtual (nativa por AT-SPI): no posible con WebKit; entrada probada
  con `ydotool`.
- ADR 0051: sigue Proposed; la acepta Santiago.

## Hashes instalados

- Escritorio `~/.local/opt/wyss-console/bin/wyss-console`:
  `747ed39cad9fad1d7b32dff85252ce28edf4e9d172b48d64292290febe772fc3`
- Autoridad `…/libexec/wyss-console/wyss-control`:
  `01375cf36e4075c0dd3f49e740420cc1daf338ae0ad205d252b52f6eac4c454b`
