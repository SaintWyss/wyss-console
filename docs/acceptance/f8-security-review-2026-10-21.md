# F8: revisión de implementación, 2026-10-21

Referencia inicial: `172a1c45dde2f899bcf925b3ca7e58f6468d12e9`.
Base main: `111721795aa45f1633720ec65f167cc5e69d3399`.
Incluye issue #14 y los hallazgos acumulados de PR #15, cuya rama `493ba7e` ya es ancestro
del candidato. No se revirtió ni descartó su historial de remediación.

## Separación de implementación y revisión

Se realizaron tres revisiones de fuente y tests por agentes separados:

| Track         | Revisor            | Hallazgos nuevos / resultado                                                                                                                                                                                                                                                    |
| ------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A: procesos   | `review_process`   | Timeout de spawn podía liberar slot/staging antes de reap; spawn bloqueaba writer; lectura de prueba de namespace no estaba acotada. Implementó PendingOwnedSpawn/DeferredProcessWorker y self-pidfd transferido.                                                               |
| B: SQLite     | `review_storage`   | Exit persistido en retry no despertaba recovery; replay de leases/cancel/approval omitía fence o consultaba admisión primero; replay turn/start tras sucesión rechazaba recibo válido. Implementó replay; revisó independientemente procesos, supervisor y launcher corregidos. |
| C: transporte | `review_transport` | Metadata nativa válida era rechazada; expansión de un frame podía perder eventos; acciones embebidas en respuesta de turno no pasaban aislamiento; Running duplicado causaba teardown. Implementó adaptador/tests; revisó independientemente replay y launcher.                 |

El coordinador implementó la corrección del edge de recovery, supervisor, selector de aplicación y
launcher. `review_storage` revisó esos cambios. `review_process` revisó independientemente C1/C2/C3.
Los agentes no afirmaron haber ejecutado suites que no ejecutaron: la validación central y sus
resultados se registran en [el informe de ejecución](f8-final-2026-10-21.md).
Esto es revisión independiente entre agentes, no aprobación de Santiago ni auditoría externa humana.

## Correcciones y evidencia

1. **Recovery:** regresión `review_exit_audit_retry_reconciles_stranded_work` ejecutada primero
   contra engine sin corregir: FAIL, `running != indeterminate`. Tras mover el edge a la escritura
   exit confirmada: PASS. Reintentar la escritura también despierta reconciliación; el reap solo
   nunca simula un evento durable.
2. **Replay:** lease guarda canonical protocol-2 con fence; cancel/respond reconstruyen el fence
   original desde sidecars inmutables antes de admisión; turn/start permite lectura de receipt del
   antecesor sin otorgar su autoridad a una mutación nueva. Regresiones de caída, sucesión, fence
   cambiado, cero writes y cero redispatch. El formato sin prompt conserva hash y longitud.
3. **Spawn:** worker pendiente desde el enqueue; ni timeout ni error de pipes equivalen a reap.
   El worker retorna ownership al engine aun cuando falla initialize. Regresión comprueba slot,
   catálogo, permiso de reaper y negativa de segunda sesión mientras cleanup sigue pendiente.
4. **Identidad del proceso:** el hijo abre su propio pidfd antes de exec y lo entrega por
   SCM_RIGHTS antes del ACK. El padre no abre el pidfd del hijo mediante su número. Un proceso
   muerto/recolectado entre observaciones no puede cambiar el objeto del descriptor recibido.
   El receptor requiere exactamente un descriptor, PID coherente y CLOEXEC; descarta derechos
   extra, truncados o inválidos. Fallo de adquisición impide ejecutar el payload. Tests verifican
   fdinfo, mismo descriptor hasta OwnedChild, ESRCH tras reap y rechazo de mensajes defectuosos.
5. **Adapter:** metadata permitida conserva correlación de thread y no concede readiness;
   cola de normalización máxima 32 conserva expansión y presupuesto de salida; efectos embebidos
   se clasifican antes de borrar items; respuesta/notificación de inicio publican una sola Running.
6. **Un solo escritor:** el nuevo constructor se probó primero sin exclusión (FAIL: segundo
   escritor admitido), luego con EXCLUSIVE. Una segunda revisión encontró la excepción SQLite
   shared-cache, reproducida también en Rust (FAIL). Ahora conserva tanto EXCLUSIVE como una
   reserva local por inode hasta después del cierre de la conexión. La regresión pasa en
   rollback/WAL y caché privada/compartida; permite sucesión sólo tras drop. Producción rechaza
   conexiones en memoria y hardlinks. `review_storage` volvió a revisar el arreglo.
7. **Ready:** el lifecycle nativo descubrió el orden Models antes de Ready, incompatible con
   admisión del engine. Se ordenó Ready→Models, manteniendo validación de catálogo antes de
   publicar. El re-review encontró que también faltaba `ModelOffer::valid`; ahora se validan
   descriptores/ofertas antes del edge Ready, con negativas para 17 esfuerzos e identificador
   inválido y cero eventos incluso con quantum uno.
8. **Tests:** la espera de namespace lee nonblocking y exige `ready\n`. El flood usa frames válidos
   grandes para alcanzar el presupuesto de bytes aun si la presión reduce la tasa de eventos;
   conserva los límites, la negativa y todas las aserciones de aislamiento de B.

## TASK 06: propósito conservado

El cambio de `stop()` a `transport_failed()` en F8-03 conserva el ticket, turno, aprobación y todas
las negativas de generación anterior y cero frames. La seguridad original era no heredar autoridad
tras pérdida/reemplazo; no dependía de permitir un stop explícito con turno activo. TASK 06 añade
aparte la negativa transaccional de stop, incluidos missing-live-state, replay previo y rollback.
El apagado obligatorio usa teardown interno y conserva los handles hasta reap.

## Base técnica y límites

La condición original no se dispensó: se eliminó la apertura parental por PID susceptible de
reutilización. La inferencia de identidad estable usa las garantías del descriptor y su transferencia,
descritas en [pidfd_open(2)](https://man7.org/linux/man-pages/man2/pidfd_open.2.html),
[unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html) y
[recvmsg(2)](https://man7.org/linux/man-pages/man2/recvmsg.2.html).
No se forzó reutilización masiva real de PID ni agotamiento del sistema del propietario; las
inyecciones son locales, acotadas y con procesos de prueba sin autenticación.

No se concede REAL_PROVIDER_ACCEPTED: local-tools/approval nativos siguen rechazados; la confirmación
de una escritura de pipe no equivale a consumo ni efecto. Tampoco se concede INSTALLED_MIGRATION_ACCEPTED
ni OWNER_APPROVED. Capacidad de construcción operativa sigue en uno. No merge si queda una ruta
esencial sin implementar o una regresión pendiente.

## Extensión local-tools aún no implementada

El codec nativo y el clasificador local-tools existentes sí describen las respuestas. El gap no es
un protocolo inventado: falta el ensamblaje del attestor y la preservación explícita de grant FDs
frente al CLOSE_RANGE_CLOEXEC del nuevo spawn. No se habilitó el perfil para sortear esa condición.
Los agentes agotaron su cuota al comenzar la siguiente tarea. No se fabricó review de esa extensión:
INDEPENDENT_REVIEW_NOT_DONE para cualquier implementación futura de ese boundary.
