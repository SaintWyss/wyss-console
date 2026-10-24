# Revisión independiente parcial — SAFE STOP

Fecha: 2026-10-24. Revisor independiente: agente `final_security_review`, sin autoría de los cambios. Comparación consultada: baseline `48396c41d0263687ea72b8392f4cbdbd46038d1a` contra el árbol de trabajo correspondiente a `8eed1be` y retoques en curso. El coordinador informó HEAD de cierre `f042e0db96bde1b57d736bd324046aeb95df674b`; esos cambios posteriores quedan NOT_PROVEN. **Sin aprobación de producción.** Se interrumpe por pedido explícito de SAFE STOP.

## Alcance efectivamente revisado

Lectura enfocada de `f8/v9.rs`, `f8/provider.rs`, `f8/claude/handoff.rs`, `bin/wyss-credential-agent.rs`, `credentials/core.rs`, `activation.rs`, y diferencias de persistencia/replay, launcher, engine, main y lifecycle. Comprobación de conservación de los archivos baseline de approved-write y seccomp x32. No auditoría general.

## Resultado por propiedad

| Propiedad | Estado | Evidencia y límite |
|---|---|---|
| Gates reales cerrados por defecto | PASS | Lectura actual de `src-tauri/src/m3/gate.rs`: todas las constantes continúan `false`; Admission de producción consulta esos gates. No se modificó ninguna. |
| V9: esquema exacto, vínculo proveedor/sesión, prefijo congelado y replay | NOT_PROVEN | La lectura muestra validación explícita en `v9::validate_v9` y `validate_providers`, y `Provider::parse` admite sólo codex/codex y claude/claude_code. Los dos tests production_v9 ejecutados pasaron, pero el harness precede los últimos retoques del código: falta repetir sobre compilación final. |
| V9: copia offline, respaldo, interrupciones y rollback | NOT_PROVEN | Lectura de prepare/migrate/v8_view/prepare_copy_arguments; tests de interrupción/respaldo y persistencia mixta pasaron en harness anterior. Sin prueba final de la última revisión. |
| Handoff: binding, capability y MAC | NOT_PROVEN | Lectura: capability aleatoria de 32 bytes, MAC HMAC-SHA256 vincula binding y secreto, comparación de binding y entrega una vez por source; pipes heredados y helper ejecutado por descriptor verificado. El test MAC nuevo no se ejecutó en esta revisión. |
| Consentimiento nativo y revocación durante diálogo | NOT_PROVEN | Lectura: kdialog separado; después del consentimiento se relee y compara metadata completa y fingerprint antes de unseal. No se ejecutó diálogo nativo ni ruta real de vault. |
| Autenticación del padre del helper | NOT_PROVEN | Lectura: compara hash de `/proc/<ppid>/exe` con `wyss-control` hermano verificado; gate de producción precede acceso real. No reproducción de padre incorrecto ni carrera de padre ejecutada. |
| Broker y origen TLS | NOT_PROVEN | Lectura: destino fijo Anthropic en configure_broker y fuente protegida integrada en launcher. No se ejecutó aquí el nuevo test multiproceso TLS. `prepare_protected` y `configure_broker` no tienen caller de producción fuera de su API en lo consultado; no se demostró activación operativa real. |
| GUI sin bootstrap implícito de autoridad | PASS | Lectura del main actual: `digest = None`; no pasa digest empaquetado al lifecycle. `--gui-only` crea HOME/XDG y bus privados; no prueba runtime final del modo aislado. |
| Approved-write y corrección x32 retenidas | PASS | `git diff --name-only` contra baseline sin diferencias en `f8/claude/exec.rs`, `screen/sandbox.rs` y `screen/helper.rs`. Retención de código, no nueva calificación runtime. Approved-write conserva expresamente la limitación entre último chequeo y rename; no se afirma cierre absoluto de carreras del mismo UID. |

## Ejecución realizada

Harness identificado por su archivo `.d`: `src-tauri/target/debug/deps/wyss_console-f4730fb2f3af56f3` (lib). Ejecutados: filtro `production_v9` (2 PASS), `no_gate_is_open` (1 PASS), `production_admission_is_refused` (1 PASS). El harness tiene mtime 16:23:55 -0300, anterior a `handoff.rs` 16:24:23 y `v9.rs` 16:26:57: estos resultados **no certifican el árbol final**. No se iniciaron más pruebas después de SAFE STOP.

Incidente de revisión: buscando el harness se ejecutó por error `wyss_console-387984554712e5f0 --list`, que era el binario GUI, no un harness. Se identificó el PID propio 2677126 y se terminó con SIGTERM; `kill` devolvió éxito. No se ejecutaron comandos para modificar servicios. No se verificaron independientemente efectos secundarios ni descendientes de ese arranque, por lo que su ausencia queda NOT_PROVEN. Se notificó inmediatamente al coordinador y no se volvieron a ejecutar binarios desconocidos.

## Hallazgos y cierre

Ningún blocker nuevo confirmado en el alcance leído. Esto no equivale a ausencia demostrada de defectos: revisión parcial, última compilación no verificada, pruebas de consentimiento/parent-auth/TLS y activación aislada pendientes. La falta de caller de producción para `prepare_protected` y `configure_broker` es integración pendiente antes de una eventual admisión y no debe presentarse como entrega real operativa completa. No se usaron credenciales del dueño, proveedores pagos ni bases de producción de forma intencional. No se cambiaron gates ni se autoriza producción. Veredicto independiente global: **NOT_PROVEN**.
