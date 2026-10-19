# Revisión adversarial posterior a las decisiones del propietario (2026-10-13)

Alcance: **solo la planificación**. Rama `docs/console-planning-review-2026-10-13`, HEAD revisado
`e45cdb2`. No se implementó nada (ver §9).

## 0. Veredicto

**REJECT (planificación).**

El brief ([38](../38-INDEPENDENT-REVIEW-HANDOFF.md)) exige REJECT cuando un bypass crítico aparece
descrito como contenido. Eso ocurre en dos lugares:

- **IR-01.** El invariante R2 de [33 §1](../33-CROSS-DOMAIN-RECONCILIATION.md) y
  [34 §3.3](../34-OL3-ORCHESTRATION-CONTRACT.md) afirman que ningún componente puede fabricar una
  aprobación de otra capa. La única frontera real es el mismo UID, y DN-08/OL3 ponen agentes nativos
  con poder de cuenta completa en el camino automático.
- **IR-03.** El modo «aprobar el DAG entero» de 34 §1 no tiene un estado bloqueado.

Además siguen vigentes los bloqueadores B-P0-01 a 06 de [36 §2](../36-PLANNING-CLOSURE-AND-VERIFICATION-GATES.md).
Los documentos 32 a 41 son, en general, honestos sobre lo que no está probado. El rechazo no se debe
al volumen faltante, sino a tres contradicciones de diseño concretas (IR-01 a 03) que ningún gate
existente captura.

**Declaración de independencia limitada.** Este revisor escribió buena parte de 05, 12, 13, 15, 16,
17, 18 y 31 en pasadas anteriores de esta misma sesión. Por eso:

- es independiente solo respecto de 32 a 41 y de las revisiones de otro autor;
- no lo es respecto de 16 y 17, que esta revisión contrasta;
- B-P0-06 sigue abierto: hace falta un revisor tercero.

## 1. Hallazgos, por severidad

Evidencia:

- **[C]** confirmado leyendo el texto citado;
- **[H]** hipótesis que requiere un experimento o una decisión;
- **[R]** riesgo residual aceptable solo con aceptación explícita.

### Críticos

#### IR-01 — Las capas de aprobación colapsan dentro del mismo UID [C]

**Afecta:** R2 de 33 §1; 34 §3.3 («Approval for one layer never satisfies a different layer»);
34 §2.1 («authenticated local IPC»); DN-08=A; DN-15=OL3.

**Evidencia:**

- La autoridad solo verifica el UID del par: ADR 0011 (línea 14) y ADR 0020 (línea 289), que dice
  «A process of the same user could already send these requests».
- 10 §1 (líneas 44 y 45) excluye expresamente a otro proceso del mismo usuario.
- 16 §5 (línea 701) llama al UID «the only hard boundary».
- 34 no define cómo se autentica `native_confirmation` (§4) más allá de esa misma frontera.

**Contraejemplo:** un pipeline OL3 ejecuta el paso 1 con un agente nativo (cuenta completa).

1. El agente lee contenido hostil del repositorio y queda bajo inyección de prompt.
2. Abre el socket de `wyss-control` y llama a `approvals.respond` sobre la aprobación pendiente de
   otra sesión gestionada concurrente (DN-08).
3. O abre el socket de `wyss-orchestrator` y llama a `grant_step` o a `submit` con una
   `native_confirmation` fabricada.

Ninguna regla de 34 lo impide. R2 dice «may not», pero no existe un mecanismo que lo haga cumplir.

**Corrección mínima:**

1. Reescribir R2 y 34 §3.3 como reglas para componentes honestos, no como frontera.
2. Declarar BLOCKED la ejecución de pasos nativos en OL3 hasta elegir una de estas opciones:
   - (a) los pasos nativos lanzados por OL3 corren sin acceso a `$XDG_RUNTIME_DIR` de los sockets
     de Wyss (namespace o UID distinto);
   - (b) OL3 auto-avanza solo pasos gestionados o de solo propuesta.
3. Hacer de `native_confirmation` algo que un proceso del mismo UID no pueda emitir, o documentar
   que sí puede.

**Validación:** prueba TS-ORC nueva en la que un paso nativo intenta conectarse a los tres sockets.
Se espera rechazo bajo (a), o, si se elige (b), una aceptación de riesgo documentada.

#### IR-02 — Shell remota sin nadie presente, y una revocación que no restaura integridad [C]

