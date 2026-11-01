# Wyss Arsenal — revisión independiente y correcciones offline

Base auditada: `43ed2fd071735899bdfcc8fa26dddabdab545661`.
Rama de trabajo: `fix/arsenal-a0-fail-closed`, creada desde el `main` local vigente.

**A0 con proveedores reales: NO-GO. AM1a offline: GO para comenzar su implementación.**
No se implementó AM1a. No se ejecutaron proveedores reales ni se usaron credenciales reales.
No se cambiaron configuraciones globales ni se hizo push o merge. Los cambios del bridge son
exclusivamente de sus herramientas de prueba; el launcher Native del producto no cambió.

## 1. Independencia y evidencia

La auditoría del baseline fue independiente del autor de harness v2. Después, el usuario
amplió la autorización para corregir defectos. Las correcciones y sus tests son trabajo del
revisor de esta sesión: **sus resultados no constituyen una revisión independiente de los cambios**.

Se leyeron el diseño, plan A0, informe de aislamiento, paquete de revisión, errata y
remediación, junto con el código primario y los tests. El run-root original fue legible.

Comprobaciones sobre los originales:

- Los **1.356 archivos** enumerados por el manifiesto v2 coincidieron con sus hashes.
- Los **8 documentos originales** enumerados también coincidieron.
- Los **500 archivos** de `pre-remediation-tree.sha256` coincidieron.
- El manifiesto del repositorio es semánticamente igual al original; su formato difiere.
  No se compararon hashes de copias formateadas como si fueran los originales congelados.
- Las capturas primarias de A8 muestran S1 = `{alpha, proj}`, S2 = `{beta, gamma}`;
  normalmente aparecen además 13 built-ins. Los errores A2-F1..F5 muestran exposición del
  catálogo completo. Esto respalda las observaciones históricas, con las limitaciones siguientes.

El congelamiento posterior de capturas v1 **no prueba su integridad en el momento de captura**.
Las 24 capturas históricas no tienen stream autenticado v2; permanecen como observaciones,
no como casos recertificados. No se reejecutaron pruebas con proveedores para esta revisión.
Los resultados antiguos de driver, preflight y e2e se inspeccionaron inicialmente como evidencia
registrada; las nuevas ejecuciones offline se distinguen en la sección 4.

## 2. Fallos y correcciones

Las referencias del baseline nombran el código que originó el hallazgo; las referencias de la
rama nombran el punto corregido. `BLOCKING` se refiere a calificar una nueva campaña A0,
no al desarrollo de la biblioteca offline.

