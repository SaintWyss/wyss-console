# F8: instalación en el escritorio real y validación aislada, 2026-10-21

> Registro histórico. La instalación posterior, hashes y limitaciones actuales están en el
> [cierre independiente](f8-closeout-2026-10-21.md).

Rama `feat/f8-installed-wiring` sobre `main` `226e41b`. Cierra el cableado de producción que
el [runbook](f8-activation-runbook-2026-10-21.md) declaraba faltante, instala el paquete en la PC
del dueño sin cambiar el protocolo en uso y valida la ruta F8 de punta a punta con estado aislado.

**F8 NO está activo en producción.** El instalado sigue en protocolo 1 y SQLite V7. La
instalación contiene el cableado F8 inerte: sin `--f8-authority` la ventana no tiene Runtimes,
y no existe ninguna autoridad de protocolo 2 corriendo.

## Qué se cableó

| Pieza                    | Dónde                                          | Qué garantiza                                                                                                                                                                                                                                                                                                                 |
| ------------------------ | ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Selección explícita      | `wyss-control --protocol 2 …`                  | Sin argumentos sigue siendo la autoridad de protocolo 1. Protocolo 2 exige `--protocol 2 --runtime-root D --state-root D`; cualquier otra forma sale con código 2. No lee variables de entorno ni cae a protocolo 1.                                                                                                          |
| Imagen y almacén propios | `control/serve.rs`                             | Base `authority.sqlite3` (V8) distinta de `control.sqlite3`; un directorio con un almacén de protocolo 1 se rechaza. Se crea sólo con `--create` (vacía) o `--migrate-copy COPIA` (ensayo sobre una copia que el llamante hizo; la copia se lee, no se cambia); nunca sobre una imagen existente (`VACUUM INTO`).             |
| Socket privado local     | `control/serve.rs` (`bind` de protocolo 1)     | `<runtime-root>/wyss-console/control.sock`, 0600, pares del mismo uid; un listener vivo nunca se desvincula; mismos `control.lock` que protocolo 1 (una segunda autoridad sobre los mismos directorios, de cualquier versión, se rechaza).                                                                                    |
| Sin runtime              | `NoRuntime`                                    | La autoridad de producción prepara sesiones, concede leases y lee el historial, pero **no inicia ningún runtime** (`unsupported`). No hay lanzamiento de Codex en este código.                                                                                                                                                |
| Conector de la GUI       | `wyss-console --f8-authority <dir>`            | Única forma de activar la interfaz: la línea de comandos de quien lanza. Sin flag, `F8Gui::inactive()`. Un flag mal formado detiene el arranque (no se lee como "sin selección").                                                                                                                                             |
| Versión y esquema        | `F8Client::greet` / `compatible`               | Protocolo 2, esquema 8 **y** `service_version` igual a la versión del producto; si no, `incompatible`. Nunca reintenta en otro protocolo.                                                                                                                                                                                     |
| TOCTOU del socket        | `SocketConnector::connect`                     | El directorio se abre con `O_NOFOLLOW`/`O_DIRECTORY` y se verifica (dueño, 0700); se conecta por `/proc/self/fd/<dir>/control.sock`: el directorio no se puede cambiar entre comprobación y conexión. Resta la entrada del socket dentro de un directorio 0700 propio, que el uid de los pares cierra (mismo uid = frontera). |
| Capacidades              | `tauri.conf.json` lista `main` y `f8`          | Los 23 comandos de `f8` más `allow-f8-state`. Sin flag, todos responden `inactive` y no envían nada. Overlay `tauri.f8.conf.json` eliminado.                                                                                                                                                                                  |
| ACL aplicada por Tauri   | test `the_resolved_acl_grants_the_interface_…` | Con la ACL resuelta del build real: `main` alcanza el comando, una ventana de otra etiqueta recibe el rechazo del framework y no llega al comando.                                                                                                                                                                            |
| **W1**                   | `desktop/f8/confirm.rs`                        | Aceptar una aprobación y conceder/renovar un lease con comandos o archivos exigen **Permitir** en una ventana GTK que la página no puede dibujar, leer ni responder. Ver abajo.                                                                                                                                               |
| Autoridad candidata      | `wyss-f8-fixture` (feature `f8-fixture`)       | Mismo servidor con un runtime de fixture (no Codex) en bubblewrap. No está en ningún paquete (`check:package-fixtures` lo verifica); se instala aparte, en un prefijo aislado.                                                                                                                                                |

### W1: la confirmación nativa

La ventana no decide sola. Antes de que `f8_approval_accept` llegue a la autoridad, **Rust lee
la acción de la propia autoridad** (comando y directorio, o cada archivo y su diff completo),
la muestra en un diálogo GTK modal y envía la decisión sólo si el usuario pulsó Permitir.