**Afecta:** DN-44=B y DN-46=B; [35 §7](../35-HOST-REMOTE-AND-MOBILE-DELTA.md), fila «Host logged
out: continue reading/typing»; 16 §5 (línea 692: LP4 con concesión de 7 días); 35 §7, fila «Lost
phone».

**Qué cubre ya B-P0-02:** que LP4 equivale a la cuenta completa.

**Qué no cubre:**

1. Con lingering, LP4 opera durante días sin nadie en el host.
2. El registro de concesiones del gateway vive bajo el mismo UID, así que una sesión LP4 puede
   reescribirlo.
3. Revocar después de cualquier uso de LP4 no elimina la persistencia que el atacante haya
   instalado.

**Contraejemplo:**

1. Roban el teléfono con una concesión LP4 vigente; el propietario cerró sesión en KDE.
2. El atacante agrega una unidad `systemd --user`, una clave SSH, o se re-concede el dispositivo en
   el almacén del gateway.
3. El propietario revoca el teléfono desde otra sesión autorizada; la revocación de 35 §7 queda sin
   efecto real.

**Corrección mínima:**

1. Cambiar la fila «Host logged out» de 35 §7: `terminal.write` se rechaza sin sesión local, salvo
   una concesión separada «LP4 en ausencia», elegida por el propietario.
2. Acortar el TTL de LP4.
3. Que 16 y 35 digan que, tras usar LP4, recuperarse exige auditar la cuenta y que la revocación
   corta el acceso futuro, no el pasado.

**Validación:** caso TS-HYB nuevo (revocación tras persistencia implantada, en una VM descartable)
más una decisión del propietario.

#### IR-03 — «Aprobar el DAG entero» lanza agentes nativos alimentados por handoffs automáticos [C]

**Afecta:** 34 §1 (línea 25, «approval mode (each start or approval of entire finite bounded
DAG)»), 34 §3.3 punto 2 y 34 §4 `submit_handoff` («automatic data-only handoff inside a reviewed
bounded DAG»), frente a R3 y R4 de 33 §1.

**Contraejemplo:**

1. El paso A (cualquier agente) produce texto con instrucciones.
2. El handoff automático lo entrega como prompt al paso B, que es nativo con cuenta completa.
3. Con aprobación de DAG entero, B arranca sin confirmación por lanzamiento.

Para un agente nativo, el prompt es control, así que «data-only» no limita nada. El texto no dice si
el modo DAG entero satisface la confirmación del punto 2 de §3.3.

**Corrección mínima:**

1. Los pasos con efecto `native/full-account` exigen confirmación nativa por lanzamiento en todos
   los modos.
2. El handoff automático solo puede apuntar a pasos gestionados o de solo propuesta.

**Validación:** TS-ORC nuevo con un DAG A→B (B nativo) en modo DAG entero; B debe quedar en
`waiting approval`.

### Altos

#### IR-04 — DN-08=A y DN-42=C escritos como invariantes de contrato sin ADR [C]

**Afecta:** 34 §4.1 («Independent managed runtime per managed session, as chosen in DN-08»).

**Contradice:**

- ADR 0012 (línea 8): como máximo un App Server compartido.
- ADR 0015 (línea 9): como máximo un turno gestionado en curso, globalmente.
- ADR 0031 §4.6: el cliente no envía nada nuevo.

36 B-P0-03 lo reconoce, pero 34 lo presenta como invariante de IF-ORC v1, y TS-ORC lo da por
supuesto.

**Contraejemplo:** quien implemente OL3 lanza dos pasos gestionados en paralelo. Entonces pasa una
de dos cosas:

- el índice único de turno activo rechaza el segundo turno y el pipeline falla de forma ambigua;
- el implementador relaja el índice sin un ADR.

**Corrección:** marcar BLOCKED (B-P0-03) esa viñeta y todo método de IF-ORC que dependa de la
concurrencia gestionada. Hasta que exista el ADR, OL3 serializa los pasos gestionados.

**Validación:** lectura del ADR futuro, más pruebas de autoridad con dos runtimes en un entorno
descartable.

#### IR-05 — Contener a Hermes no basta: el ejecutor de Wyss Agent compara texto con globs [C en código original / H en diseño final]

**Afecta:** 40 §2, punto 3 («No freeform command strings are considered executable proposals by
themselves»); [17](../17-WYSS-AGENT-AND-HERMES.md) OW3.

**Evidencia:** en el Wyss original, las reglas de denegación del shell se evalúan con `fnmatch`
sobre el texto del objetivo (`packages/wyss-core/src/core/policy/engine.py:81-85`; patrones en
`builtin_deny.py`). Se leyó, no se ejecutó.