| Hallazgo original                                                                         | Clase                                      | Causa raíz                                                                                                                                 | Corrección en esta rama                                                                                                                                                             |
| ----------------------------------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Controles vacíos o rangos nulos podían producir PASS                                      | BLOCKING                                   | `oracle.py:33` del baseline comprobaba presencia de claves, no sus valores; se reproducían PASS con listas vacías o `requests=[None,None]` | `harness-v2/oracle.py:79`: tipos/rangos, claves cerradas, controles efectivos y canarios conocidos. `run_case.py:155`: lint antes de crear fixtures o lanzar procesos               |
| Stream íntegro sin FIXTURE podía producir PASS                                            | BLOCKING                                   | `oracle.py:93` del baseline exigía FINAL y ausencia de problemas criptográficos, no el ciclo completo; aceptaba archivos exportados extra  | `harness-v2/oracle.py:165`: START/FIXTURE/SUT_DONE/FINAL/END únicos y ordenados, forma básica de registros, exportación de requests y conjunto exacto de archivos                   |
| Timeout interno, error de harness/teardown o salida inesperada podían producir PASS       | BLOCKING                                   | El cierre criptográfico se confundía con una ejecución válida; el timeout externo era el único gate adicional                              | `harness-v2/oracle.py:197`: errores y estado desconocido impiden PASS; las salidas no cero requieren expectativa explícita                                                          |
| Identidad de namespace ausente y árbol no observado se trataban como aislamiento correcto | BLOCKING                                   | `launch.py:67` y `run_case.py:218` del baseline convertían ausencia de observación en lista vacía/True                                     | `harness-v2/launch.py:70`: rechazo antes de enviar la clave. `run_case.py:133`: exige namespace, árboles observados y launcher completado                                           |
| Clave HMAC dependía de Yama sin gate                                                      | BLOCKING                                   | El secreto vive en memoria del harness dentro del sandbox, con el mismo UID que el proveedor                                               | `harness-v2/limits.py:35` y `launch.py:45`: se exige `ptrace_scope` 1, 2 o 3 antes del spawn; valor 0, desconocido o ilegible rechaza. No se modifica el sysctl                     |
| Pantalla/drenaje incompletos no impedían PASS                                             | BLOCKING para casos PTY                    | El contador de operaciones desconocidas y EOF no llegaban al oráculo; IRM/DECOM/autowrap/reset se ignoraban como si fueran inocuos         | `case_inside.py:133`, `oracle.py:197`, `screen.py:117`: registra EOF/pending/unknown, exige salida limpia y respuesta visible; operaciones no soportadas invalidan la certificación |
| Preflight podía devolver éxito con un launcher fallido o control negativo inútil          | BLOCKING                                   | El retorno atendía sólo `sandbox_all_pass`, integridad y leftovers                                                                         | `harness-v2/run_preflight.py:70`: incluye salida, timeout, kill, identidad, lectores y discriminación del control host                                                              |
| Dos entradas del probe heredaban el entorno del host antes de bwrap                       | BLOCKING para reutilizar esos probes en A0 | `--clearenv` sólo actúa después de arrancar bwrap; `subprocess` heredaba el entorno para ese arranque                                      | `src-tauri/claude-bridge/probe/sandbox.py:59` y `driver.py:28`: `env={}`. El entorno explícito del hijo sigue en `--setenv`                                                         |
| Reproducción requería layout local y referencias no incluidas en las copias publicadas    | NONBLOCKING para AM1a                      | Los tests requieren `harness/run_batch.py`, capturas originales y un directorio e2e nuevo                                                  | `scripts/research/a0/run_offline.py`: crea un run-root nuevo fuera del checkout/originales, verifica hashes y prepara las referencias necesarias                                    |
| Test de clave incorrecta usaba un stream vacío                                            | NONBLOCKING                                | No ejercitaba realmente una firma con clave errónea                                                                                        | `tests/test_oracle_v2.py`: firma un registro y exige rechazo con otra clave                                                                                                         |

También se cierra explícitamente cada pipe del launcher una vez drenado; los restos del namespace
se eliminan antes de esperar a sus lectores. Los fallos de drenaje impiden aprobar el caso.

### Límite de confianza del HMAC

La clave se genera en el host, entra por stdin y no se entrega en argv/env al proveedor.
Bwrap arranca con entorno vacío, namespaces separados, HOME/tmpfs desechables, mounts de
sistema de sólo lectura y sin bind de disco del host escribible en los casos v2.

La firma autentica lo emitido por el **harness**, no convierte en hechos fiables todo lo que
el proveedor puede escribir. Los marcadores de hooks, árboles finales y logs residentes en
`/markers` siguen siendo contenido accesible al SUT. Deben corroborarse con requests,
resultados de herramientas y controles que produzcan efectos distintos. `judge_pair` existe,
pero la futura campaña debe usarlo; aprobar dos filas por separado no prueba discriminación.