- Lo mostrado es lo enviado: la revisión decidida es la leída por Rust y la de la página debe
  coincidir (si no, `stale`). Para un lease, los alcances y la vigencia mostrados son los enviados.
- Falla cerrada: sin ventana, sin respuesta (5 min), otro diálogo abierto, o un cambio de
  archivos que no cabe entero (16 KiB / 64 archivos) → `not_confirmed`; un cambio que no se puede
  mostrar entero sólo se puede rechazar.
- Sin hábito: Cancelar es la respuesta por defecto; Permitir está inerte ~1,2 s tras abrir
  (un clic dirigido a lo que había debajo no confirma); sin "no preguntar más"; una ventana a la
  vez. Una vez permitida, la misma acción reenviada (misma intención) no vuelve a preguntar
  durante 2 min.
- Los caracteres de control y de formato (RLO, ZWJ…) se muestran como `⟨U+XXXX⟩`.
- Rechazar y cancelar no piden confirmación: sólo reducen lo que puede pasar.

**Límite honesto:** el diálogo protege contra una _página_ comprometida. Un proceso hostil del
mismo usuario en un display X11 puede sintetizar clics (XTEST); bajo Wayland no. El uid sigue
siendo la frontera de confianza, igual que en protocolo 1.

## Qué cambió en la instalación real

| Elemento                             | Antes                                              | Después                                                                                           |
| ------------------------------------ | -------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `~/.local/opt/wyss-console`          | 0.2.0 del 20/10 (bin `0d9bcda5…`)                  | 0.2.0 de `feat/f8-installed-wiring`: bin `6c31bd38…`, authority `4dc2da4a…`                       |
| `~/.local/opt/wyss-console.previous` | build del 20/10 05:38                              | el build que estaba instalado (rollback con `install.sh rollback`)                                |
| Prefijo nuevo                        | —                                                  | `~/.local/opt/wyss-console-f8-candidate` (mismo paquete + `wyss-f8-fixture`), sin lanzador propio |
| Base `control.sqlite3` (V7)          | schema 7                                           | **sin tocar** (schema 7)                                                                          |
| Autoridad en ejecución               | `~/.local/share/wyss-console/authority` `29beda67` | **sin reiniciar**; la nueva (`4dc2da4a`) se instala la próxima vez que no corra ninguna           |

La GUI, el host (con sus terminales), la autoridad y el `codex app-server` del dueño estaban
abiertos durante la instalación (PIDs 255878/255925/256074/256140, arrancados a las 20:22, no por
esta tarea). Se instaló con `install.sh install --keep-host`: **no se interrumpió ninguno** y
siguen corriendo con la versión anterior hasta que el dueño los cierre. La versión nueva se
verá al reiniciar la ventana.

## Respaldo y rollback

Respaldo previo a la instalación, en `~/.local/state/wyss-console-backup-20261021-f8/` (0700;
`MANIFEST.sha256` con los hashes):

- `control.sqlite3`: `.backup` consistente de SQLite (integridad `ok`, `foreign_key_check` vacío,
  `user_version` 7; 2 sesiones, 77 eventos), sha256 `915eb7a2…`.
- `state-tree/` (anchor, layout, configuración, native, restauraciones), `config-wyss-console/`,
  `share-wyss-console/`, `opt-wyss-console/` (paquete instalado antes), `opt-wyss-console.previous-before-upgrade/`.

**El respaldo ya es anterior al estado vivo** (el dueño siguió usando sesiones: la base vive
tenía `head 78` a las 20:22). Se repite el respaldo inmediatamente antes de cualquier migración.

Rollback del binario instalado: `bash ~/.local/opt/wyss-console/share/wyss-console/install.sh rollback`
(intercambia con `.previous`; sin tocar la base). No hay rollback SQL: el de la base es restaurar el snapshot
(runbook, sección Rollback).

## Validación con estado aislado (no con datos instalados)

La GUI instalada (`6c31bd38…`, mismo binario que la instalada) se lanzó desde el prefijo candidato en
un display X11 anidado propio (`kwin_wayland --virtual` + Xwayland), con `HOME` y todos los
`XDG_*` aislados, contra una autoridad `wyss-f8-fixture` con base V8 temporal creada por
`--create` y directorios propios. Sin credenciales, sin Codex, sin tocar `~/.local/state/wyss-console`.

