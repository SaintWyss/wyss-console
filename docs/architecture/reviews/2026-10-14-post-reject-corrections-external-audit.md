# Auditoría externa de las correcciones IR-01 a IR-15

Fecha: 2026-10-14. Autor de esta pasada: Codex. Alcance: arquitectura y seguridad **en papel**.

Repositorio privado `SaintWyss/wyss-console`, PR [#2](https://github.com/SaintWyss/wyss-console/pull/2),
rama `docs/console-planning-review-2026-10-13`.
Revisión fijada: **`906822fd7b87b71728198a11672831ff41f7713e`**, antes de agregar este informe.

## 1. Veredicto y declaración de autoría

**REJECT de la preparación del plan para su aceptación.** Las correcciones eliminan varias
afirmaciones falsas y establecen prohibiciones útiles, pero todavía no proporcionan contratos
compatibles y verificables para el producto completo. El rechazo no se fundamenta simplemente
en que no se ejecutaron pruebas: quedan decisiones de diseño ausentes, requisitos operativos
contradictorios y criterios de aceptación que todavía prometen propiedades que la arquitectura
admitida no puede ofrecer.

En particular:

- Un agente nativo irrestricto o una shell LP4 del usuario conservan la posibilidad de actuar
  como ese usuario ante la autoridad. Bloquear su despacho automático desde OL3 no elimina un
  agente iniciado manualmente ni una shell remota ya autorizada. La separación entre aprobaciones
  no está garantizada en esa coexistencia.
- Ejecutar el mismo agente bajo un principal restringido cambia sus capacidades. No hay un
  contrato de compatibilidad que demuestre qué parte de la experiencia nativa se conserva.
  Tampoco se ha diseñado y validado una alternativa que proteja la autoridad manteniendo los
  agentes nativos irrestrictos. No puede aprobarse esa combinación por llamarla `BLOCKED`.
- No están resueltos el protocolo de entrada remota frente a reinicios, el dominio de fallo
  PTY/parser ni el vocabulario efectivo del controlador de políticas de Wyss Agent.
- La serialización gestionada es coherente con los ADR vigentes; sigue siendo una restricción
  provisional que no satisface DN-08=A. LP4 solo con presencia local tampoco satisface por sí
  solo el caso de control desde fuera de casa después de logout.

**Autoría e independencia:** no participé en la autoría de la base ni de las correcciones
durante esta conversación. No me atribuyo el informe previo ni su identidad de revisor.
El historial atribuye `00b18fac46f17a36d53ed5f30cc675b337dd30a1` a Claude, y ese propio informe
declara solapamiento con la autoría anterior. Los commits de corrección figuran bajo la cuenta
del propietario; esa atribución Git no identifica de manera suficiente a cada asistente que
pudo producirlos. El contexto y la memoria disponibles no registran autoría previa mía de estos
documentos, pero no permiten certificar todas las sesiones históricas. Por ello, la independencia
histórica absoluta es **NOT_PROVEN**; esta es una nueva revisión adversarial, sin coordinación
con autores anteriores, y no una certificación incondicional de cierre de B-P0-06.

El informe no acepta ADR, riesgos ni reducciones de alcance en nombre del propietario.

## 2. Método y categorías de evidencia

Se leyó primero el informe post-owner preservado, después 42 y después 38, en el orden solicitado.
Luego se contrastaron las correcciones con los contratos originales, el registro 32, la jerarquía
de 00 y los pasajes pertinentes de los ADR. Se revisaron tanto las cláusulas iniciales como las
adendas: una prohibición en 42 no convierte automáticamente en correcto un método de 16 o un
criterio de 20 que todavía exige otro comportamiento.

Categorías utilizadas:

| Categoría | Significado en este informe |
| --- | --- |
| Base aceptada | Texto de ADR vigente o código leído en revisión fijada; no implica nueva validación de ejecución |
| Requisito del propietario | Capacidad futura seleccionada en 32; no equivale a ADR aceptado |
| Diseño candidato | Contrato propuesto, todavía sujeto a revisión y aceptación |
| Observación estática | Resultado reproducible de leer/comparar texto o fuente en un commit concreto |
| Bloqueado / NOT_PROVEN | Falta diseño, decisión o evidencia indispensable; no cuenta como capacidad entregada |

En la matriz IR, **PASS_SPEC** significa únicamente que la corrección documental del defecto
concreto es verificable por lectura. **PARTIAL_SPEC** y **FAIL_SPEC** indican carencias o
contradicciones que siguen presentes. Las pruebas de ejecución mencionadas permanecen
**NOT RUN**; una corrección documental no es un PASS de seguridad del sistema.

### 2.1 Base contractual que sigue vigente

| Base | Pasaje contrastado | Consecuencia |
| --- | --- | --- |
| ADR 0002 | Decisión y líneas 53–72 | La UI no es autoridad; otro proceso por sí solo no establece aislamiento OS |
| ADR 0003 | Líneas 69–72 | Persistencia de secretos prohibida salvo futura excepción formal; un rótulo no autoriza guardar contenido |
| ADR 0005/0007 | Decisiones de PTY y sesiones locales | Persistencia fuera de la ventana exige nueva arquitectura; guardar un layout no conserva un proceso |
| ADR 0008 | Decisión/capacidades | Capacidad observada no es autoridad; desconocido no equivale a soportado |
| ADR 0011 | Líneas 8–45 | Un escritor SQLite, recibos durables y validación de UID; no protege contra un proceso comprometido del mismo usuario |
| ADR 0012 | Líneas 8–25 | Un App Server compartido, no un runtime por sesión |
| ADR 0013/0014 | Gates, revocación y binding | Leases y attachments actuales son necesarios; un binding o reconexión no concede ejecución |
| ADR 0015 | Líneas 8–29 y prohibición de overrides | Un único turno activo global; selección gestionada de modelo requiere extensión propia |
| ADR 0016/0018 | Mediación; 0018 líneas 110–127 | Accept y revalidación protegen el flujo gestionado dentro de su modelo; el mismo usuario puede conectar, conceder leases y aceptar aprobaciones |
| ADR 0020/0023 | 0020 líneas 289–297; 0023 aprobación/seguridad | La UI envía intents; su existencia no autentica una intención humana frente al mismo UID |
| ADR 0025/0026 | Bootstrap y preferencias | Instalación y preferencias no son nuevos poderes ni una nueva frontera |
| ADR 0027 | Decisión | Producto personal local; no obligación de distribución pública |
| ADR 0028 | §5, §8 y compatibilidad con 0012/0015/0018 | Conserva un runtime/un turno; contiene afirmaciones sobre página comprometida y concurrencia pendientes de corregir formalmente |
| ADR 0029/0030/0031 | Estado y propuestas | Los tres siguen **Proposed**. 0031 observa modelos; no autoriza concurrencia ni selección por turno |

La excepción de seguridad de 00, líneas 94–95, resuelve la aparente prioridad del producto:
un objetivo funcional no deroga un control aceptado. 33 §4 aplica correctamente esa regla.
No se exige modificar los ADR históricos para ocultar sus errores: corresponde una decisión
posterior que identifique exactamente las cláusulas sustituidas.

## 3. Resultado individual de IR-01 a IR-15

Las referencias numéricas de documento corresponden a archivos de `docs/architecture/`.
Los números de línea son los del commit auditado, no los de versiones futuras.
Contratos centrales: [16](../16-MULTI-DEVICE-AND-REMOTE.md),
[34](../34-OL3-ORCHESTRATION-CONTRACT.md), [35](../35-HOST-REMOTE-AND-MOBILE-DELTA.md),
[36](../36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md),
[37](../37-HANDOFF-ADAPTERS-UX-AND-DATA-CONTRACTS.md),
[40](../40-HERMES-BOUNDARY-STATIC-EVIDENCE.md) y [42](../42-POST-REJECT-CORRECTIVE-PLAN.md).

| IR / severidad original | Resultado documental | Evidencia y efecto de la corrección | Qué falta para cerrar realmente |
| --- | --- | --- | --- |
| IR-01 CRITICAL | **FAIL_SPEC** para la combinación final; reconocimiento del límite corregido | 33 R2/R4; 34 §1 y §3.3, líneas 167–177; 42 C-A01 reconocen suplantación y bloquean despacho. Pero 34 TS-ORC-11, líneas 293–294, todavía exige que un runtime nativo no pueda falsificar aprobaciones. ADR 0018 dice expresamente lo contrario dentro del mismo UID | ER-01/02. Frontera de confianza para todos los orígenes nativos y LP4, no solo workers lanzados por OL3; contrato funcional y ADR posterior. TS-SEC-OL3-01 **NOT RUN** |
| IR-02 CRITICAL | **PARTIAL_SPEC** | 35 §6/7 y 42 C-A02 corrigen revocación e integridad; LP4 fuera de sesión se deniega como propuesta provisional. Persisten GR2 de 16 §5.2.4 y ambigüedad entre grant largo y claim corto. No se define cancelación atómica de escrituras admitidas al lock/logout | ER-02/03. Política diferenciada de grant/claim, revocación, presencia y LP4-away; aprobación informada del cambio funcional. TS-SEC-REM-01/02 **NOT RUN** |
| IR-03 CRITICAL | **PASS_SPEC**, dependiente de IR-01 | 34 §1, §3.3, `submit`, `grant_step` y `submit_handoff`; 37 §8; 42 C-A03 excluyen que el DAG o un handoff automático autoricen un lanzamiento nativo. Consentimiento por intento, bytes y destino revisados, sin desbloquear una frontera ausente | Es un stop verificable en papel, no una implementación ni el cierre de OL3 nativo. TS-SEC-OL3-02 **NOT RUN**; resolver ER-01/02 antes de habilitarlo |
| IR-04 HIGH | **PASS_SPEC** de la precedencia actual; alcance final bloqueado | 34 §4.1, líneas 225–231, conserva un App Server y turnos seriales; TS-ORC-04 tiene gate explícito. DN-08/DN-42 siguen requisitos futuros | ER-05: 20 AC-FINAL-06 y 34 TS-ORC-14 necesitan la misma condición. Nueva arquitectura de concurrencia y selección, sin usar 0031 como autorización. TS-SEC-MAN-01 y pruebas concurrentes **NOT RUN** |
| IR-05 HIGH | **PARTIAL_SPEC** | 17 §14, 40 §5 y 42 IR-05 niegan shell opaca desde Hermes y reconocen límites del matcher. Fuente Wyss fijada confirma el matcher, pero también defensas adicionales; véase §5 | ER-04. Enumerar operaciones, parámetros, efectos transitivos y autorización de ejecución; cerrar todo despacho desconocido. E-HRM-01..09 y TS-SEC-AGT-01 **NOT RUN** |
| IR-06 HIGH | **FAIL_SPEC** por contrato aún divergente | 35 §6 añade identidad de arranque, invalida claims y exige OUTCOME_UNKNOWN. 16 §5.2.5 y RT-05 todavía describen respuesta original/ausencia de segundo efecto basada en RAM 24h; IF-HOST.write no define secuencia/recibo de entrada | ER-03. Protocolo único hasta el escritor PTY, incluido efecto parcial, pérdida de ACK, saturación y reinicios independientes. TS-SEC-REM-03 **NOT RUN** |
| IR-07 HIGH | **PASS_SPEC** del límite y prohibición de retry; recuperación no probada | 16 §4.11, 34 §5 y 35 §4 distinguen PTY perdido de descendientes; ORPHANS_POSSIBLE impide relanzamiento automático. Mejora correcta respecto de inferir muerte de todo el árbol | ER-06/07. Contabilidad y propiedad del scope, tratamiento de efectos fuera de él y liberación de locks. Un cgroup vacío no prueba ausencia de servicios/persistencia previamente creados. TS-SEC-HOST-01 **NOT RUN** |
| IR-08 HIGH | **PASS_SPEC** de la regla de inactividad | 16 §4.1/SM-HOST y 35 §4 ya exigen ausencia de PTY, pipelines activos/pausados/en espera/recuperación y cliente requerido | ER-07. Falta protocolo de keepalive, expiración y reconciliación tras caída del coordinador; la regla y el test no definen esa implementación. TS-SEC-HOST-03 **NOT RUN** |
| IR-09 HIGH | **PARTIAL_SPEC** | 35 §2 exige supervisor PTY separado del parser, helper por terminal y límites. 42 y TS-SEC-HOST-02 exigen supervivencia de PTYs; ya no se deja la separación como opción | ER-06. Principal, descriptores, IPC, cgroups y semántica OOM aún sin concretar. Procesos separados no demuestran aislamiento de fallos. Fuzz/OOM y TS-SEC-HOST-02 **NOT RUN** |
| IR-10 MEDIUM | **PARTIAL_SPEC** | 35 §7, líneas 210 y 220–224, deniega por defecto LP4 sin atestación y autenticación por uso. Es más que posesión de una clave, si se verifica correctamente | Contrato del verificador host: cadena/raíces, challenge, clave enrolada, propiedades respaldadas por hardware, revocación y fallos/offline. Evidencia Android física y TS-SEC-MOB-01 **NOT RUN**; no se presume compatibilidad del teléfono del propietario |
| IR-11 MEDIUM | **PASS_SPEC** de la política por defecto | 34 §5, línea 256, y 42 IR-11 pausan trabajo no iniciado dependiente del dispositivo revocado y no prometen matar procesos nativos ya activos | ER-08. Origen durable de cada autorización y orden entre revocación/dispatch/resume; TS-SEC-ORC-01 **NOT RUN**. No equivale a recuperar integridad tras LP4 |
| IR-12 MEDIUM | **PASS_SPEC** del rechazo provisional; función bloqueada | 36 §10, 37 §8 y 42 IR-12 agregan VAULT_LOCKED_NO_SESSION y prohíben aplicar propuestas encoladas. 16 `config.request` conserva rechazo mientras las escrituras estén bloqueadas | DN-21/31, L1/L2/L5 y custodia siguen abiertos. ER-07/09: estados y pruebas positivas todavía incompletos. TV-29/keyring y TS-SEC-CFG-01 **NOT RUN** |
| IR-13 MEDIUM | **PASS_SPEC** de la conservación de significado | Comparación literal contra `e45cdb2`: la fila original TS-CFG-17 to 20 de 20 §3.4 es idéntica. Negativos nuevos en 37 §3 usan 21..24 y 20 §15/16 los referencia correctamente | No hay pérdida del texto original, pero siguen sin cuatro casos individuales de undo ni aceptación AC-CFG-AUTH-01 definida en 20: ER-09. No confundir preservar la fila con completar sus pruebas |
| IR-14 LOW | **PASS_SPEC** | 33 §2 usa RC-01..16; 37 §4/6 usa MA-01..10; 31 conserva C1..C18 y 16 conserva MS-01..10. Las familias ya no cambian significado por compartir prefijo | Resultado estático, sin inferir unicidad global de todo el repositorio. TS-SEC-TRACE-02 es un plan, no una ejecución |
| IR-15 LOW | **PASS_SPEC** | 36 §10, líneas 237–241, limita 41 a control negativo entre mounts. 20 TS-SEC-EVID-01 y 23 §7 conservan DN-31 abierto | No exige otro experimento para corregir esa atribución documental. La seguridad real del filesystem continúa **NOT_PROVEN** |

**Ninguno de los 15 hallazgos obtiene aquí un PASS de ejecución.** Varias correcciones sí
resuelven su error textual concreto; las filas no deben sumarse como porcentaje de seguridad.

## 4. Defectos pendientes y contradicciones de las correcciones

### ER-01 — CRITICAL: el stop de OL3 no resuelve la coexistencia con cualquier proceso del mismo UID

**Afecta:** IR-01/03, FR-ORC-04/07, FR-SEC-11; 05 §4.2/17.1, 10 §2, 33 R2,
34 §3.3/§7, 42 C-A01; ADR 0011/0018/0020.

**Observado:** 42 identifica correctamente el problema. El bloqueo se expresa principalmente
para el despacho automático nativo; el producto sigue ofreciendo agentes manuales con derechos
del usuario y LP4. `server.rs:121–132` comprueba UID, y `server.rs:182` aplica esa comprobación.
ADR 0018:119–121 admite explícitamente que el usuario puede conectar, conceder leases y aceptar
aprobaciones. No hay una nueva identidad autenticada por añadir un campo a `grant_step`.

**Contraejemplo por razonamiento contractual, no ejecutado:** iniciar manualmente un agente
nativo, mantener una aprobación Local Tools pendiente y comprometer el agente mediante contenido
del proyecto. El stop del coordinador no participa en su conexión al socket de la autoridad.
Puede presentar el intent de aprobación con los identificadores/revisión que la propia API le
permita obtener. Esto no elude necesariamente la revalidación del comando: suplanta quién lo
aceptó, que es precisamente la propiedad reclamada por TS-ORC-11.

**Impacto:** una ejecución gestionada puede tener un Accept válido en su base y carecer de
consentimiento humano. La separación lógica de capas y el audit no prueban identidad humana.
LP4 también puede alterar archivos, servicios o el estado accesible del usuario; ocultar solo
un pathname de socket no cubre todos esos caminos.

**Contrato mínimo pendiente:** inventario de principales y activos protegidos, autenticador y
emisor de aprobación, integridad de código/estado/claves, IPC, descriptores heredados, servicios
de usuario y canales alternativos. Cubrir nativos manuales, externos, OL3 y LP4. Toda afirmación
de no suplantación debe estar condicionada a esa frontera. Si se conserva el modelo de cuenta
compartida, declararla una limitación funcional de confianza, nunca una aprobación infalsificable.
El ensayo futuro debe probar ambos caminos: aprobación legítima aceptada y suplantación rechazada.

### ER-02 — HIGH: no hay contrato que preserve la capacidad nativa y el control remoto ausente

**Afecta:** DN-08/14/15/44/46, FR-NAT-11, 05:20–23 y §17.4, 32 §2,
35 §1/7, 42 §1/3.

**Observado:** el nativo original equivale al programa invocado a mano, con shell, hooks,
configuración y credenciales propias. 42 pide un principal restringido que conserve funcionalidad,
pero no especifica una matriz de capacidades. Un nativo restringido deja de ser irrestricto;
el mero cambio de nombre no preserva el contrato original. 42 reconoce que el bloqueo es temporal,
lo cual evita una eliminación explícita, pero todavía falta una vía concreta de aceptación final.

**Contraejemplos:** aislar el home para impedir acceso a autoridad/configuración puede ocultar
autenticación, hooks, MCP o servicios requeridos. Copiar credenciales para repararlo contradice
DN-14. Además, el propietario sale de casa, cierra KDE y conserva un host despierto con lingering:
la nueva denegación de LP4 fuera de login le permite observar pero no controlar esa terminal.
Presencia local de login tampoco equivale a una persona físicamente vigilando el equipo.

**Impacto:** aprobar solo serialización, handoff manual y watch remoto entregaría un alcance menor
que el seleccionado. La aprobación por lanzamiento de IR-03 cambia la modalidad original de
OL3 que permitía aprobar un pipeline acotado una vez (17 §8, fila OL3).

**Contrato mínimo pendiente:** capacidades exactas conservadas/perdidas por proveedor y perfil,
incluidos login/refresh sin copia, hooks obligatorios, archivos/proyectos, red, plugins y procesos
auxiliares; alternativa para proteger la autoridad sin presentar al agente como aislado si no lo
está; criterios positivos para trabajo útil. Modelar por separado OL3 gestionado, OL3 nativo y
LP4-away. El propietario decide el compromiso real después de esa evidencia, no ahora a ciegas.

### ER-03 — HIGH: concesión, claim y entrega de entrada no forman aún un protocolo único

**Afecta:** IR-02/06, FR-REM-12/16; 16 §4.4/4.6/5.2.4/5.2.5 y RT-05;
35 §6/7; 42 IR-06; 20 TS-HYB-03/TS-SEC-REM-03.

**Observado:** 16:602 propone siete días para `terminal.write`; 16:692 y 35:210 proponen
claims de hasta quince minutos. Es posible diseñar un grant de elegibilidad largo y un claim
corto, pero los textos no formalizan esa distinción ni todas las invalidaciones. No afirmo que
dos TTL distintos sean por sí solos una vulnerabilidad: falta decidir cuál limita qué y comprobarlo
en cada método. Crear/cerrar una terminal y arrancar un agente también son efectos LP4
(16:657), aunque el nuevo stop se describa como prohibición de escribir bytes.

16:614 y RT-05:760 todavía prometen repetición del resultado sin segundo efecto usando memoria
de 24 horas limitada a 100.000 IDs. 35:179–187 corrige el reinicio, pero la tabla operativa
`terminal.write` de 16:284 recibe attachment, epoch y bytes; no especifica número de lote,
recibo de entrada, ni su semántica frente al reinicio de host/gateway. `gateway_instance_id`
no está incorporado al contrato original de 16. El TTL del registro no demuestra deduplicación
si se llena antes, ni la época por sí sola demuestra aplicación como máximo una vez.

**Contraejemplo pendiente de ensayo:** aplicar bytes, perder ACK, reiniciar solo el gateway
mientras el host conserva el PTY, volver a reclamar y reenviar el lote. El texto nuevo ordena
rechazarlo; todavía no define la identidad/estado que lo impide en el escritor. Dentro del mismo
arranque, la saturación o una escritura parcial también requieren un resultado definido.

**Contrato mínimo pendiente:** una sola especificación de grant/claim y efectos cubiertos;
identidad de arranque de gateway y host, attachment, claim y secuencia autenticados hasta el
escritor; dueño del ledger de entrada, punto de aceptación/aplicación y ACK, bytes parciales,
evicción/saturación, reconexión, revocación y lock/logout concurrentes. Mantener
OUTCOME_UNKNOWN sin replay automático. La entrega exactamente una vez no puede prometerse
para una shell arbitraria; los recibos durables de intents gestionados son otra propiedad.

### ER-04 — HIGH: negar shell opaca no define todavía el motor de políticas seguro

**Afecta:** IR-05, DN-06=D, FR-AGT-04; 17 §6/14, 40 §2/5, 42 IR-05.

**Observado:** se exige vocabulario cerrado, pero no se enumeran operaciones, schemas, operandos,
targets, credenciales delegadas ni efectos transitivos. `Action(type: str, target: str,
parameters: Mapping[str, object])` del Wyss original no es esa lista cerrada. El motor asigna
nivel al tipo desconocido; eso no equivale al rechazo incondicional exigido para esta persona.
El dispatcher por prefijo tampoco es por sí mismo un autorizador de operaciones. Véase §5.

**Contraejemplo de diseño:** si la futura lista incluye escritura de archivo y una operación
tipada que ejecuta pruebas o Git, código/hook/configuración del proyecto podría producir
subprocesos o red indirectamente aunque no exista una herramienta llamada `shell.run`.
Es una hipótesis sobre una lista aún no definida, no un exploit demostrado del producto actual.

**Contrato mínimo pendiente:** catálogo cerrado por versión, schemas sin argumentos libres,
principal y raíces autorizadas por acción, validación del objeto realmente ejecutado, bindings
de plan/política/consentimiento y revalidación, política de retries y efectos desconocidos.
Enumerar callbacks/hooks/intérpretes transitivos, egress y custodia. Probar acciones útiles
permitidas y las variantes negativas. Una lista vacía que rechaza todo no satisface Wyss Agent.

### ER-05 — HIGH: la corrección gestionada aún no llega a toda su aceptación

**Afecta:** IR-04, DN-08=A/DN-42=C, FR-ORC-06; 34 §4.1/7, 20 §14,
23 §5; ADR 0012/0015/0031.

**Observado:** 34:225–231 y TS-ORC-04 están correctamente bloqueados y serializados.
Pero 34:299–300, TS-ORC-14, sigue pidiendo prueba de carga de runtimes gestionados independientes
sin esa condición local, y 20:599, TS-ORC-04/AC-FINAL-06, exige dos sesiones concurrentes sin
explicitar el ADR pendiente. La adenda TS-SEC-MAN-01 no define qué criterio acepta qué baseline.

**Contraejemplo:** una implementación que mantiene el índice único pasa el nuevo caso serial
y no puede pasar AC-FINAL-06 como concurrencia real. Relajar el índice para pasar ese criterio
cambiaría una garantía aceptada sin el nuevo ADR.

**Contrato mínimo pendiente:** criterios separados para la base serial vigente y la capacidad
final concurrente, ambos con precondiciones explícitas; identidad por runtime/sesión/turno,
catálogos, credencial compartida/refresh, admission, fairness, aprobación, cancelación y migración
de bindings. Selección de modelo debe ser una extensión distinta de observación/reroute.

### ER-06 — HIGH: proceso parser separado no basta para garantizar supervivencia de PTYs

**Afecta:** IR-07/09, DN-47; 35 §2/5, 42 IR-07/09, TS-SEC-HOST-01/02.

**Observado:** se exige helper de bajo privilegio, pero no se fija su principal, los FD que
hereda, el IPC ni el árbol de recursos. Dos PID bajo un mismo dominio de muerte por OOM
pueden seguir fallando juntos. El kernel documenta que `memory.oom.group` puede matar todas
las tareas del cgroup y descendientes como una unidad; la separación efectiva depende del
dominio donde se dispara el OOM. Es una posibilidad de configuración, no una observación de
este equipo. [Documentación de cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files).

**Contraejemplo por validar:** helper y supervisor en un mismo dominio OOM, salida hostil
agota memoria, desaparecen los masters pese a haber procesos distintos. Un helper comprometido
con acceso residual al master o al IPC de control tampoco está contenido por ser otro PID.

**Contrato mínimo pendiente:** propiedad exclusiva del PTY master, allowlist de FD y mensajes,
principal/permisos y límites por helper, presupuesto del supervisor y colas, tratamiento del
OOM del helper frente al del sistema, reinicio acotado y estado visual degradado. Pruebas de
crash/OOM/IPC malformado con PTY y hermanos vivos; sin prometer supervivencia a cualquier OOM
global. Para nativos irrestrictos, el scope permite contabilidad, no prueba ausencia de
persistencia fuera de él.

### ER-07 — MEDIUM: los estados nuevos no están integrados al ciclo de vida y la UX

**Afecta:** IR-07/08/12, FR-ORC-05, FR-REM-13/17, FR-DAT-06;
16 IF-HOST/SM-HOST, 34 SM-ORC, 35 §4, 37 §8, 13 §9.6/9.7.

**Observado:** keepalive aparece en las reglas de inactividad, pero IF-HOST no contiene su
operación, TTL, propietario ni reglas ante pérdida. SM-ORC no incorpora HOST_UNAVAILABLE ni
especifica su mapeo a PAUSED/NEEDS_REVIEW. El contrato UX 13 carece de los nuevos estados
BLOCKED_TRUST_BOUNDARY, ORPHANS_POSSIBLE, OUTCOME_UNKNOWN y VAULT_LOCKED_NO_SESSION.
13:1041 todavía dice que tras reiniciar el servicio las terminales terminaron y los agentes
se muestran perdidos; 16:374–375 exige avisar que otros procesos podrían seguir vivos.

**Contraejemplo:** coordinador cae entre pasos, no hay PTY ni viewers, vence el supuesto
keepalive. No está especificado cómo el host descubre que el journal todavía contiene un
pipeline pendiente, cómo decide salir o cómo reanuda sin aprobación nueva. Otro cliente puede
interpretar «lost» como ausencia de escritor cuando hay descendientes posibles.

**Contrato mínimo pendiente:** operación y máquina de estados de keepalive/reconciliación,
resultado al agotarse reinicios, ownership de locks con proceso UNKNOWN, traducción única de
los estados al cliente y acciones de recuperación permitidas. Un keyring bloqueado nunca
debe resolverse suponiendo que logout significa keyring abierto o que desbloquearlo autoriza
una propuesta remota vieja.

### ER-08 — MEDIUM: revocación de pipeline sin relación de autorización ni orden de admisión

**Afecta:** IR-11; 34 §2/3/5, 35 §6; 42 IR-11.

**Observado:** RunJournal menciona orígenes de decisión, pero no define la relación durable
dispositivo→aprobación→plan/revisión→step/attempt ni qué ocurre con varias aprobaciones, una
reaprobación local o la revocación concurrente con STARTING. No se declara un punto de orden
entre el registro de revocación del gateway y el dispatcher del coordinador. El valor por
defecto de pausa está corregido; la condición de aplicación todavía debe ser verificable.

**Contraejemplo pendiente:** se decide revocar después de verificar un grant pero antes de
spawn; el step aún no estaba RUNNING. Las frases «pausar trabajo no iniciado» y «pueden seguir
los procesos ya activos» no deciden esa carrera.

**Contrato mínimo pendiente:** linaje durable mínimo, revisión de grant, orden/punto de admisión,
tratamiento de STARTING y errores, reglas de resume con nueva autorización. Probar ambos
órdenes de la carrera; identificar efectos ya admitidos sin llamarlos deshechos.

### ER-09 — MEDIUM: preservar identificadores no completa las pruebas originales de autoría

**Afecta:** IR-12/13, FR-HRS-04/05, FR-CFG-18/19;
20:87, §15/16; 23:317 y 360–364; 37 §3.

**Observado:** la fila TS-CFG-17..20 está preservada literalmente y los nuevos negativos ya
no la sustituyen. Sin embargo, sigue siendo una fila agrupada cuyo campo de fallo es «—».
`AC-CFG-AUTH-01` se referencia en 23, pero no se define en 20. `AC-CFG-AUTH-02` solo se
asocia a los negativos. TS-SEC-CFG-01 verifica keyring cerrado/no autoaplicar, no restauración
correcta. Estos son huecos de aceptación funcional, no problemas de tipografía.

**Contraejemplo:** pasar rechazo de comandos de la página y vault bloqueado sin implementar
undo exacto, ni advertir antes de editar/borrar que no existe undo. Ninguno de esos negativos
demuestra las aserciones originales de TS-CFG-17..20.

**Contrato mínimo pendiente:** cuatro casos originales individualizados, fixtures/versiones,
condiciones positivas y de fallo, origen exclusivo desde vault para undo y definición de
AC-CFG-AUTH-01. Separar preparación de `config.request` de aplicación: qué parte funciona
sin filesystem/keyring y dónde debe detenerse. Conservar DN-21/31 y ADR 0029/0030 cerrados
hasta evidencia/aceptación; no reintroducir escritura directa como remedio documental.

## 5. Motor de políticas: fuente efectivamente contrastada

Se accedió en lectura al repositorio privado `SaintWyss/wyss` mediante GitHub, fijando
**`0761dcdacdafa216c8afc25036e24685855d2228`**. No se ejecutó código importado ni se usaron
políticas, comandos o credenciales operativas del propietario.

| Fuente fijada | Observación estática | Límite de lo que demuestra |
| --- | --- | --- |
| [policy/engine.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/packages/wyss-core/src/core/policy/engine.py), líneas 40–62, 77–90, 99–140 | Evalúa plan completo, combina riesgos hacia arriba, compara tipo/target por fnmatch, parámetros por igualdad y substrings de args | Confirma un clasificador textual; no demuestra equivalencia semántica entre comandos |
| [policy/builtin_deny.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/packages/wyss-core/src/core/policy/builtin_deny.py), líneas 20–134 | Reglas incorporadas para archivos sensibles y diversas formas de shell | Son defensas reales; la ausencia de match no prueba inocuidad de los efectos |
| [domain/plan.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/packages/wyss-core/src/core/domain/plan.py), líneas 27–55 | Action/Plan tienen nombres de tipo y target string y parámetros genéricos | No constituyen el catálogo cerrado que propone 42 para la persona Hermes |
| [execution/shell.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/src/execution/shell.py), constructor/execute | Binarios permitidos explícitamente; lista vacía por defecto; argv sin expansión de shell y guard de ejecución | No sería correcto afirmar que cualquier string aceptado por el motor se ejecuta. Si un intérprete está permitido, shell=False tampoco interpreta su programa para probar efectos seguros |
| [execution/filesystem.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/src/execution/filesystem.py), _second_wall/execute | Resuelve ruta, vuelve a autorizar y comprueba redirección del último componente | Evita atribuir al motor aislado una ausencia de protección que el adapter intenta cubrir; no sustituye la evaluación completa de efectos y carreras |
| [execution/executor.py](https://github.com/SaintWyss/wyss/blob/0761dcdacdafa216c8afc25036e24685855d2228/src/execution/executor.py), líneas 142–165, 171–226 | Dispatch por prefijo y retries de adapters | Requiere contrato específico de acciones/retries para Hermes; no es evidencia de autorización independiente por el nombre de una acción |

**Conclusión acotada:** IR-05 tiene fundamento estático. No queda probado un bypass de todas
las defensas del Wyss original. Tampoco queda probado que el diseño corregido controle todos
los efectos simplemente por negar `shell.run`. E-HRM-09 debe ejercitar la cadena completa
propuesta, además del evaluator aislado, y comprobar que una variante rechazada no se puede
expresar mediante otro adapter o callback permitido.

## 6. Revisión de las 29 filas de subsistemas

Reevaluación documental acotada a los contratos y referencias inspeccionados. No se heredan
como evidencia los porcentajes ni los PASS históricos de 31. Esta tabla no certifica revisión
línea por línea de cada implementación ni cobertura global de todo el repositorio.

Dimensiones: **R** requisitos, **D** profundidad de diseño, **Dc** decisiones, **T** trazabilidad,
**V** especificación de verificación. C=COMPLETE en papel para el objeto indicado;
P=PARTIAL; B=BLOCKED; ND=NOT DESIGNED. C nunca significa código o pruebas aprobados.

| # | Subsistema | R | D | Dc | T | V | Fuente inspeccionada / bloqueo restante |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Terminal existente | C | C | C | P | P | 05 §1; ADR 0005/0006/0007; 20 §4. Referencia funcional/performance no reejecutada |
| 2 | Workspace, panes, búsqueda, teclas, layout | C | P | B | C | P | 05 §10–17/22, 13/20. ADR workspace, teclado/IME y reattach real pendientes |
| 3 | Lanzador nativo | C | P | B | P | P | 05 §4/17, 34/42. ER-01/02: principal y funcionalidad nativa |
| 4 | Registro y supervisión nativos | C | P | B | P | P | 05 §16, 16 §4, 35 §4. Orphans, locks y ER-06/07 |
| 5 | Plano gestionado vigente | C | C | C | C | C | 10 §1–3; ADR 0011–0018, 0020/0023; server.rs. C solo para su alcance explícito que excluye atacante mismo UID; sin revalidación runtime |
| 6 | Inference independiente de hooks | C | P | B | C | P | 10 §5; ADR 0016/0028. E1..E9 y ADR G-INF pendientes |
| 7 | Escala gestionada / segundo agente | C | P | B | P | P | 02 FR-MAN-09, 10 §6, 34 §4.1, 20/23. ER-05; no contrato completo del segundo agente |
| 8 | Adaptadores y evidencia por versión | P | P | B | P | P | 06 §1–3, 37 §2. Manifiesto instalado, precedencia y round trip sin observar |
| 9 | Inventario y watcher de configuración | C | P | P | C | P | 07 §1–4/7–8, 20 §3. Límites/parser/inotify/versiones pendientes |
| 10 | Escritura y concurrencia de archivos | C | B | B | C | P | 07 §5.4, 28 §5–7, 30, 36 B-P0-01. L1/L2/L5, DN-21/31, B1 |
| 11 | Switches nativos | C | P | B | C | P | 06/08, 20 §3, 27/28. Compatibilidad reversible y writer pendiente |
| 12 | Vault, copias y exportación | C | P | B | P | P | 27 §8/9, 14, ADR 0029. Custodia, no-session, snapshot/sync y ER-07/09 |
| 13 | Mutaciones Codex | C | P | B | C | P | 10 §4/5, 06/08, ADR 0028. G-INF y escritura bloqueados |
| 14 | Reglas/hooks/skills/MCP y autoría | C | P | B | P | P | 08, 37 §3, 20:87/§15, 23 §6. ER-09; undo positivo incompleto |
| 15 | Adaptadores OpenCode/Hermes | P | P | B | C | P | 37 §2, 06, 40. Cobertura por operación/versión aún parcial; acciones admitidas no probadas |
| 16 | Perfiles | C | P | B | P | P | 09 §3–4, 05 §17, 32 DN-14/20. Efectos de excluir controles y ER-02 |
| 17 | Orquestación y handoff | C | P | B | P | P | 34/37, 20 §14–16. ER-01/02/05/07/08; persistencia de handoff es candidato separado |
| 18 | Modelos | C | P | B | C | P | 12 SP5/overlay, ADR 0031, 34 §4.1. Observación distinta de selección; provider/ADR pendientes |
| 19 | Datos y recuperación | C | P | B | P | P | 14 §3–4, 27 §8, 37 §5. Políticas de metadata, keyring y recuperación real |
| 20 | Modelo de amenazas | C | P | B | P | P | 11, 16 RT, 33/42. Nuevas amenazas dispersas; ER-01/03/04/06 sin frontera cerrada |
| 21 | Interfaces | C | P | B | P | P | 13 §9.6/9.7, 37 §4/6. Estados de ER-07 y mockups/accesibilidad pendientes |
| 22 | Varios computadores | C | P | B | C | P | 16 §2, 27 §9, 30/36. DN-31 y comportamiento real de réplica/snapshot |
| 23 | Terminales persistentes del host | C | P | B | P | P | 16 §4, 35 §2/4/5. ER-06/07; ADR host/parser y mediciones |
| 24 | Remoto, dispositivos y Android | C | P | B | P | P | 16 §5–8, 35, 42. ER-02/03/08, DN-05, atestación y compatibilidad física |
| 25 | Wyss Agent / Hermes | P | P | B | P | P | 17 §6/14, 40, fuente Wyss fijada. ER-04; vocabulario funcional incompleto, aislamiento y utilidad NOT_PROVEN |
| 26 | Entrega y mantenimiento personales | P | P | B | C | P | 32 DN-07, 36 §4, ADR 0027 y 20/23. Aceptación personal aún candidata; paquete/upgrade/rollback no verificados; público condicional |
| 27 | Performance | C | P | B | C | P | 15, 05 §18, 35 §5, 36 G4. Cotas numéricas y nuevos procesos/parser sin medir |
| 28 | Observabilidad y diagnóstico | C | P | B | P | P | 14 §7, 37 §5, 20 TS-OBS/TS-MET. Retención y stores todavía candidatos; no contenido sensible |
| 29 | Accesibilidad | C | P | B | C | P | 13 §5/9, 37 §6, 20 §15. WebKitGTK/Android, temas, IME y estados nuevos no comprobados |

No se calcula un porcentaje: el objeto de esta pasada es la corrección y viabilidad del
contrato, no repetir el indicador histórico 64,2%.

## 7. Decisiones realmente necesarias del propietario

No corresponde pedirle que elija parámetros que todavía puede resolver la ingeniería.
Sí deberán volver a él los siguientes compromisos, con alternativas concretas y evidencia:

1. **Confianza y funcionalidad nativa:** qué capacidades de cuenta deben conservar los agentes
   dentro de OL3, y si acepta que la coexistencia irrestricta permita suplantar acciones del
   usuario cuando no haya una frontera real. Antes deben compararse aislamiento de workers y
   protección separada de la autoridad, con una matriz funcional. Una aceptación de riesgo
   no convierte en verdadera la propiedad de aislamiento.
2. **Modalidad OL3 nativa:** confirmar si la revisión local por cada intento y payload sigue
   satisfaciendo su objetivo de pipeline aprobado una vez. El bloqueo provisional es correcto;
   sustituir definitivamente OL3 nativo por handoff manual o solo gestionado exige su decisión.
3. **Control remoto fuera de presencia local:** si necesita LP4 después de lock/logout, en qué
   dispositivos y con qué exposición residual. LP4-away es una propuesta nueva, no una
   capacidad ya aprobada. Grant/claim, TTL, pérdida/robo y recuperación deben presentarse juntos.
4. **Android sin evidencia admitida:** solo si su dispositivo real no puede satisfacer el
   verificador, elegir compatibilidad limitada o aceptar explícitamente un perfil con garantías
   menores. No presuponer ese resultado ni rebajar automáticamente a posesión de clave.
5. **DN-05:** elección H1/H2 u otra alternativa después de compatibilidad, coste operativo,
   privacidad y roaming medidos. La preferencia de UX del propietario ya existe.
6. **DN-21/31:** únicamente si no aparece un writer/layout que satisfaga sus restricciones,
   decidir el compromiso concreto entre escritura concurrente, exposición y recuperación.
   No pedir aceptación genérica de pérdida de datos o snapshots por falta de investigación.

La concurrencia gestionada seleccionada, los estados de error honestos, el rechazo de shell
opaca desde el planner y no borrar copias únicas no necesitan una nueva votación funcional.
Necesitan diseño, evidencia y, donde corresponda, aceptación formal de ADR. La aprobación
final del plan pertenece al propietario y sigue pendiente.

## 8. Gates que requieren evidencia posterior

**Todos NOT RUN en esta auditoría.** No se solicita ni se presupone permiso para ejecutarlos.

| Gate | Evidencia mínima que permitiría evaluarlo | Qué no puede contarse como PASS |
| --- | --- | --- |
| Principal/autoridad | Frontera aceptada, matriz de canales y ensayo de suplantación por nativo manual/OL3/LP4; flujo legítimo útil funciona | Negar todos los starts; GUI separada; HMAC accesible al mismo atacante; namespace solo descrito |
| Capacidad nativa | Por versión: login/refresh, hooks/policies, MCP, proyectos, egress y subprocessos bajo perfil propuesto | Probar solo que no puede conectar un socket y omitir el resto de las capacidades |
| OL3 | DAG, datos reales revisados, estados/retries, cancelación y revocación en ambos órdenes de carreras | Un ID de test o un workflow que evita todos los pasos nativos sin declarar la reducción |
| Concurrencia/modelo | Base serial preservada; luego ADR y pruebas multiruntime/catalog/lease/refresh/binding; modelo seleccionado separado del observado | Aceptar 0031 como autorización de selección; quitar índice único sin decisión |
| LP4/revocación | Dispositivo/claim/TTL/lock/logout y límites de recuperación tras persistencia en entorno descartable | Mensaje de «revocado» como prueba de cuenta limpia; presencia de login como vigilancia humana |
| Entrada remota | Reinicios independientes de gateway/host entre recepción/escritura/ACK, bytes parciales, reordenamiento, evicción y reconnect | RAM de IDs como garantía durable; replay de una shell incierta para averiguar su resultado |
| Host/parser | Cgroups/principales/FD/IPC fijados, fallo y OOM por terminal, supervisor y PTYs hermanos vivos; logout/escape en VM | Dos PID; cgroup vacío como ausencia de toda persistencia; fuzz solo del parser sin supervisor |
| Android | Verificación en host de cadena/challenge/key/properties/revocación; pruebas físicas de éxito y rechazo, lifecycle/reinstall | Campo booleano declarado por el cliente; emulador como prueba hardware |
| Hermes/Wyss | Catálogo de efectos completo, imagen y policy fijas, bypass/aliases/callbacks denegados; planes útiles admitidos | Menos toolsets; negar solo el nombre shell.run; corpus solo contra globs |
| Filesystem/vault | TV-29 y writer exacto, sync/snapshots, keyring, races/crash/rollback; entorno del propietario solo con autorización específica | EXDEV de 41 como aprobación DN-31; marker o ignore como garantía universal |
| Datos y UX | Errores completos, undo positivo, estados nuevos, journals sin contenido, límites y accesibilidad por superficie | Lista de IDs, mockups felices o quotas que destruyen recuperación |

La [guía oficial de atestación Android](https://developer.android.com/privacy-and-security/security-key-attestation)
requiere validación fuera del dispositivo, cadena confiable y comprobación de revocación.
Esto respalda la exigencia de un verificador concreto; no demuestra que el cliente Wyss ya lo
tenga ni que el teléfono del propietario sea compatible.

## 9. Trazabilidad y alcance que deben conservarse

- **Resuelto en papel:** TS-CFG-17..20 conserva literalmente su definición anterior;
  TS-CFG-21..24 agrega negativos distintos; RC/MA evita colisiones con C/MS; 41 ya no satisface DN-31.
- **Pendiente:** AC-CFG-AUTH-01 sin definición en 20; originales de undo agrupados sin fallo
  individual; AC-FINAL-06/TS-ORC-14 sin precondición uniforme de ADR futuro; TS-ORC-11 sin
  condicionar la no suplantación; estados de 42 sin contrato completo IF/SM/UX.
- **No eliminar:** concurrencia real, ejecución OL3 nativa útil, control remoto requerido,
  screen fidelity, configuración reversible y Wyss Agent útil bajo su política. Las etiquetas
  de bloqueo permiten detener trabajo inseguro, pero no satisfacen estas capacidades.
- **Persistencia y privacidad:** el journal metadata-only no es un almacén del DAG/prompt/handoff
  completo. Si la caída pierde el payload revisado, no reconstruirlo silenciosamente ni considerar
  que un digest basta para ejecutar; debe pasar a revisión/bloqueo o usar un store separado
  expresamente diseñado y aceptado. No se ha probado recuperación de contenido aquí.
- **Modelo de amenazas:** 11 conserva límites útiles del mismo usuario, pero las amenazas
  nuevas siguen repartidas entre 16/33/34/35/40/42. Debe existir una relación explícita de
  amenaza, control real, límite, test y gate para suplantación, persistencia LP4 y policy deputy.

## 10. Proveniencia, límites y cambios de esta pasada

**Documentos contrastados:** informe post-owner de 2026-10-13, 42 y 38; 00, 02, 05, 06, 07,
08, 09, 10, 11, 12, 13, 14, 15, 16, 17, 20, 21, 23, 27, 28, 30, 31, 32, 33, 34, 35,
36, 37 y los pasajes pertinentes de 40/41. Lectura focalizada por sección, no afirmación de
inspección exhaustiva de cada línea de todos esos documentos. Se inventariaron los estados
de ADR 0001..0031 y sus decisiones pertinentes; se contrastaron directamente los pasajes de
frontera/garantías listados en §2.1. Las bitácoras extensas de validación de los ADR no se
reprodujeron ni se aceptaron como pruebas nuevas.

**Commits:** objeto principal `906822fd7b87b71728198a11672831ff41f7713e`;
informe previo `00b18fac46f17a36d53ed5f30cc675b337dd30a1`, que revisó `e45cdb2`;
comparación original de TS-CFG contra `e45cdb2`; fuente Wyss externa fijada en §5.
El historial de correcciones desde `7251d1b` hasta `906822f` se inspeccionó para autoría y alcance.
El código Console leído puntualmente pertenece al mismo HEAD auditado.

**Operaciones:** status/log/show/fetch/avance directo de la rama, lectura con rg/sed/nl/Python,
consulta de metadatos de la PR y fuente fijada mediante `gh`, lectura de documentación oficial
del kernel/Android. La rama local estaba limpia y atrasada; se avanzó sin reescribir commits
ni descartar trabajo del usuario. Los controles Python fueron comparaciones documentales,
no pruebas de la aplicación ni ejecución del motor externo.

**Sin implementación ni experimentos:** no se ejecutaron proveedores, tests, builds, servicios,
VMs ni ensayos en Android; no se leyeron valores de credenciales ni configuraciones personales
del producto; no se aceptó/editó ADR ni se cambió configuración. No hubo subagentes ni contacto
con terceros. Las fuentes externas se consultaron en lectura. No se modificó `main` ni se
fusionó la PR.

**Único cambio de contenido autorizado en esta pasada:** este nuevo informe. Se entrega en
un único commit documental. Este veredicto mantiene abiertos los bloqueadores de arquitectura
y la aprobación final; no convierte el REJECT anterior en una aprobación del producto.