**Contraejemplo:** con Hermes perfectamente contenido, el planificador propone el mismo comando
destructivo con los argumentos reordenados, o una carga codificada que se pasa a un intérprete. El
glob no coincide, el nivel sale del tipo de acción, y la persona aprueba un resumen engañoso.

**Corrección:** bajo DN-06=D, una propuesta originada en Hermes solo puede ser una operación tipada
de una lista cerrada. Un shell libre:

- nunca es ejecutable desde Hermes; o
- solo lo es con confirmación nativa por comando, que muestra el texto literal con la etiqueta de
  cuenta completa.

**Validación:** E-HRM-09 nuevo (corpus de ofuscación contra el motor de política), que se suma a
E-HRM-01 a 08.

#### IR-06 — La deduplicación y las épocas no sobreviven a un reinicio del gateway [H]

**Afecta:** 16 §5 (línea 614: ids en memoria durante 24 horas; RT-05 en la línea 760); 35 §6
(`connection_epoch`, época del claim y secuencia).

**Contraejemplo:**

1. El gateway aplica un `managed.turns` o un lote de entrada.
2. Cae antes de enviar el ACK y systemd lo reinicia.
3. El teléfono reconecta y retransmite. La memoria de deduplicación está vacía y la época puede
   reiniciarse desde el mismo valor, así que el efecto ocurre dos veces.

**Corrección:** que la época incluya un `gateway_instance_id` aleatorio por arranque. Las
solicitudes con efecto de una instancia anterior se rechazan con `outcome_unknown` y se muestran
para revisión; no se deduplican ni se reaplican.

**Validación:** experimento aislado (matar el gateway entre el efecto y el ACK) y una prueba
TS-HYB-03 ampliada.

#### IR-07 — Procesos que sobreviven al logout o a la muerte del host: recuperación falsa o duplicada [H]

**Afecta:** 34 §5 («Host dies → LOST + restart proposal»); 35 §4; 16 §4.11 (línea 375); TS-HYB-06;
R8 de 33 §1, que solo cubre una dirección.

**Contraejemplo:**

1. Un agente nativo deja descendientes fuera del PTY (`setsid`, `nohup`, `systemd-run --user`).
   O una terminal dentro de la ventana, el fallback de la línea 375, vive en el scope de la sesión
   gráfica.
2. Con `KillUserProcesses=no`, el valor típico de las distribuciones basadas en Arch (no verificado
   en el equipo del propietario), siguen escribiendo tras el logout o la caída de `wyss-host`.
3. El coordinador marca el paso LOST y propone reiniciarlo, y quedan dos escritores sobre el mismo
   worktree.

**Corrección:**

- cada paso o terminal en su propio cgroup (un scope transitorio);
- LOST solo cuando ese cgroup está vacío;
- reiniciar está bloqueado mientras queden procesos, y el estado muestra «procesos huérfanos vivos».

**Validación:** una VM descartable con la configuración de logind de CachyOS, más la lectura de
`logind.conf` del propietario (solo lectura, con su permiso).

#### IR-08 — Regla de apagado por inactividad de `wyss-host` contradictoria [C]

**Afecta:** 16 §4.1 (línea 211) y su tabla de estados (línea 403: «no terminal, no client for
60 s»), frente a 35 §4 (línea 104: ningún PTY vivo, ningún pipeline en curso, ningún cliente
requerido).

**Contraejemplo:**

1. Un pipeline queda entre pasos (el paso 1 terminó, el 2 espera aprobación). No hay terminal ni
   cliente.
2. Según 16, el host sale a los 60 s.
3. Con lingering y nadie presente, el lanzamiento del paso 2 depende de la socket activation. Tras
   3 caídas en 60 s, el servicio queda caído (línea 375).

**Corrección:**

- reemplazar la regla de 16 por la de 35;
- definir «cliente requerido» como una concesión de keep-alive que emite `wyss-orchestrator`;
- definir el estado del pipeline cuando el host agota sus reinicios.

**Validación:** lectura más una prueba de la máquina de estados.

#### IR-09 — El parser del emulador comparte destino con todos los PTY [C, aunque la decisión está abierta]

**Afecta:** 35 §2 (líneas 40 a 44: el aislamiento es una «ADR question»); 35 §5 (líneas 140 y 141:
no hay un tope duro de memoria posible); 16 §4.11 (una caída cierra todas las terminales).

