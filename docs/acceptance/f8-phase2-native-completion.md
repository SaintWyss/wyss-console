# F8 Phase 2 — native Codex local-tools and approvals

## Estado y alcance

**COMPLETE para integración aislada sin autenticación.** No constituye aceptación
con proveedor real, readiness de producción ni aceptación de ADR 0051.

- SHA inicial: `ef5fbbf362b7cb60e32ca57a97cfa28787444962`.
- SHA final de implementación: `3f1b78e6196f0e988e2a4abd3a21f9ab2f08f0c2`.
- La entrega documental posterior no cambia el código validado. El SHA final
  publicado se identifica en el informe de entrega y con
  `git log -1 --format=%H -- docs/acceptance/f8-phase2-native-completion.md`.
- Rama: `integrate/f8-final`; worktree `wyss-console-rama-4`.
- Capacidad activa: **1**, sin cambios en contracts/resources.json.
- La aplicación instalada conserva protocolo 1/V7. No se ejecutaron servicios,
  sesiones autenticadas, migraciones instaladas, herramientas del usuario ni GUI.

## Archivos cambiados respecto del SHA inicial

1. `src-tauri/src/adapters/codex/probe/isolation.rs`
2. `src-tauri/src/adapters/codex/probe/isolation_tests.rs`
3. `src-tauri/src/f8/authority.rs`
4. `src-tauri/src/f8/engine.rs`
5. `src-tauri/src/f8/launcher.rs`
6. `src-tauri/src/f8/native_approvals.rs`
7. `src-tauri/src/f8/native_process_tests.rs`
8. `src-tauri/src/f8/operations.rs`
9. `src-tauri/src/f8/process_integration_tests.rs`
10. `src-tauri/src/f8/provider_adapter.rs`
11. `src-tauri/tests/fixtures/codex_runtime.py`
12. `docs/acceptance/f8-phase2-native-completion.md`

## Lanzamiento, aislamiento y attestation

`NativeLocalToolsPlan` sólo puede construirse mediante preparación host interna.
No es deserializable; no acepta argv, FDs o environment desde protocolo 2.
El factory consume un plan ya preparado: no tiene callback capaz de copiar o
probar el ejecutable durante el poll del único writer. Cada plan se consume una
vez; otra generación requiere preparación host nueva. La ausencia de plan falla
cerrada, sin restart implícito ni reconstrucción desde SQLite.

Se reutilizan `IsolationLaunch`, los grants OwnedFd y la herencia de TASK 07A.
El perfil F8 agrega `--unshare-pid` al launcher validado con `--die-with-parent`;
el worker exige DescendantsContained y la readiness requiere observación real
de otro PID namespace. El factory nativo rechaza el perfil equivocado antes de
reservar una generación. La ruta inference existente conserva sus restricciones.

El ejecutable, el catálogo y las dos políticas se capturan en imágenes memfd
selladas contra escritura, crecimiento y truncamiento. El catálogo se valida
sobre los bytes sellados; las políticas se comparan con sus constantes. El
launcher crea mounts privados readonly con `--ro-bind-data`, usando únicamente
FDs propios. Una prueba real conserva los bytes originales después de sustituir
el pathname y después de sobrescribir el inode fuente mientras el spawn está
encolado. Los intentos de escribir en los memfds sellados fallan en el kernel.

El probe host acotado ejecuta `--version` sobre **la misma imagen sellada**, con
FDs reabiertos para offsets independientes, pidfd, gate y PID namespace. Exige
exit exitoso, reap probado y stdout exacto `codex-cli 0.159.0\n`; versión distinta,
salida excesiva, fallo o timeout rechazan el plan. La preparación ocurre antes
de entregar el plan al supervisor; no se añade trabajo post-fork salvo los
syscalls existentes de ownership y preservación de grants.

La credencial no se copia ni se lee para crear imágenes. Conserva su capability
O_PATH al inode validado y el `--bind-fd` existente. Las pruebas sólo usan
`FIXTURE-CREDENTIAL` dentro de directorios temporales privados. Se revalidan
workspace, máscaras de política, identidad de credencial, catálogo, ejecutable
y políticas; el environment F8 queda fijo, con CODEX_HOME privado. Se ocultan el
home Codex suministrado, el home Codex predeterminado y el control plane indicado
por el host. La ruta de construcción de protocolo 1 conserva su comportamiento.

Initialize debe declarar el home correcto. Después se verifican config, managed
requirements, ausencia de hooks externos y modelos antes de publicar ready.
Thread start/resume usa la postura local-tools existente: untrusted,
workspace-write, sin red ni raíces adicionales, proyectos untrusted y clamps
fijos. La respuesta verifica también cwd, thread identity y cliVersion fijada.
Config/requirements/hooks y hechos host se vuelven a comprobar antes de admitir
turnos y decisiones; iniciar un proceso nunca basta para atestarlo ready.

