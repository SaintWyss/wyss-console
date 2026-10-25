# M1–M5 — informe de cierre seguro, 2026-10-24

**Dictamen: NO-GO. Trabajo detenido por pedido de Santiago.** La implementación quedó guardada; la calificación del último estado está incompleta. Este informe reemplaza las afirmaciones de estado del paquete anterior sólo para este worktree; no acredita un nuevo release.

## Estado Git y aislamiento

- Repositorio: SaintWyss/wyss-console; PR objetivo #27.
- Baseline verificado: `48396c41d0263687ea72b8392f4cbdbd46038d1a`.
- Worktree: `/home/saintwyss/Github_repos/wyss-console-blockers`.
- Rama local: `fix/m1-m5-production-blockers`.
- Commit de implementación: `8eed1be63885c193babb3dea82cc60e486c76858`.
- Último commit de código: `f042e0db96bde1b57d736bd324046aeb95df674b`.
- Los commits conservan la historia del baseline. No se hizo push de estos cambios ni se actualizó el HEAD remoto de #27 en esta ejecución. No hubo merge.

## Cambios implementados

### A. V9

`f8/v9.rs` ya es código de producción: esquema versionado, metadatos schema 9/vocabulario 3, migración offline sobre copia, límite histórico V8 con digest, replay y validación multiproveedor. Pares cerrados: `codex/codex` y `claude/claude_code`. Las conversaciones son únicas dentro de su proveedor; pueden compartir el mismo identificador entre proveedores sin mezclarse.

Se adaptaron writers, allocator, operaciones, handshake, contratos, estado de autoridad y recuperación. Los perfiles Claude se validan contra el proveedor de la sesión. El spawner productivo Codex sigue rechazando Claude: ningún cambio del esquema habilita inferencia real.

Existe el comando explícito `wyss-control --prepare-v9-copy SOURCE BACKUP_V8 CANDIDATE_V9`, con tres rutas absolutas, fuente sólo lectura, outputs nuevos privados y rechazo de sobrescrituras. No descubre ni sustituye una base instalada. La preparación usa una instantánea SQLite consistente y devuelve backup V8 y candidato V9. El uso operativo exige una copia offline aprobada; no se ejecutó contra datos productivos.

La prueba nueva usa writers y autoridad reales, ClaudeWorker y bridge/ejecutor sintéticos; conserva historial Codex, crea Claude con identidad real del contrato, completa un turno/aprobación, persiste, reabre y recupera. No es una prueba con el proveedor real. Se añadieron pruebas de alteración de atribución/UUID y crash real de reconstrucción; esas últimas incorporaciones no terminaron de ejecutarse antes del stop.

Rollback: restaurar el backup V8 completo con su binario compatible. El downgrade de una base que contiene historia Claude se rechaza; reemplazar sólo el binario no es rollback de V9.

### B. Credenciales

Se retiró el delivery anterior que podía desellar dentro del proceso GUI. El núcleo del almacén cifrado se separó de sus comandos de ventana. Se añadió `wyss-credential-agent`: proceso separado con diálogo nativo de metadatos, verificación del ejecutable padre, core dumps deshabilitados y acceso al vault limitado al registro/fingerprint elegidos.

`ProcessCredentialSource` liga la entrega a autoridad, sesión, runtime, generación, perfil y registro; usa capability aleatoria, HMAC-SHA256 y pipes heredados de un solo uso, mensajes acotados y timeout. No transporta la clave en argv, variables de entorno ni archivos plaintext. El launcher dispone de `prepare_protected`; la fuente productiva por defecto sigue siendo `NoCredential`. La activación real del servidor para Claude no está conectada ni autorizada.