**Contraejemplo:** una salida hostil (escapes OSC o DCS, o una tormenta de redimensionados) agota
la memoria de `wyss-host`. Todos los PTY reciben SIGHUP y todos los pasos nativos de OL3 quedan
LOST.

**Corrección:** fijar como requisito, no como opción, que el dueño del PTY master es un proceso
distinto del parser. El parser corre por terminal, se puede reiniciar y tiene límites de memoria.
Si el parser cae, se degrada la restauración de pantalla; la terminal no muere.

**Validación:** TS-HYB-08 ampliado con una aserción de aislamiento de caídas (fuzzing más inyección
de OOM en un entorno descartable).

### Medios

#### IR-10 — La «presencia» del dispositivo Android no es verificable por el host [H]

**Afecta:** 35 §6 (un desafío de presencia por cada claim) y 35 §7 (sobre Keystore y su fallback).

**Contraejemplo:** en un dispositivo sin clave atada a autenticación de usuario, la firma solo
prueba posesión. LP4 queda protegido solo por la posesión del teléfono.

**Corrección:**

- registrar en la concesión las propiedades atestiguadas de la clave (`userAuthenticationRequired`,
  y StrongBox o TEE);
- negar LP4 sin una atestación válida, salvo que el propietario elija otra cosa.

**Validación:** un dispositivo real. Un emulador solo produce atestación por software, así que no
prueba nada.

#### IR-11 — La revocación de un dispositivo no frena los pipelines que aprobó [C]

**Afecta:** 34 §5 («existing pipeline effects retain the original host-level authorisation but may
be paused under configured revocation policy»). La política no está definida.

**Corrección:** por defecto, pausar los pasos cuya última aprobación o step-up vino del dispositivo
revocado.

**Validación:** TS-ORC nuevo.

#### IR-12 — Ruta de escritura y bóveda siguen bloqueadas; falta el estado «bóveda no disponible» [C/H]

**Afecta:**

- B-P0-01 (L1, L2, L5 y DN-31), reconocido en 36 y en 41 §3;
- TS-CFG-19 de 37 §3 y DN-45 (`config.request`), que presuponen un protocolo de transacción que no
  existe.

**Riesgo nuevo:** con lingering y sin sesión, el keyring de la sesión está bloqueado. No hay un
estado definido para las operaciones de bóveda en esa situación.

**Riesgo de LP4:** una sesión LP4, con el keyring desbloqueado, puede leer la clave (36 B-P0-02
también lo menciona).

**Corrección:**

- marcar TS-CFG-19 y `config.request` como BLOCKED(B-P0-01);
- agregar el estado `vault_locked_no_session` con un rechazo visible.

**Validación:** experimentos sobre el sistema de archivos real del propietario (TV-29) y sobre el
keyring en una VM.

#### IR-13 — TS-CFG-17 a 20 cambiaron de significado [C]

**Evidencia:**

- 20 §2 (línea 87) los define como autoría: validar, avisar si se puede deshacer, deshacer solo
  desde la bóveda, y nada tecleado se vuelve comando.
- 37 §3 (línea 182) y 20 (línea 679) los redefinen como pruebas negativas.

Las aserciones originales sobre deshacer quedan sin ninguna prueba.

**Corrección:** renumerar los negativos de 37 (por ejemplo TS-CFG-21 a 24) y conservar 17 a 20.

**Validación:** lectura.

### Bajos

#### IR-14 — Colisiones de identificadores [C]

- **MS-01 a 10:** 16 §8 (líneas 777 a 781; MS-04 es «Session detail» y MS-05 es «Terminal») frente
  a 37 (líneas 204 a 222; MS-04 es «Terminal» y MS-05 es «Pipeline»).
- **Conflictos:** C01 a C16 de 33 §2 frente a C1 a C18 de 31 §5.

**Corrección:** prefijos distintos, por ejemplo MA-xx en 37 y RC-xx en 33.

**Validación:** lectura.

#### IR-15 — 41 se limita a sí mismo correctamente, pero no puede contar como gate [C]

41 §1 no verificó los enlaces de 41, de los ADR ni de las revisiones. La verificación de §3 se hizo
en un contenedor, con otros sistemas de archivos. 41 lo dice.

**Corrección:** que 36 G4 cite 41 §3 solo como «control negativo de EXDEV», nunca como evidencia
para DN-31.

**Validación:** lectura.

## 2. Bloqueadores

**Nuevos de esta revisión:** IR-01, IR-02 e IR-03 (de diseño, sin un estado bloqueado).

**Ya registrados en 36 y todavía vigentes:**