La investigación de memfd reprodujo que bind-fd no resuelve una imagen anónima.
La prueba directa con bwrap pasó usando ro-bind-data. Sus operaciones están
especificadas en el [código primario de bubblewrap](https://github.com/containers/bubblewrap/blob/main/bubblewrap.c).

## Solicitudes y respuestas nativas

Se reutilizan WireMessage, el clasificador local-tools, las estructuras bounded
y `encode_approval_response` de Codex 0.159.0. No hay JSON de respuesta alternativo.

La correlación distingue el item ID normalizado del JSON-RPC request ID tipado
(string o número). Los IDs vistos permanecen acotados por conexión y no se
reutilizan después de una resolución ni de otro turno. Thread y turn deben
coincidir con el binding y turno de esa instancia. La generación y sesión son
las del worker que el engine registró, nunca una declaración del proveedor.

Una propuesta completa precede la solicitud. Comando, cwd y commandActions se
contrastan; archivos requieren el change set completo. Las acciones pasan por
ProviderAction, canonical fingerprint y confinamiento existentes. Authority y
Writer verifican lease/policy y abren el registro durable; Engine conserva el
ticket y presentación efímeros. El adaptador no posee autoridad durable propia.

La respuesta exige una decisión explícita, ofrecida por la solicitud original.
Authority verifica fence, revisión, intent, deadline, lease y acción, además de
preflight; Writer registra antes del efecto; Engine revalida antes de escribir.
El adaptador verifica de nuevo su acción retenida y construye la respuesta desde
el request original. Consume su ticket antes de intentar el write y nunca lo
reconstruye para reintentar una aceptación potencialmente entregada.

Salida o completado de una herramienta sin aceptación causan fencing. Antes de
confirmar un resultado se reconstruye su fingerprint desde la estructura nativa
completada. Los items agregados en un terminal también deben corresponder a
items ya mediados; un resultado distinto o una acción escondida se rechazan.

`serverRequest/resolved` puede ser una retirada: antes de decidir invalida la
conexión mediante shutdown fail-closed; después de decidir sólo retira el request.
**No se convierte en ProviderConsumed ni en confirmación de ejecución.**

## Semántica real de send_critical

- Se llama el transporte crítico existente, inmediatamente y sin cola propia.
- Rechaza writes normales previos aún buffered y frames fuera del límite atómico.
- Un write completo al pipe sólo permite registrar **KernelAccepted**.
- El resultado de una acción sólo se confirma desde evidencia nativa correlacionada
  del item completado y su fingerprint; no desde el éxito del write.
- Backpressure o error después de decidir causan fencing. Una aceptación incierta
  permanece indeterminate, sin retransmisión automática.
- El replay exacto devuelve el recibo durable; un intent con otra decisión entra en
  conflicto. Ninguno vuelve a escribir la respuesta nativa.
- Un fallo de auditoría posterior al write también detiene el runtime y permite
  recovery posterior sin redispatch.

## Evidencia de aplicación y fallos

La fixture recorre PreparedApplication → F8Transport → Authority → Engine →
NativeCodexWorker → DeferredProcessWorker, con AF_UNIX real y almacenamiento SQLite
V8 temporal. El cliente hace hello, prepare, runtime/start local-tools,
initialization/attestation, lease, binding/materialización, turn/start, lecturas de
aprobación, decisión explícita, terminal, stop/reap, close y audit/validación V8.
Los ciclos cubren comando y archivos con accept, decline y cancel; las acciones
son sintéticas: el comando nunca se ejecuta y el archivo modificado es del fixture.

Regresiones adicionales comprueban estado durable y tickets/procesos:

| Frontera                                                      | Evidencia                                                                        |
| ------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Sesión, generación o instancia incorrecta                     | Rechazo por socket, aprobación original sigue pending, cero respuesta nativa     |
| Thread/turn divergente, comando/parse divergente              | Fencing, reap, ausencia de pending ejecutable y V8 válido                        |
| ID duplicado o reutilizado tras resolución/otro turno         | No aparece una segunda aprobación durable ni segunda respuesta                   |
| Solicitud retirada, salida temprana o acción no solicitada    | Shutdown, invalidación durable y cero aceptación                                 |
| Permisos/red/entorno remoto/grantRoot                         | Rechazo del codec mediado y cero efecto                                          |
| Approval expiry y lease expiry/revoke                         | Sin aceptación; status no ejecutable, reap y ledger validado                     |
| Cancelación de turno y cambio de política                     | Capacidad efímera invalidada; aceptación posterior rechazada                     |
| Fingerprint transient incorrecto                              | Ledger no autoriza el claim divergente                                           |
| Backpressure y entrega incierta                               | Decisión durable indeterminate, runtime cercado, replay sin write                |
| Crash antes o después de decidir                              | Sin autoridad heredada; efecto no confirmado queda desconocido                   |
| Auditoría fallida después del write                           | Recovery/indeterminate, cero reenvío                                             |
| Versión, home, requirements, hooks, MCP o sandbox incorrectos | Ningún evento instance_ready; cleanup sin capacidad perdida                      |
| Grant obsoleto, exec fallido, launcher sin namespace          | Falla cerrada y sin ready                                                        |
| Reap sin prueba                                               | Runtime sigue ocupando capacidad; otro start es capacity_exceeded                |
| Restart                                                       | Historia y replay legibles; sin atestación/ticket ejecutable ni relanzamiento    |
| Confinamiento y ownership                                     | Suites existentes de pidfd, SCM_RIGHTS, descendants y bounded reaper conservadas |

La inyección PrefixThenFail entrega un prefijo malformado por un pipe real y
simula la pérdida de resultado en el límite del adapter. **No se presenta como
una escritura parcial real de Linux:** el transporte normal usa PIPE_BUF y sus
pruebas de pipe lleno verifican la operación atómica. AfterWriteFail simula pérdida
del resultado después de escribir el frame completo. ReapStuck retiene la prueba
de reap; no pretende reproducir un proceso kernel en estado D.

## Validación

Todos los comandos terminaron con exit 0 sobre el código final:

| Comando                                                                                                                                     | Resultado                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `node scripts/tool.mjs cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`                                                           | PASS                                                                                                                                  |
| `node scripts/tool.mjs cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`                             | PASS                                                                                                                                  |
| `node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib f8::`                                                 | 269 PASS; 0 failed; 0 ignored                                                                                                         |
| `node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --bins --test authority_boundary --test f8_contracts` | Library 910 PASS / 7 ignored; desktop binary 361 PASS / 12 ignored; control binary 0; authority boundary 1 PASS; F8 contracts 10 PASS |
| `npm run scan:repo`                                                                                                                         | 5 PASS / 0 failed; 928 tracked files incluidos los nuevos                                                                             |

La suite completa incluye 27 ownership/pidfd/containment, 11 process worker,
1 process integration, 4 application, 15 native process/application, 31 codecs
Codex y 16 isolation tests: todos PASS. SQLite V8 se valida en las fixtures de
éxito, fallos, recovery y restart. Respecto de TASK 07A son 16 tests nuevos
(13 dentro de F8 y 3 de aislamiento), además de las matrices dentro de cada test.
Los casos críticos nuevos se ejecutaron primero de manera focalizada; después
se corrieron las suites completas. Los fallos encontrados en memfd/bwrap,
shape de la fixture de archivos y lifetime del handle de la fixture fueron
reproducidos y corregidos antes de esta validación final.

Los logs locales están en `.cache/f8-phase2-*.log` (no versionados). Los tests
ignorados existentes que requieren configuración real, proveedores instalados,
credenciales o servicios privados no se ejecutaron. No se debilitó ni eliminó
ninguna aserción previa. No hubo cambios en tipos frontend públicos ni GUI.

## Revisión de seguridad

Revisión independiente read-only: **PASS por inspección**, agente
`native_boundary_review`. No ejecutó independientemente las suites finales.
Todos sus hallazgos quedaron corregidos:

1. commandActions divergente: comparación con propuesta y regresión por socket.
2. Sustitución del ejecutable: imágenes selladas y exec real bajo sustitución.
3. Versión desvinculada: probe de la misma imagen sellada antes del supervisor.
4. Preparación dentro del spawn: factory consume sólo planes preexistentes.
5. Catálogo validado antes de snapshot: validación del inode sellado y regresión
   restaurando un pathname válido sobre un snapshot inválido.

La revisión propia también agregó verificación de fingerprints al completado y
al terminal agregado, invalidación al iniciar stop y tests de escritura/auditoría
incierta y de capacidad retenida. No queda un defecto bloqueante confirmado en
el alcance de esta integración aislada.

## Operaciones rechazadas y condiciones NOT_PROVEN

Continúan rechazadas permissions grants, user-input callbacks, MCP y herramientas
no mediadas, writeStdin, entornos remotos, network approvals, grantRoot,
acceptForSession y amendments persistentes. Un file request sin change set
completo previo y acciones agregadas sin la secuencia individual verificada
fallan cerrados; no se inventa una aprobación ni una respuesta fallback.

Una nueva generación necesita plan host nuevo; no existe restart automático ni
recuperación de grants/attestations desde historia durable. La entrega no conecta
estos tipos con selección instalada o GUI.

**NOT_PROVEN:** proveedor autenticado, efectos de herramientas reales, seguridad
Codex multiproceso de refresh de credenciales, cuotas, mediciones de recursos de
producción y todas las combinaciones de agotamiento/kernel schedules. La
contención de procesos ininterrumpibles conserva ownership/capacidad en vez de
atribuirles reap. Los casos inyectados están identificados arriba.

ADR 0051 permanece Proposed. No se modificaron main, PR #15, la instalación, el
protocolo 1 desplegado, V7 instalado ni max_active_runtimes. No se inició Phase 3
ni Phase 4. La condición de aceptación aquí es exclusivamente la implementación
nativa completa con fixtures aislados y sin autenticación.