| #   | Prueba                                    | Resultado                                                                                                                                                  |
| --- | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Arranque de la GUI y funciones existentes | PASS: Console, terminal fish "Running", barra de estado, vistas de protocolo 1 intactas (el aviso "authority could not start" es esperado: sin systemd)    |
| 2   | Conexión y handshake de protocolo 2       | PASS: "Connected · protocol 2", esquema 8, vía Tauri ACL real                                                                                              |
| 3   | Crear y listar sesiones                   | PASS                                                                                                                                                       |
| 4   | Iniciar y detener un runtime contenido    | PASS (iniciar: Ready, gen 1, Local Tools, bubblewrap); detener: `f8_fixture_process` + test                                                                |
| 5   | Lease: conceder / revocar                 | PASS: concedido tras Permitir; "No permitir" no envía nada (`not_confirmed`); Permitir inerte los primeros ms. Revocar: `f8_fixture_process`               |
| 6   | Enlazar workspace                         | PASS (carpeta por el diálogo GTK nativo)                                                                                                                   |
| 7   | Turno y salida en vivo                    | PASS (streaming del fixture)                                                                                                                               |
| 8   | Aprobación nativa de comando/archivo      | PASS comando en GUI (diálogo con directorio y comando leídos de la autoridad); archivo con diff: tests (`approval_file`)                                   |
| 9   | Aceptar / rechazar / cancelar             | PASS aceptar y cancelar turno en GUI; rechazar y cancelar acción: tests de proceso                                                                         |
| 10  | Streaming, auditoría, recuperación        | PASS: SIGKILL de la autoridad → "Offline", sin acciones; reinicio → reconexión con aviso de nueva instancia, sesión persistida, runtime Stopped, sin hijos |
| 11  | Reinicio de la aplicación y reconexión    | PASS: cierre por SIGTERM, relanzado, sesión recuperada                                                                                                     |
| 12  | Limpieza de procesos y consumo            | PASS: sin restos (`ps`, mounts, sockets); ver mediciones                                                                                                   |
| —   | Redimensionado                            | PASS: 1400×880 y 560×760 (barra lateral se contrae a iconos, columnas apiladas)                                                                            |

### Ensayo de migración sobre una copia de los datos reales

`wyss-control --protocol 2 --migrate-copy` sobre una **copia** del respaldo (la base viva no se abrió):
V7 → V8 sin pérdida (2 sesiones, 77 eventos, 4 turnos, 2 aprobaciones, 11 leases, idénticos), `integrity_check`
`ok`, `foreign_key_check` vacío, `user_version` 8; la autoridad la sirvió y se detuvo limpia; la copia
quedó con el mismo sha256. Es un ensayo de datos, no la migración instalada.

### Mediciones (una máquina, display anidado, un runtime fixture; no son presupuesto)

| Proceso                                     | CPU ocioso (30 s)      | RSS                         |
| ------------------------------------------- | ---------------------- | --------------------------- |
| `wyss-f8-fixture` (autoridad)               | 0,4–0,5 % de un núcleo | 9,7 MB, 2–4 hilos, 9–14 fds |
| `wyss-console` (GUI)                        | 0,1–0,2 %              | 230–264 MB, 45–53 hilos     |
| `WebKitWebProcess` + `WebKitNetworkProcess` | ~0 %                   | 338 MB + 65 MB              |
| host de terminales                          | 0,07 %                 | 37 MB                       |

## Pruebas de código

Totales finales: `npm run check` (formato, lint, tipos 0/0, vitest 55 archivos / 911 tests, harness, build) PASS; `npm run check:rust` (fmt, check, test, clippy `-D warnings`; también con `f8-dev,f8-fixture`) PASS: lib 974, binario 361, boundary 1, contracts 10, `f8_serve_process` 8; `f8_fixture_process` 4 (con `--features f8-fixture`); `scan:repo` 5/5; `check:package-fixtures` PASS sobre el paquete nuevo. Nuevas: `tests/f8_serve_process.rs` (8, binario real),
`tests/f8_fixture_process.rs` (4, flujo completo y recuperación con el proceso real, `--features f8-fixture`),
`desktop/f8/confirm/tests.rs` (8), `f8/gui/client.rs` (4: versión, socket, enlaces simbólicos),
`desktop/select.rs` (3), `control/serve/tests.rs` (4) y el de ACL (1).

## Fronteras que siguen

NOT_PROVEN, sin cambios: proveedor autenticado (cero lanzamientos de Codex), refresco de credenciales con
varios procesos, migración **instalada**, selección de modelo/esfuerzo por sesión, confinamiento de descendientes
en el host real, benchmarks visuales, capacidad > 1. Nuevo: el diálogo W1 se vio en X11 anidado, no en la sesión
Wayland real; la autoridad de producción **no puede iniciar runtimes** (`NoRuntime`) hasta que se ensamble el
`NativeLocalToolsPlan` de producción (no existe fuera de pruebas) y se autorice.