El E2E sintético pasó desde un almacén cifrado descartable, a un agente separado, a la fuente de autoridad y al broker Python real con TLS local verificado. Rechazó certificado no confiable y token incorrecto; comprobó consumo único y ausencia de plaintext en archivos del almacén. Esa corrida no abrió el diálogo nativo: se añadió después un fixture opt-in para probarlo, aún sin ejecutar. Los últimos cambios de borrado de memoria, negativos de MAC y consentimiento sintético requieren recompilación y repetición.

No se afirma resistencia frente a un atacante con control de la misma cuenta ni revocación retroactiva automática: eliminar el registro impide futuros inicios; detener el runtime/broker revoca el acceso actual.

### C. Activación

La GUI pasa `digest=None` al lifecycle: abrir la ventana ya no concede instalación/bootstrap/reemplazo de autoridad, incluso con digest embebido de procedencia. El modo explícito `--gui-only` crea HOME/XDG y bus privados, valida su dominio y cierra sólo su host/bus al salir. Se bloquea la operación de lingering en ese dominio.

Se añadió limpieza RAII y timeout de parada. Esto está implementado en fuente, pero el último ejecutable y su coexistencia desktop con la instalación anterior todavía no fueron calificados. No debe presentarse como aceptación E2E terminada.

### D. Paquete y revisión

El empaquetador ahora incluye un cuarto ejecutable, `wyss-credential-agent`, con escaneo de marcadores fixture. Se actualizaron los tests del inventario. **No se produjo un nuevo build de producción, archivo ni checksums para estos commits.** Los artefactos del baseline 48396c4 no corresponden al código nuevo.

Se solicitó una única revisión independiente a otro agente. Su informe parcial se conserva junto a éste. No hay aprobación independiente final del HEAD actual.

## Evidencia ejecutada y límites

Los logs están en `.artifacts/blocker-evidence/`. Los resultados pertenecen a las revisiones intermedias efectivamente compiladas; no se extrapolan al último HEAD.

| Verificación                                                         | Resultado observado                         | Evidencia / limitación                                                                                                                                                                                                                                        |
| -------------------------------------------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust lib/bins + authority_boundary + f8_contracts + f8_serve_process | PASS: 1.990 pruebas, 0 fallos, 46 ignoradas | `rust-full.log`; snapshot intermedio anterior a los últimos retoques. Ignoradas no son PASS.                                                                                                                                                                  |
| V9 genuino Claude + historial Codex + disco + recuperación           | PASS                                        | `v9-live6.log`, `v9-production2.log`, `rust-full.log`.                                                                                                                                                                                                        |
| Interrupciones por etapa + backup/copia/restauración V8              | PASS                                        | `v9-production2.log`, fuente fila por fila sin cambios.                                                                                                                                                                                                       |
| Nuevos negativos de atribución y crash productivo                    | NOT_PROVEN en HEAD final                    | Incorporados después de la corrida completa.                                                                                                                                                                                                                  |
| Credential handoff proceso → broker → TLS sintético                  | PASS                                        | `credential-process2.log`; anterior al último hardening/fixture de diálogo.                                                                                                                                                                                   |
| Diálogo nativo completo + credenciales reales                        | NOT_PROVEN                                  | Fixture preparado, no ejecutado; claves reales nunca utilizadas.                                                                                                                                                                                              |
| Python bridge/broker                                                 | PASS: 77 pruebas                            | `python.log`; ResourceWarnings de archivos no cerrados en tests, sin fallo de suite.                                                                                                                                                                          |
| Typecheck Svelte/TypeScript                                          | PASS, 0 errores/avisos Svelte               | `typecheck.log`.                                                                                                                                                                                                                                              |
| Frontend                                                             | Incompleto                                  | Primera corrida: 1.055 PASS/1 FAIL por archivo eliminado todavía rastreado antes del commit; escáner pasó tras commit. Otra corrida: 1.054 PASS/2 timeouts de 5 s en tests Git bajo compilación. No se atribuye definitivamente a carga sin repetición final. |
| Escáner de credenciales/repo                                         | PASS: 5/5                                   | `repo-scan.log`, posterior al primer commit.                                                                                                                                                                                                                  |
| Clippy -D warnings                                                   | NO final PASS                               | Detectó needless_borrow/collapsible_if corregidos; siguiente corrida cruzó una edición concurrente del fixture y falló por método todavía no visible. Repetición final interrumpida antes de completarse.                                                     |
| Última repetición de frontend, Clippy y credential E2E               | INTERRUPTED por safe stop                   | `frontend-qualified.log`, `clippy-final.log`, `credential-final.log`.                                                                                                                                                                                         |
| Desktop final, coexistencia, Native Claude/Codex, M4 final           | NOT_PROVEN para este HEAD                   | Hay regresión Rust intermedia; falta E2E del binario final.                                                                                                                                                                                                   |
| Nuevo paquete, reproducibilidad, checksums y rollback de instalación | NOT_PROVEN                                  | No construidos/probados en este estado.                                                                                                                                                                                                                       |
| Rendimiento del nuevo candidato                                      | NOT_PROVEN                                  | No se midió; no se reutilizan números del baseline como evidencia nueva.                                                                                                                                                                                      |

