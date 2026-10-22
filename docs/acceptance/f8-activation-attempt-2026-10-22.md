# F8: primer turno con Codex real, causa y corrección (2026-10-22)

> **Historical record.** For current status use the [gap register](../product/implementation-status.md)
> and [remaining roadmap](../product/roadmap.md). F8 was subsequently activated in production
> with real Codex, P2/V8, ADR0051 Accepted and cap1; P1 remains. The report below retains
> its original observations and limitations; its procedures are not new authorization.

Estado: **el turno con Codex 0.159.0 real funciona de punta a punta en una GUI aislada.** F8 sigue
**sin activar**: la consola instalada del dueño (protocolo 1, SQLite V7) no cambió, no se paró, no se
reinició y no se migró. ADR 0051 sigue _Proposed_.

## Causa raíz

Tras `thread/archive` y `thread/unarchive`, Codex 0.159.0 deja el hilo **sin cargar**
(`thread/status/changed → notLoaded` en la transcripción real) y rechaza `turn/start` sobre él con
`-32600` hasta que llega un `thread/resume`. El adaptador nativo de F8 materializa el hilo con
`start → archive → unarchive` y daba el binding por confirmado al contestar `unarchive`; el primer
`turn/start` caía entonces en `ProviderRejected` y la autoridad cerraba la generación
(`runtime.instance_exited · protocol_failed`, 25 ms después de `managed_turn.admitted`).

- `turn/moderationMetadata` **no** era la causa: el test que reproduce frame a frame una transcripción real
  de 0.159.0 (`local_tools_real_0_159_turn_is_accepted_frame_by_frame`) acepta todas las notificaciones.
- La corrección está en `src-tauri/src/f8/provider_adapter.rs`: la respuesta de `unarchive` ya no publica
  el binding; envía `thread/resume` con los **mismos** parámetros que `thread/start` y que el resume de
  siempre (nueva `thread_params()` compartida) y el binding se publica sólo cuando esa respuesta pasa
  las comprobaciones existentes (postura Local Tools o inferencia, `cliVersion` igual a la del lanzador,
  `cwd` igual al workspace, `ephemeral == false`, id del hilo esperado).

## Seguridad: lo que la corrección no cambia

- Local Tools sigue en `approvalPolicy=untrusted`, `workspace-write`, sin red, sin raíces escribibles
  extra y sin perfil de permisos activo; la raíz de confianza es sólo el workspace. El resume usa la misma
  `thread_params()` que el inicio, así que el perfil no puede divergir.
- Identidad (sesión gestionada, instancia de runtime) viene del worker, no del proveedor; el hilo se fija
  por `expected` y una respuesta con otro id es `CorrelationError`.
- Id `f8-N` monótonos, tope de 16 pendientes; la entrada de `unarchive` se retira antes de registrar el
  resume, y `operation` se usa una sola vez. Una respuesta repetida no encuentra pendiente.
- Tests nuevos: id/cwd/ephemeral alterados y un error JSON-RPC **nunca** enlazan (`native_unarchive_resume_that_answers_wrongly_never_binds`),
  y el resume se dirige al id realmente generado, no a uno fijo (`native_unarchive_resumes_thread_before_binding`).
- Revisión de seguridad independiente (Codex, sólo lectura, sobre este diff): **sin hallazgos**
  CRITICAL/HIGH/MEDIUM/LOW. Sugirió cubrir respuestas incorrectas, duplicadas y ausentes: las dos primeras
  están en el test anterior; la ausente se cubre porque sin respuesta no hay evento que enlace.

## Regresión de integración

El fixture `src-tauri/tests/fixtures/codex_runtime.py` ahora se comporta como el real: tras
`archive`/`unarchive` el hilo queda sin cargar y `turn/start` devuelve `-32600` hasta un `thread/resume`.
Sin la corrección **4 tests de `f8_codex_process` fallan** (`the_real_process_starts_the_isolated_runtime…`,
`a_file_change_is_written_only_when_accepted`, `a_decision_codex_did_not_offer…`,
`a_runtime_that_crashes_is_reported_failed…`); con ella pasan. Transcripciones reales anonimizadas:
`tests/fixtures/codex-0.159.0-local-turn.jsonl` y `codex-0.159.0-resume.jsonl`.