| Bloqueador | Asunto                                             | Hallazgos de esta revisión que lo amplían |
| ---------- | -------------------------------------------------- | ----------------------------------------- |
| B-P0-01    | Escritores concurrentes, snapshots, replicación    | IR-12                                     |
| B-P0-02    | LP4 frente a la configuración acotada              | IR-02 e IR-10                             |
| B-P0-03    | Concurrencia de la autoridad y selección de modelo | IR-04                                     |
| B-P0-04    | Herramientas con efecto de Wyss Agent              | IR-05                                     |
| B-P0-05    | Gateway, parser y secretos locales                 | IR-06 e IR-09                             |
| B-P0-06    | Veredictos de revisión                             | Revisor tercero pendiente                 |

## 3. Qué se resuelve leyendo y qué exige experimentos aislados

| Hallazgo                          | Se resuelve leyendo o editando documentos               | Exige un experimento aislado posterior (no ejecutado)                                               |
| --------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| IR-01                             | La reescritura de R2 y 34 §3.3, y la elección (a) o (b) | Bajo (a): un paso nativo en un namespace sin sockets de Wyss, en una VM descartable                 |
| IR-02                             | La fila de 35 §7, el TTL y el texto de revocación       | Persistencia tras la revocación, en una VM descartable                                              |
| IR-03                             | La regla de confirmación por lanzamiento                | —                                                                                                   |
| IR-04                             | Marcar BLOCKED y serializar                             | Dos runtimes y la autoridad, en un entorno descartable                                              |
| IR-05                             | La regla de operaciones tipadas                         | E-HRM-09: corpus de ofuscación contra el motor (sin el host del propietario)                        |
| IR-06                             | La especificación del id de instancia                   | Matar el gateway entre el efecto y el ACK                                                           |
| IR-07                             | El diseño por cgroup                                    | Logout y caída con la configuración de logind de CachyOS, en una VM                                 |
| IR-08, IR-11, IR-13, IR-14, IR-15 | Sí, por completo                                        | —                                                                                                   |
| IR-09                             | Fijar el requisito                                      | Fuzzing e inyección de OOM del parser fuera de proceso                                              |
| IR-10                             | La regla de atestación                                  | Un dispositivo Android real con y sin StrongBox                                                     |
| IR-12                             | Marcar BLOCKED y el estado nuevo                        | TV-29 en el sistema de archivos real (requiere autorización explícita); keyring bloqueado en una VM |

Ninguno de estos experimentos fue ejecutado. Ninguno debe correr en el equipo ni en el teléfono del
propietario sin un objetivo nombrado y un acuerdo sobre los efectos secundarios (39 §4, 40 §3).

## 4. Las 29 filas de 31 §2, reevaluadas

**Etiquetas**, en este orden: cobertura de requisitos, profundidad de diseño, completitud de
decisiones, trazabilidad, especificación de verificación.

**Abreviaturas:** C = COMPLETE, P = PARTIAL, B = BLOCKED, ND = NOT DESIGNED.

Las filas marcadas «=31» no se reevaluaron en esta pasada y conservan el valor de 31 §2. No es un
juicio nuevo.

| #   | Subsistema                                | Etiquetas         | Motivo                                                          |
| --- | ----------------------------------------- | ----------------- | --------------------------------------------------------------- |
| 1   | Terminal tal como está construida         | =31               | —                                                               |
| 2   | Espacio de trabajo de la terminal         | =31               | —                                                               |
| 3   | Lanzador nativo                           | C · P · B · C · P | IR-01 e IR-03: falta confirmación nativa por lanzamiento en OL3 |
| 4   | Registro y supervisión nativos            | C · P · B · C · P | IR-07: LOST sin cgroup                                          |
| 5   | Plano gestionado tal como está construido | =31               | —                                                               |
| 6   | Inferencia gestionada                     | =31               | —                                                               |
| 7   | Trabajo gestionado a escala               | P · P · B · P · P | IR-04                                                           |
| 8   | Adaptadores                               | =31               | —                                                               |
| 9   | Inventario de configuración               | =31               | —                                                               |
| 10  | Protocolo de escritura                    | C · P · B · P · P | B-P0-01                                                         |
| 11  | Interruptores nativos                     | =31               | —                                                               |
| 12  | Bóveda                                    | C · P · B · P · P | IR-12                                                           |
| 13  | Mutaciones de Codex                       | =31               | —                                                               |
| 14  | Reglas, hooks, skills y MCP (autoría)     | C · P · P · P · P | IR-13                                                           |
| 15  | Adaptadores de OpenCode y Hermes          | =31               | —                                                               |
| 16  | Perfiles                                  | =31               | —                                                               |
| 17  | Orquestación y handoff                    | C · P · B · P · P | IR-01, IR-03, IR-08 e IR-11                                     |
| 18  | Modelos                                   | C · P · B · C · P | IR-04 (DN-42)                                                   |
| 19  | Datos y recuperación                      | =31               | —                                                               |
| 20  | Modelo de amenazas                        | C · P · P · C · P | IR-01 e IR-02 no figuran como amenazas                          |
| 21  | Interfaces                                | =31               | —                                                               |
| 22  | Varios equipos                            | =31               | —                                                               |
| 23  | Terminales del host                       | C · P · B · P · P | IR-07, IR-08 e IR-09                                            |
| 24  | Acceso remoto                             | C · P · B · P · P | IR-02, IR-06 e IR-10; DN-05 abierto                             |
| 25  | Wyss Agent y Hermes                       | P · P · B · P · P | IR-05                                                           |
| 26  | Distribución                              | =31               | No se reevaluó FR-DIST-07 en profundidad                        |
| 27  | Rendimiento                               | =31               | —                                                               |
| 28  | Observabilidad                            | =31               | —                                                               |
| 29  | Accesibilidad                             | =31               | —                                                               |