## Parada segura

Se enviaron señales sólo a 13 procesos pertenecientes a las tres ejecuciones propias aún activas (credential fixture build, Clippy, frontend y sus descendientes). El registro `safe-stop.json` terminó con `remaining_non_zombie: []`. No se hicieron cambios productivos para detenerlos.

El revisor informó un arranque accidental del binario GUI al intentar listar tests; lo terminó. El alcance y límites de esa observación constan en su informe; no se afirma ausencia de efectos sin evidencia suficiente. Esto debe considerarse al revisar la disciplina de aislamiento de la sesión.

## Bloqueadores mínimos al retomar

1. Compilar y ejecutar sobre un único HEAD congelado los nuevos tests V9 (negativos/crash), handoff MAC/limpieza, fixture de consentimiento nativo y Clippy estricto; cerrar la repetición frontend sin fallos.
2. Completar la revisión independiente del código final y corregir cualquier hallazgo. Approved-write mantiene un intervalo check/rename frente a escrituras concurrentes de la misma cuenta: no se califica como CAS libre de TOCTOU; Local Tools Claude sigue bloqueado. Revalidar x32 con el helper empaquetado.
3. Probar GUI-only y coexistencia en desktop descartable, recuperación M4, Native y Codex Managed con el binario final. La ruta real de activación Claude queda pendiente y deshabilitada, no se debe afirmar integración productiva completa.
4. Construir el paquete final con cuatro binarios, revisar permisos/fixtures/ACL/versiones, checksums y reproducibilidad; ejecutar instalación/rollback descartables, actualizar runbook y release packet, y publicar los commits en #27.

## Autorizaciones del owner todavía pendientes

ADR0053, ADR0054, ADR0055, ADR0056, ADR0057 y ADR0058; política de historial de métricas; OD-M2-01 y OD02; concurrencia Managed por encima de uno; provisión de claves y pruebas con proveedores reales/pagadas; lingering; migración live; instalación productiva y reinicio de servicios. Ninguna se aceptó tácitamente. B1, afirmaciones fuertes de aprobación humana y Claude Managed real siguen sin habilitarse.

No se solicita autorización de producción: faltan verificaciones de ingeniería y seguridad. **NO-GO** hasta completar los bloqueadores anteriores. Para retomar, continuar desde esta rama y este HEAD; no reiniciar M1–M5 ni comenzar M6.

## Archivo de evidencia de cierre

Logs e informes preservados en `.artifacts/blocker-evidence-safe-stop.tar.gz`; SHA-256 `aff7566f69f7792a8da0a0719ab7c0856de5c9595f8821657843cac25664df52`. Índice por archivo dentro de `blocker-evidence/evidence-index.json`. El archivo captura los informes antes de añadir este párrafo de referencia. No contiene un build de producción.