## Prueba real con Codex 0.159.0 (GUI aislada)

Paquete construido desde este árbol (`npm run desktop:package`; autoridad `2443a7ae…8546e`), **sin
instalarlo**. Display anidado aislado (KWin virtual + Xwayland `:77`), `HOME`/`XDG_*` propios, autoridad
`wyss-control --protocol 2 --create` con `--codex …/0.159.0-…/bin/codex --allow-credential`, y una
**copia** 0600 de `auth.json` en un codex-home propio (la original no se abrió para escribir). Capturas en
`evidence/f8-codex-real-2026-10-22/`.

| Paso                                                                               | Resultado | Evidencia                                                                                                                                                                           |
| ---------------------------------------------------------------------------------- | --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Runtimes, protocolo 2 conectado                                                    | PASS      | pantalla de arranque; "Connected · protocol 2"                                                                                                                                      |
| Runtime Local Tools **Ready**, versión 0.159.0                                     | PASS      | `01-…`                                                                                                                                                                              |
| Lease con diálogo nativo W1 (sesión, workspace, alcances, 5 min) → Allow           | PASS      | `02-…`, `03-…`                                                                                                                                                                      |
| Binding del workspace (archive → unarchive → **resume**) → Attached · Local Tools  | PASS      | `04-…`; auditoría: _Session bound to a workspace_ 16:36:37                                                                                                                          |
| Turno real ("Reply with the single word OK…") → admitido → running → **Completed** | PASS      | `05-…`, `06-…`: admitted/running 16:37:18, completed 16:37:21                                                                                                                       |
| Parar runtime, cerrar sesión ("0 prepared · 1 closed")                             | PASS      | `07-…`, `08-…`                                                                                                                                                                      |
| Limpieza                                                                           | PASS      | sesiones propias del rig terminadas por sid verificado; copia de `auth.json` y caché borradas; workspace temporal borrado; display y montaje portal del intento anterior terminados |

- La copia de `auth.json` tiene **el mismo SHA-256 antes y después**: no hubo refresco de token en esta
  corrida. El `auth.json` real no se tocó (mtime 2026-10-13).
- PIDs del dueño (autoridad 255925, host 256074, fish 256102, Codex 256140): sin cambios.

Observaciones del rig y del producto (no bloquean):

- Un codex-home de prueba **sin `models_cache.json`** hace fallar el arranque con "The runtime process could
  not be started" (la autoridad lo exige para el catálogo Local Tools). Es del rig; el mensaje no dice qué falta.
- El primer botón **Run** tras vincular devolvió "The authority is busy with another operation"; el
  reintento admitió el turno de inmediato. Un solo turno quedó registrado. No se investigó la causa.
- Banner "Authority: Failed to start": el rig no tiene service manager de usuario; la ventana estaba
  "Connected". Con la instalación real activada ese camino **sigue NOT_PROVEN**.

## Validación

`npm run check:rust` (fmt, check, tests de librería 987+, integración 147, clippy `-D warnings`) PASS;
suite frontend, paquete y escaneo: ver registro de cierre del commit.

## Qué falta para activar (todo del dueño)

1. ADR 0051 sigue _Proposed_; lo acepta Santiago.
2. El instalado (`~/.local/opt/wyss-console`: escritorio `747ed39c`, autoridad `01375cf3`) **no tiene esta
   corrección**: hay que instalar el paquete nuevo en una ventana autorizada.
3. Migración V7 → V8 de la base viva y parada/relanzamiento de `wyss-control.service` y del host de la
   consola (cierra la terminal fish del dueño): no hechos; respaldo vigente en
   `~/.local/state/wyss-console-backup-20261022-f8/`.
4. Refresco de credenciales con varios procesos: **NOT_PROVEN** (el daemon de la app, 0.163.0, comparte
   `~/.codex/auth.json`; el access token real vence ~2026-10-24).
5. Un proveedor autenticado en producción necesita proveedor/ADR/migración aceptados; `max_active_runtimes = 1`.