Yama restringe el acceso ptrace entre procesos del mismo UID; es una precondición ambiental,
no una propiedad intrínseca del HMAC. La documentación primaria explica las diferencias entre
los modos [0, 1, 2 y 3](https://docs.kernel.org/admin-guide/LSM/Yama.html).
Se observó valor 1 en esta máquina. No se intentó leer la memoria de proveedores ni de sesiones reales.

## 3. Pendientes y límites

| Pendiente                                                      | Clase                                                           | Motivo / siguiente unidad mínima                                                                                                                                                                                                                                                                                                                                    |
| -------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Límite agregado de memoria y procesos                          | BLOCKING para reabrir A0                                        | `limits.py:49` sólo fija FSIZE/CORE/NOFILE/CPU; tmpfs y timeout no acotan RAM anónima ni cantidad de tareas. `case_inside.py:100` captura salida sin límite durante la ejecución y la recorta después. Resolver un mecanismo de contención compatible con JS y verificarlo con un proceso falso acotado; no introducir RLIMIT_AS ni un límite por UID por intuición |
| Tabla de campaña v2 con pares y repeticiones                   | BLOCKING para reabrir A0                                        | 22/24 casos v1 no pasan el lint v2. Hace falta una tabla nueva, limitada a las operaciones de AM1b, sin reescribir el registro histórico. Mantener 3/3 y los modos H/I exigidos por el plan                                                                                                                                                                         |
| Revisión externa de este diff                                  | NOT_PROVEN                                                      | Las regresiones de esta rama las ejecutó su autor. No se las presenta como aceptación independiente                                                                                                                                                                                                                                                                 |
| Señalización de init por PID numérico                          | NONBLOCKING residual, no certificación contra carreras hostiles | `limits.py:85` conserva una ventana entre comprobar membresía y señalizar. No se reprodujo daño. El test de huérfanos demuestra conteo/ausencia posterior, no identidad exacta del sleep. No se lanzó una prueba que pudiera señalar procesos ajenos                                                                                                                |
| Operaciones VT fuera del subconjunto implementado              | NONBLOCKING                                                     | Ahora invalidan el caso cuando se detectan; no se afirma emulación VT completa ni validación visual universal                                                                                                                                                                                                                                                       |
| Plugins, permisos interactivos, concurrencia y primera request | NOT_PROVEN para el producto                                     | Las capturas anteriores no completan estas validaciones. A8 secuencial no prueba sesiones concurrentes; C headless no prueba permiso Auto interactivo                                                                                                                                                                                                               |

El límite de RAM/procesos requiere una decisión de contención y validación que excede una
corrección mecánica; se detiene esa parte conforme a la autorización del usuario. No se instala
nada, cambia la política de cgroups ni relajan namespaces para superar el bloqueo.

## 4. Pruebas y reproducción

Los probes puros anteriores al fix reprodujeron los falsos PASS. La primera versión de las
regresiones nuevas falló sobre el baseline: 17 métodos, 36 fallos de subcasos y 3 errores
(incluidos helpers todavía inexistentes). Esto no debe contarse como 39 defectos diferentes.
Los tests del entorno del bridge también se ejecutaron contra los dos archivos de `main` en
una copia temporal y rechazaron ambos puntos; después pasaron contra los corregidos.

Se ejecutaron las suites existentes en un run-root nuevo, con bwrap y proveedor **falso**:
validator 39/39, screen 30/30, oracle 24/24, driver 14/14 y e2e 19/19. Después se ampliaron
las regresiones nuevas a 23 métodos y se añadieron 2 tests del entorno del bridge.
La corrida final, sus comandos y salidas se guardan fuera del repositorio; el recibo ata los
resultados al árbol final. No se suma una corrida anterior como prueba del código posterior.

El runner reproducible acepta la ruta histórica mediante argumento. Rechaza reutilizar la
salida, escribir dentro del run-root original o del checkout, y referencias con hash distinto.
No publica capturas crudas ni claves. Requiere Python stdlib, bwrap instalado y los originales
congelados; **un clon del repositorio sin esos originales no contiene la suite histórica completa**.
Las regresiones nuevas `tests/test_fail_closed.py` se pueden correr sin esos originales.

Comandos de la verificación final de la primera remediación (desde la raíz del repositorio):

```sh
python3 -B scripts/research/a0/run_offline.py --historical-root /home/saintwyss/.local/state/wyss-a0/20261029-pilot-e98a8e --output-root /tmp/wyss-a0-review-20261029-final
python3 -B -m unittest discover -s src-tauri/claude-bridge/tests
```

El directorio de salida es de un solo uso: una reproducción posterior debe elegir otro nombre.
No se necesita reiniciar ningún servicio o sesión; los scripts modificados sólo se usan cuando
se invocan de nuevo. El Wyss instalado no fue actualizado.

## 4.1 Corrección dirigida tras REQUEST CHANGES

Esta ronda conserva las correcciones anteriores y resuelve únicamente los cuatro puntos
confirmados en la revisión del diff. Sigue siendo verificación del autor, sin aceptación
independiente del nuevo código.

- **Registros:** `oracle.RECORD_TRANSITIONS` define las fases y todas sus transiciones.
  REQUEST sólo es válido después de FIXTURE y antes de SUT_DONE; las exportaciones sólo
  después de FINAL y antes de END. Eventos obligatorios ausentes, duplicados o fuera de
  fase hacen que la evidencia sea incompleta. La producción normal de `case_inside.py`
  ya respeta ese orden; el nuevo caso e2e lo comprueba con un SUT falso y el stream real
  autenticado por el host. No se cambió el significado ni el contenido de capturas antiguas.
- **ANSI:** cada parámetro de un modo privado se procesa por separado, en orden.
  `?6;1049h` y `?1049;6h` cuentan el modo desconocido; combinaciones soportadas siguen
  restaurando la pantalla principal. El gate PTY observa ese contador.
- **Aislamiento:** las siete claves obligatorias salen del productor real `isolation_checks`:
  `integrity`, `namespace_verified`, `no_leftovers`, `no_unexpected_proxy`,
  `owned_files_unchanged`, `not_killed_for_storage`, `launcher_complete`.
  Un `False` observado implica STOP_ISOLATION; claves/valores desconocidos implican
  INCONCLUSIVE. Una alteración de evidencia conocida conserva FAIL. Un mapa completo
  de booleanos True es el único que permite continuar hacia PASS.
- **Runner:** `tests/offline-suite-contract.json` enumera suites y checks esperados.
  Los unittest reportan IDs, resultados y número ejecutado; las otras suites conservan sus
  reportes JSON. El runner exige inventario exacto y cada resultado literalmente True,
  además de exit 0. Rechaza reportes ausentes/vacíos, checks omitidos o fallidos, cero tests
  descubiertos y suites omitidas. El contrato debe actualizarse explícitamente si se agregan
  o eliminan pruebas intencionalmente.

Las regresiones dirigidas están en `tests/test_targeted_contracts.py` (18 métodos), además
de los 23 de `test_fail_closed.py`. Las primeras se ejecutaron contra el código anterior
y fallaron; sus controles positivos y negativos pasan con el corregido. El inventario
esperado completo es: 23 + 18 + 39 + 30 + 24 + 14 + 20 checks, por siete suites.
La corrida completa nueva deja reportes por suite y `summary.json` en un run-root nuevo:

```sh
python3 -B scripts/research/a0/run_offline.py --historical-root /home/saintwyss/.local/state/wyss-a0/20261029-pilot-e98a8e --output-root /tmp/wyss-a0-targeted-20261029-full
```

Las reservas de RAM/procesos, tabla de campaña y revisión externa permanecen vigentes.
Esta ronda no califica proveedores reales ni inicia AM1a, y no hace commit, push o merge.

## 5. Decisión de implementación: AM1a GO

La división **AM1a → AM1b → AM2 → AM3** es técnicamente correcta. Separa un incremento
útil de la calificación del proveedor. La corrección es que AM1a termine en una **propuesta
NO APLICADA**, sin alcanzar el launcher ni materializar nada en directorios de descubrimiento
del proveedor. AM1a no depende de que A0 quede aprobado.

Se puede implementar ya: store versionado, lectura acotada de instalaciones, importación
por copia, hashes/procedencia, duplicados, compatibilidad declarada, pestaña Arsenal,
búsqueda, detalles, presets y selecciones guardadas en estado privado de Wyss.

### Reutilización con límites

- `agentconfig/reader.rs:167`, `roots.rs:43` e `inventory.rs:87`: reutilizar lectura,
  raíces aprobadas y claves cerradas. El inventario resumido no sustituye un importador de
  árboles completos; agregar traversal acotado y revisión de recursos/scripts.
- `native/store.rs:53`: reutilizar escritura atómica y su manejo de fallos. Los blobs nuevos
  necesitan hash completo y verificación al leer. `native/profiles.rs:75` usa un hash corto
  de presentación: no copiarlo como identidad criptográfica de un árbol.
- `native/profiles.rs:17`: reutilizar patrón de versiones/puntero, no podar revisiones aún
  referenciadas por presets o selecciones. Copiar un perfil no puede conceder autoridad.
- `native/service.rs:665`: reutilizar el concepto de plan/fingerprint, no llamar al servicio
  Native para el preview offline. `preview():705` devuelve `can_start` y etiquetas `applied`:
  ese resultado no debe convertirse en la semántica de AM1a.
- `native/capabilities.rs:32`: la calificación exacta existente sigue intacta. 2.1.312 no
  recibe una calificación por ser más nuevo; el registro actual es de sintaxis y no prueba
  que un turno usara una configuración Arsenal.
- `native/profile.rs:303`: el compilador actual puede emitir `disableAllHooks` al resolver
  un perfil. AM1b deberá rechazar esa composición al combinarla con Arsenal, incluida
  herencia. No reutilizarlo como compilador Arsenal sin esa validación final.

### Importación inerte

Guardar copias en el store privado de Arsenal, fuera de `.claude/skills`, `.agents/skills`,
roots de plugins y carpetas de proyecto que el proveedor descubra. Cada revisión conserva
bytes, lista de archivos, hashes completos y procedencia; la revisión nunca se edita in-place.

Leer sólo archivos regulares dentro de raíces aprobadas y límites de tamaño/profundidad/conteo.
Rechazar enlaces que escapen, sockets/FIFO/dispositivos y material de autenticación. Si un
árbol requiere recursos externos no admitidos, mostrarlo incompleto; no seguir referencias
arbitrarias para completar la copia. Escanear señales de secretos y mostrar exclusiones; una
heurística de secretos no prueba ausencia absoluta de secretos.

La revisión es estática: texto escapado, sin HTML activo, imágenes remotas, imports de código,
instalación de dependencias ni ejecución de scripts/hooks/MCP/plugins. Los permisos sin bit
executable ayudan a la higiene, pero no son una frontera: un intérprete puede leer un script.
Las instrucciones de una skill se muestran como datos no confiables, nunca se obedecen al importarla.
La aprobación de una revisión está vinculada a su hash y no habilita automáticamente otra.

### Estados de UI y ausencia de efectos reales

| Estado                       | Qué prueba                                            | AM1a                                                             |
| ---------------------------- | ----------------------------------------------------- | ---------------------------------------------------------------- |
| Descubierta/instalada        | Snapshot del origen, alcance y fecha                  | Sí, lectura solamente                                            |
| Importada                    | Revisión en el store Wyss                             | Sí; no equivale a selección                                      |
| Seleccionada                 | Intención guardada para un scope                      | Sí; **NOT APPLIED** incluso si el scope nombra una sesión activa |
| Planificada para lanzamiento | Propuesta determinista y sus incompatibilidades       | Sí, sin botón Apply/Start ni credencial de lanzamiento           |
| Visible al proveedor         | Observación de esa sesión/request y revisión concreta | Desconocido en AM1a; no derivarlo de selección o argv            |
| Cargada / invocada           | Evidencia del cuerpo / llamada concreta               | Desconocido en AM1a; separar lectura/carga de invocación         |

Las filas deben conservar procedencia y evidencia; no son una progresión automática.
Una skill oculta del catálogo puede seguir siendo legible por filesystem. Un padre con una
skill no demuestra que un hijo la recibió o cargó. Compatibilidad declarada y dependencia
estática no significan soporte verificado de una versión exacta.

Garantía de AM1a: sus comandos backend sólo pueden leer los orígenes aprobados y escribir
bajo Arsenal. No exponen spawn, `native.start`, writer de configuración, señalización,
materialización en proyecto, hooks ni recarga de sesiones. Las selecciones existentes no
se conectan al Start Native ni siquiera de forma implícita. Las pruebas usan un executor
que falla ante cualquier spawn y verifican árboles de origen/configuración byte a byte en
fixtures; no atribuyen a Arsenal escrituras de sesiones reales que continúan trabajando.
Managed conserva su perfil fijo. Codex/GSD pueden catalogarse como datos, sin activar sus CLIs.

## 6. PRs propuestos de AM1a

| PR     | Alcance                                                                       | Criterio de aceptación                                                                                                                                                                                                                           |
| ------ | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| AM1a-1 | Store, revisiones y copia local de una skill; procedencia y revisión estática | Importar una fixture y reabrirla conserva bytes/hash; cambios de origen no modifican la revisión; rechazo de traversal/enlaces externos/especiales/oversize; fallos de escritura no avanzan punteros; ningún spawn ni escritura fuera de Arsenal |
| AM1a-2 | Pestaña Arsenal, descubrimiento, búsqueda, detalles y duplicados              | Mostrar instalaciones sin invocarlas; dos árboles idénticos se reconocen por hash; mismo nombre con bytes distintos no se fusiona; descripción/texto malicioso se representa inerte; navegación por teclado y estados de error reales            |
| AM1a-3 | Presets y selección por sesión/terminal, con revisión fijada                  | Guardar, reiniciar la app y restaurar preserva pins; una revisión nueva no reemplaza silenciosamente la seleccionada; cambios en una selección no cambian otra ni una sesión real                                                                |
| AM1a-4 | Preview efectivo offline e integración de la selección en la UI               | Flujo descubrir → importar → buscar → seleccionar → guardar → reabrir → preview **NOT APPLIED**; muestra incompatibilidades y estimaciones como tales; no habilita Start/Apply, no escribe overlays y no altera el plan Native existente         |

El slice mínimo útil entrega ese flujo para skills locales y un preset, con persistencia.
No requiere diseñar todo el modelo de composites ni todas las clases de capacidad antes de
mostrar la primera biblioteca. Las interfaces pueden reservar esos tipos sin implementarlos.

## 7. Qué bloquea AM1b, AM2 y AM3

**AM1b — Claude:** compilador Rust cerrado, inventario exacto y versión calificada, adición
mediante plugin de sesión (A-7), comportamiento de plugins preexistentes (A-6), permisos
realmente emitidos y preservación de hooks/deny en H e I. El AM1 original mezclaba todo eso
con store/importación/UI; sólo esas partes de integración siguen dependiendo de A0.

Conservar validación de token y build en `native/service.rs:771`, ampliándola a revisiones,
árboles materializados, inventario y binario. Revalidar antes de exec y rechazar drift sin
fallback silencioso a una sesión sin Arsenal. No aceptar JSON/argv/env libres desde la UI.

**Primera request:** una observación después de lanzar no impide haber enviado un catálogo
incorrecto. La compilación cerrada y la evidencia por build deben funcionar antes de exec;
un parser de transcript posterior sólo aporta observación. Si el contrato exige impedir
cualquier primera request incorrecta aun cuando el proveedor ignore una configuración válida,
esa garantía sigue **NOT_PROVEN** con Native. No inventar ni dar por aprobado un proxy/gate
nuevo para salvarla; mantener bloqueada esa afirmación hasta contar con un mecanismo previo
compatible con los límites de producto aceptados.

**AM2 — Codex:** siguen pendientes la precedencia real de `skills.config`, reglas por nombre/path,
merge con disables del usuario, proyectos/managed, primera request y aislamiento entre threads
con el daemon compartido. El soporte de adición de skills no se deduce del de exclusión.
No cambiar CODEX_HOME, copiar auth ni escribir roots de descubrimiento como fallback implícito.

**AM3 — GSD:** depende de evidencia de los proveedores y de propagación por cada frontera de
spawn. El binding documentado es de proyecto, no de sesión; el loader puede escribir al normalizar.
Faltan manifest explícito en wrappers, pins por hijo y observación de carga de cada hijo.
No modificar `.planning/config.json` ni invocar consultas GSD en AM1a: también son efectos
potenciales. Mantener el orden AM2 → AM3 como secuencia de entrega, sin bloquear AM1a por ello.

## 8. Validación mínima restante antes de una sesión Claude con Arsenal

1. Resolver la contención de RAM/tareas, crear la tabla v2 acotada y revisar en frío este diff.
   Ejecutar sus controles offline en un run-root nuevo, conservando originales.
2. Con autorización específica para proveedores, ejecutar únicamente los casos de la
   superficie que se ofrecerá: A-2/A-8, A-7 para skills importadas, A-6 para plugins,
   C-1..C-3 y permisos emitidos, brief/settings/version guard. Mantener 3/3 y H/I donde
   el plan lo exige, pares que discriminen, evidencia completa e integridad host.
   Probar D si se ofrecerán sesiones Arsenal concurrentes; de lo contrario esa modalidad
   debe rechazarse realmente, no sólo llevar una advertencia.
3. Completar y probar sin proveedor el compilador/token/drift de AM1b. La calificación es
   por operación y build; no extender resultados a flags no probados, otra versión, resume,
   name-only, presupuesto de catálogo, MCP o subagentes.
4. Resolver explícitamente la garantía de primera request descrita arriba y el canal de
   evidencia visible/cargada. Sólo entonces integrar Native y realizar la aceptación mínima
   autorizada. La inferencia real para medir calidad no es necesaria para construir AM1a.

No hace falta reabrir discovery ni repetir toda la matriz A–J para entregar la biblioteca.
Siguiente incremento recomendado: AM1a-1, una vez autorizado; no se inicia en esta sesión.
