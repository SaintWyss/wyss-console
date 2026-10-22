# F8: procedimiento de activación y rollback (preparado, NO ejecutado)

Estado: **preparado y ensayado en aislado; nada de esto se aplicó a la instalación del dueño.** Requiere
la aprobación explícita de Santiago (ADR 0051, interrupción, migración y uso autenticado).

## Qué se investigó

- **"Busy" transitorio.** Es el diseño del primer turno: la autoridad prueba el runtime con un
  `config/read` antes de admitir (`NativeCodexWorker::preflight` devuelve `RuntimeOperationInProgress`
  hasta que el runtime contesta) y la ventana lo mostraba como error. Corregido en
  `src/lib/features/f8/controller.ts`: `startTurn` repite la petición con **la misma identidad**
  (hasta 10 s, cada 300 ms), igual que el binding; al vencer, muestra "busy" y conserva el texto.
  Dos tests nuevos en `tests/f8-controller.test.ts`.
- **Refresco de credenciales con varios procesos.** Sin defecto confirmado en Wyss. El `auth.json` real
  nació el 2026-07-27 y se modificó por última vez el 2026-10-13 **con el mismo inode**: Codex escribe en
  el lugar, así que el bind por descriptor del runtime aislado y `credential_holds` (que compara inode)
  siguen valiendo tras un refresco de cualquier proceso. Que dos procesos Codex (el daemon 0.163.0 y el
  runtime 0.159.0) refresquen a la vez es comportamiento de Codex, el mismo que hoy ya existe entre el daemon y
  el hijo de la consola. Probarlo de verdad exigiría rotar el refresh token real: **no se hizo**; sigue
  NOT_PROVEN y es parte de lo que el dueño acepta.

## Piezas (en el repositorio)

- `scripts/install.sh install … --f8-service --codex RUTA_ABSOLUTA [--allow-credential]`: verifica que el
  ejecutable responda exactamente `codex-cli 0.159.0`, escribe `wyss-console-f8.service` (unidad de usuario
  propia, **junto a** `wyss-control.service`, que no se toca) y cambia el lanzador a
  `wyss-console --f8-authority $XDG_RUNTIME_DIR/wyss-console-f8`. `--codex` y `--allow-credential` sin
  `--f8-service` se rechazan. Sin `--allow-credential` la credencial no se liga.
- `scripts/f8-activation.sh backup | rehearse | migrate | rollback`.
  - `backup`: respaldo en línea (API de backup de SQLite, la base viva sólo se **lee**), integridad,
    FK, versión 7, recuentos y SHA-256 en `MANIFEST.json`.
  - `rehearse`: migra una copia del respaldo en un directorio descartable y lo borra.
  - `migrate`: crea `~/.local/state/wyss-console-f8/wyss-console/authority.sqlite3` (V8) con
    `--migrate-copy`; nunca reemplaza una base existente; comprueba que cada tabla V7 conserva sus filas.
  - `rollback`: `systemctl --user disable --now wyss-console-f8.service` y `install.sh rollback`.

## Ensayo en aislado (2026-10-22)

HOME y `XDG_RUNTIME_DIR` propios y un `systemd --user` privado; datos reales sólo como copia.

| Comprobación                                                                                                                                                                                           | Resultado |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------- |
| Respaldo fresco de la base viva (`…-backup-20261022-170022-f8`): V7, integridad ok, 0 FK, 78 eventos, 11 leases, 2 sesiones, 4 turnos, 48 intents, mismo SHA que el respaldo anterior (nadie escribió) | PASS      |
| `rehearse` con la autoridad empaquetada: V8, integridad ok, 0 FK, todas las tablas conservan filas, directorio descartado                                                                              | PASS      |
| `install.sh install --f8-service …`: unidad y lanzador correctos; rechaza `--codex` sin `--f8-service`, un ejecutable que no sea 0.159.0 y una ruta relativa                                           | PASS      |
| `migrate` en el HOME aislado + `systemctl --user start wyss-console-f8.service`: activo; `…/wyss-console-f8` 0700, `control.sock` 0600                                                                 | PASS      |
| `kill -KILL` de la autoridad → systemd la relanza (NRestarts=1) y sirve de nuevo; `restart`; `stop` limpio y el directorio de runtime desaparece; nuevo `start` sobre la misma base                    | PASS      |

Con el runtime real y la credencial real se probó antes (2026-10-22, GUI aislada): Ready → lease → binding →
turno → Completed → cierre (ver el informe del primer turno).

## Secuencia de activación (tras la aprobación)

La autoridad de protocolo 1 (`wyss-control.service`) y su base V7 **no se paran ni se modifican**: el
protocolo 2 usa otra unidad, otro socket y otra base. El dueño decide aparte cuándo retirar el protocolo 1.

1. `scripts/f8-activation.sh backup` (otra vez, justo antes) y guardar la ruta.
2. Mover aparte, sin borrar, la base de pruebas anterior: `mv ~/.local/state/wyss-console-f8 ~/.local/state/wyss-console-f8.attempt-20261022`.
3. `install.sh install --package .artifacts/package/… --keep-host --f8-service --codex ~/.codex/packages/standalone/releases/0.159.0-x86_64-unknown-linux-musl/bin/codex --allow-credential`
   (`--keep-host`: el host y sus terminales siguen con su versión hasta que el dueño los cierre).
4. `scripts/f8-activation.sh migrate --backup RUTA --authority ~/.local/opt/wyss-console/libexec/wyss-console/wyss-control`.
5. `systemctl --user daemon-reload && systemctl --user enable --now wyss-console-f8.service`.
6. El dueño abre la ventana nueva (lanzador actualizado): Runtimes debe decir "Connected · protocol 2" y listar
   las 2 sesiones migradas. Reiniciar el **host** (cierra la terminal fish) es opcional y sólo del dueño.
7. Un turno real con la credencial real; reinicio del servicio y recuperación (turno en vuelo → Indeterminate,
   sin nada actuado); limpieza.

## Rollback (probado en parte)

1. `scripts/f8-activation.sh rollback` → deshabilita y para `wyss-console-f8.service`, `install.sh rollback`
   restaura el paquete anterior y el lanzador sin `--f8-authority`.
2. La base V7 **no se tocó**, así que no hay nada que restaurar. Sólo si se hubiera modificado:
   parar `wyss-control.service`, copiar `control.sqlite3` del respaldo (0600) y arrancarlo; se pierden las
   escrituras posteriores al respaldo.
3. La base V8 se conserva (nunca se borra en un rollback).

Probado en aislado: parada/arranque/recuperación de la unidad. **No** probado sobre la instalación real:
`install.sh rollback` con una versión anterior (lo ejerció la instalación del 21/10 con `--keep-host`) y la
restauración de V7 (innecesaria mientras el protocolo 1 siga intacto).

## Riesgos que el dueño acepta al aprobar

- Refresco de credenciales con varios procesos: NOT_PROVEN (ver arriba).
- El Codex fijado en 0.159.0 vive en `~/.codex/packages/standalone/releases/`, que la propia aplicación de
  Codex actualiza: si borra esa versión, la autoridad se niega a lanzar (no hay otro ejecutable aceptado).
- Una sola autoridad de protocolo 2 y `max_active_runtimes = 1`.
- ADR 0051 pasa a _Accepted_ sólo por decisión de Santiago.