## 5. Decisiones que solo puede tomar el propietario

1. **LP4 en ausencia (IR-02):** si `terminal.write` puede operar sin sesión local, y con qué TTL.
2. **Pasos nativos en OL3 (IR-01 e IR-03):** ¿confinamiento (a), solo pasos gestionados (b), o
   aceptar por escrito que un agente nativo puede aprobar por las demás capas?
3. **Dispositivos Android sin clave atestiguada (IR-10):** ¿negar LP4 o aceptar «solo posesión»?
4. **DN-05:** H1 o H2, después de E-REM-01 a 06.
5. **Shell libre desde Hermes (IR-05):** prohibirlo o exigir confirmación nativa por comando.

## 6. Gates que solo puede cerrar la evidencia

Los cubre la columna derecha de §3, además de:

- G4 de 36 (TV-29, OQ-31, OQ-35, OQ-40, OQ-50, OQ-55 a 57);
- E-REM-01 a 06;
- E-HRM-01 a 08;
- TS-HYB-01 a 14;
- TS-ORC-01 a 15.

No hay evidencia ejecutada para ninguno.

## 7. Fallas de trazabilidad

| Falla                                                     | Hallazgo |
| --------------------------------------------------------- | -------- |
| TS-CFG-17 a 20 redefinidos                                | IR-13    |
| MS-xx y C-xx duplicados                                   | IR-14    |
| Invariante de DN-08 en 34 §4.1 sin ADR                    | IR-04    |
| Regla de inactividad: 16 §4.1 frente a 35 §4              | IR-08    |
| Bypass del mismo UID sin entrada en el modelo de amenazas | IR-01    |

El chequeo de 41 §2 (presencia de identificadores) es correcto, pero no detecta colisiones ni
cambios de significado.

## 8. Documentos y commits revisados

- **Documentos de la rama**, en el HEAD `e45cdb2`:
  - 32 a 41, completos;
  - 16 §4.1, §4.11, §5, §8;
  - 10 §1;
  - 17 OW3 y §6;
  - 31 §2 y §5;
  - 02 FR-DIST-07;
  - 20 §2 y §14;
  - 23 §6.
- **ADR:** 0011, 0012, 0015, 0020 y 0031, en las líneas citadas.
- **Código del Wyss original:** `policy/engine.py` y `policy/builtin_deny.py`, solo lectura, sin
  copiar código.

## 9. Confirmación

- **No se implementó nada.**
- **No se modificó código de aplicación.**
- **Nada real tocado:** no se instaló, ejecutó ni configuró ningún proveedor, servicio, credencial
  ni configuración real.
- **Sin experimentos:** no se hizo ninguno, ni en equipos del propietario ni en contenedores.
- **ADR:** ninguno se aceptó ni cambió de estado.
- **Rama `main` y PR:** no se modificó `main` ni se fusionó ningún PR.
- **Sin coordinación:** no hubo coordinación con otros agentes.
- **Comandos usados:** lectura con `git`, `grep`, `sed`, `find` y `ls`; `prettier` y un chequeo de enlaces en Python solo sobre este archivo.
- **Archivos modificados:** únicamente este informe.
