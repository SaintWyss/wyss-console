# Auditoría de las correcciones posteriores al REJECT (IR-01 a IR-15)

Fecha: 2026-10-14. Alcance: **solo la planificación, sobre el papel**.

- **Rama:** `docs/console-planning-review-2026-10-13`.
- **HEAD auditado:** `906822f` (PR SaintWyss/wyss-console#2).
- **Commits de corrección contrastados:** `7251d1b` a `906822f` (14 commits), contra el REJECT de
  `00b18fa`.
- **Código original de Wyss:** `SaintWyss/wyss@0761dcdacdafa216c8afc25036e24685855d2228`, leído y
  no ejecutado.

## 0. Declaración de autoría: este informe NO es independiente

El historial de la rama muestra que Claude Opus 5.5, el mismo modelo que redacta este informe, es
coautor de:

- `00b18fa`: la revisión REJECT que origina IR-01 a IR-15 (sesión `session_01GRzb9…`).
- `537eabc`, `2faa109`, `50859d3`, `6b6602a`, `9663e16`, `1c400d1`, `69958a0`, `7f30f56` y
  `9a85848`: el conjunto de arquitectura original y 05, 12, 13, 15, 16, 17, 18 y 31.
- `9f1477d`, `b49069c` y `df6b35b`: la propuesta y la aceptación del ADR 0028.

Esta sesión no tiene memoria de aquellas. Aun así, es el mismo modelo, y por lo tanto comparte sus
sesgos y sus puntos ciegos. Los commits de corrección (`7251d1b` a `906822f`) no llevan trailer de
coautoría de IA. Su autor efectivo no se puede determinar desde el repositorio.

En consecuencia:

- Este informe **no satisface** el gate de revisor tercero de 38 §7, 42 §5.3 ni B-P0-06 de 36.
- No debe citarse como «revisión independiente».
- Sirve como auditoría adversarial adicional. El gate de independencia sigue abierto.

## 1. Veredicto

**REJECT (planificación, sobre el papel).**

Las correcciones mejoran la honestidad del texto. Retiran afirmaciones falsas en 33 R2 a R4,
34 §3.3 y §4, 35 §7 y 16 §5.3, y crean estados bloqueados explícitos. Pero 38 §5 exige REJECT por
tres motivos que siguen presentes:

1. **Un bypass crítico sigue descrito como contenido en otros contratos.**
   - TS-ORC-11 (34 §7, línea 293): el runtime nativo «cannot forge» la aprobación gestionada.
   - 16 §5.3 (línea 700): «No remote client can produce one».
   - 34 §2.1 (línea 82): «authenticated local IPC».

   Ninguno se corrigió, y contradicen a 42 C-A01 (EA-01).

2. **Partes requeridas del producto final quedan bloqueadas sin un camino de aceptación
   verificable.** El criterio para desbloquear OL3 nativo es una conjunción que el propio plan hace
   insatisfacible: un agente «unrestricted» que a la vez esté aislado de la autoridad y conserve las
   credenciales del proveedor sin copiarlas (EA-02). LP4 en ausencia y la concurrencia gestionada
   remiten a decisiones o ADR futuros sin criterio de salida.
3. **La prueba de aceptación que cierra IR-01 tiene un alcance que produciría un falso verde.**
   TS-SEC-OL3-01 solo prueba sockets. La autoridad, su unidad systemd, su binario, su SQLite y la
   configuración del proveedor que gobierna el sandbox gestionado son archivos escribibles por el
   mismo UID (EA-03).

Que un documento diga BLOCKED no se acepta como corrección cuando:

- el mismo plan conserva, en otro documento, la garantía que el bloqueo debía retirar; o
- el bloqueo no tiene un criterio de salida comprobable.

## 2. Estado de cada hallazgo IR-01 a IR-15

**Clases:**

- **(a)** mitigado solo en la especificación, con el gate bloqueado correcto;
- **(b)** sigue defectuoso, con contraejemplo;
- **(c)** requiere evidencia técnica, NO EJECUTADA.

Un hallazgo puede estar en más de una clase.

| IR    | Clase                  | Dónde quedó corregido                            | Qué sigue mal (detalle en §3)                                                                                                                                                                                                                        |
| ----- | ---------------------- | ------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| IR-01 | (b)+(c)                | 33 R2 a R4; 34 §1, §3.3, §4; 42 C-A01; 36 §10    | El bloqueo cubre solo el auto-despacho OL3. El mismo bypass existe con agentes nativos manuales y con LP4. TS-ORC-11, 16:700 y 34:82 siguen afirmando la frontera. El criterio de desbloqueo es insatisfacible tal como está escrito (EA-01 a EA-03) |
| IR-02 | (b)+(c)                | 16 §5.3 LP4 y §13; 35 §6, §7, §10; 42 C-A02      | 16 GR2 (línea 602) y 22 DN-44 (líneas 90 y 712) mantienen los 7 días. 42 bloquea por bloqueo de pantalla y el resto solo por logout. La puerta «login local» no es una frontera y anula el motivo declarado de DN-44 (EA-04)                         |
| IR-03 | (a), (b) residual      | 33 R3 y R4; 34 §1, §3.3, `submit_handoff`; 37 §8 | Corregido para el handoff explícito. Queda el canal implícito: el worktree que un paso gestionado puede modificar (hooks y reglas del proveedor, `.git/hooks`) antes de la confirmación nativa (EA-07)                                               |
| IR-04 | (b)+(c)                | 34 §4.1, TS-ORC-04; 36 §10; 42 §2                | 20 §14 (TS-ORC-04, AC-FINAL-06 y la línea 649), 23:258, 02 FR-ORC-06 y FR-MOD-11, y TS-ORC-14 siguen exigiendo la concurrencia. Hay pares de aceptación contradictorios. La serialización bajo ADR 0015 reduce funcionalidad sin decirlo (EA-05)     |
| IR-05 | (b)+(c)                | 17 §14; 40 §5; 42 §2                             | La evidencia no está fijada a un commit y es incompleta. Omite la allowlist vacía del shell. No ve el TOCTOU real del adaptador de archivos ni que la política vive en archivos del mismo UID. El vocabulario tipado no está enumerado (EA-06)       |
| IR-06 | (b)+(c)                | 35 §6; 16 §13; 42 §2                             | 16 §5.2.5 (línea 614) mantiene «never a second effect» con memoria de 24 h. No se define quién acuña la identidad de intent hacia la autoridad, así que un reintento tras reiniciar el gateway puede crear un intent nuevo (EA-08)                   |
| IR-07 | (a), (b) residual, (c) | 34 §5; 35 §4; 16 §4.11 y SM-HOST; 42 §2          | `ORPHANS_POSSIBLE` no tiene transición de salida ni un estado en SM-ORC o en el SM nativo de 05. El escape del cgroup (`systemd-run --user`) no es detectable, así que el estado es permanente o se sale por un clic no verificable (EA-09)          |
| IR-08 | (a)                    | 16 §4.1 y SM-HOST; 35 §4; 42 §2                  | `HOST_UNAVAILABLE` no figura en SM-ORC (34 §3.1). El contrato de keepalive (TTL, renovación, autenticación) no está especificado. Gravedad baja                                                                                                      |
| IR-09 | (a)+(c)                | 35 §2; 42 §2                                     | Falta la política de contrapresión si el parser se cuelga sin caer, y el bucle de caídas por entrada venenosa al reinyectar el anillo. 16 §4.1 (línea 208) sigue diciendo «it moves bytes» frente a DN-47=B (EA-10)                                  |
| IR-10 | (b)+(c)                | 35 §7; 42 C-A02; 16 §5.3                         | No enumera qué campos de la atestación se verifican. No dice quién mantiene la raíz y la revocación. No existe regla de presencia para un PC con LP4. El verificador es del mismo UID (EA-11)                                                        |
| IR-11 | (a)+(c)                | 34 §5; 42 §2; 20 §16                             | Sin defecto nuevo de fondo. El origen «desde el dispositivo» depende del diario del gateway, que es del mismo UID                                                                                                                                    |
| IR-12 | (a), (b) residual, (c) | 42 §2; 36 §10; 37 §8                             | `VAULT_LOCKED_NO_SESSION` no está en 27, 07 §8 ni 13. La copia de 13 para `vault_locked` (línea 958) pide desbloquear cuando no hay sesión que pueda hacerlo                                                                                         |
| IR-13 | (a)                    | 37 §3; 20 §16; 23 §7                             | AC-CFG-AUTH-01 está referenciado en 23:317 pero no definido en 20. TS-CFG-17 a 20 conservan su significado, pero no tienen criterio de fallo (20 §3.4, «—»). Gravedad baja                                                                           |
| IR-14 | (b)                    | 33 §2 (RC); 37 §4 (MA)                           | 23:273 a 291 sigue citando «33 C02, C05, C12, C13», que ya no existen. Dos inventarios móviles divergentes (MS en 16, MA en 37) sin uno normativo. Gravedad baja                                                                                     |
| IR-15 | (a)                    | 36 §10; 20 §16 TS-SEC-EVID-01                    | —                                                                                                                                                                                                                                                    |

**Recuento:**

- mitigados en especificación sin defecto residual de peso: 4 (IR-08, IR-11, IR-13 e IR-15);
- con defecto residual: 11;
- de esos, críticos o altos: 6 (IR-01, IR-02, IR-04, IR-05, IR-06 e IR-07).

## 3. Hallazgos de esta auditoría

**Evidencia:**

- **[C]** confirmado leyendo el texto o el código citado;
- **[H]** hipótesis que necesita un experimento;
- **[D]** depende de una decisión.

### Críticos

#### EA-01 — El bloqueo de IR-01 es más estrecho que la vulnerabilidad [C]

**Afecta:** IR-01; 42 C-A01; 34 §1, §2.1, §3.3, §7 TS-ORC-11; 16 §5.3; 11 §2 y §7; ADR 0011,
0019 y 0020.

**Evidencia:**

- ADR 0011 (líneas 44 y 45): «Same-UID validation is a local access boundary, not protection
  from a compromised same-user process».
- 11 §7.1: «A same-user process can do everything the Console can, without it».
- 42 C-A01 solo detiene el _auto-despacho_ OL3 de pasos nativos.
- El producto lanza agentes nativos de forma manual en sus terminales (05, FR-TERM).
- LP4 da una shell del mismo UID.

**Contraejemplo 1 (sin OL3):**

1. El usuario abre a mano Claude Code en una terminal de la Console.
2. En paralelo corre una sesión gestionada de Codex con una aprobación pendiente.
3. El agente nativo, bajo inyección de prompt, conecta a `control.sock` y envía
   `managedSession/turn/approval/respond` con `accept` (ADR 0023 línea 234).
4. La autoridad lo acepta: el par tiene el mismo UID. El diario lo registra como una respuesta de
   cliente.

Nada en 42 lo bloquea, porque no hubo auto-despacho.

**Contraejemplo 2:** un agente nativo llama a `prepare` y luego a `submit` de `wyss-orchestrator`.
Crea y «aprueba» un pipeline de pasos gestionados. El `RunJournal` registra como decisión del
propietario algo que no lo fue.

**Contradicciones que siguen en el texto:**

| Documento y línea | Texto                                                                                                              |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ |
| 34:293            | TS-ORC-11: «coordinator or native runtime cannot forge it»                                                         |
| 16:700            | «Local native confirmation … **No remote client can produce one**». Lo contradice 16 §13 (línea 881): LP4 sí puede |
| 34:82             | «authenticated local IPC»                                                                                          |
| 20:647            | «native confirmation cannot be supplied by Android». Es cierto para la app, falso para LP4                         |

**Impacto:**

- La aprobación humana del plano gestionado deja de ser una garantía cuando corre cualquier agente
  nativo o cualquier sesión LP4. Ese es el uso normal del producto final.
- La auditoría atribuye decisiones al propietario que no tomó.

**Corrección mínima:**

1. Declarar en 10, 11 y 34 que las aprobaciones gestionadas, la aprobación de DAG y la
   confirmación nativa son **advisory** mientras exista cualquier proceso del mismo UID no
   confiable. Esto incluye agentes nativos manuales y LP4, no solo OL3.
2. Marcar TS-ORC-11 como `BLOCKED_TRUST_BOUNDARY`, igual que TS-ORC-04.
3. Corregir 16:700 y 34:82.
4. Que el `RunJournal` y el diario de la autoridad registren el origen como «cliente del mismo UID
   no autenticado», nunca como «propietario».
5. Agregar a 11 la amenaza «agente nativo responde aprobaciones gestionadas, o crea y aprueba
   pipelines». El 11 no se tocó en las correcciones.

#### EA-02 — El criterio de desbloqueo de OL3 nativo es insatisfacible tal como está escrito [C/D]

**Afecta:** 42 C-A01 (líneas 25 a 35); 34 §1 (líneas 32 a 39); 33 RC-11; 20 TS-FINAL-15
(AC-FINAL-33); ADR 0025 («No root»); ADR 0027.

**Qué pide 42:** un «per-run restricted principal» que:

- (i) conserve la funcionalidad del proveedor;
- (ii) impida que sus credenciales lleguen a la autoridad;
- (iii) separe la IPC.

**Por qué no puede cumplirse:**

- **Con otro UID**, el agente no puede usar el inicio de sesión del proveedor del usuario sin
  copiarlo. RC-11 prohíbe «auth-copy fallbacks» y TS-FINAL-15 exige «no credential copying or
  impersonated sign-in». Además, un servicio o principal distinto que no sea del usuario exige root.
  ADR 0025 lo rechaza («A root service … needs a privilege the product does not need») y ADR 0027
  descarta empaquetado e instalación.
- **Con el mismo UID y un namespace** (por ejemplo bwrap), ocultar los sockets de
  `$XDG_RUNTIME_DIR/wyss-console` no basta. El agente sigue pudiendo escribir en `$HOME`:
  - `~/.config/systemd/user/`, `~/.bashrc` o `~/.config/autostart`, que se ejecutan _fuera_ del
    namespace en el próximo arranque;
  - el bus de sesión y `systemctl --user`, que lanzan procesos fuera del namespace;
  - el keyring por Secret Service y el `ssh-agent`.

  Cerrar todo eso convierte al agente en uno «restringido». Eso es una reducción funcional del
  agente nativo que el propietario no eligió.

**Conclusión:**

- Para un agente nativo **irrestricto** no existe aislamiento posible frente a la autoridad del
  mismo usuario.
- Solo existen dos salidas coherentes:
  - un perfil nativo **restringido** (menos capacidades: lo decide el propietario); o
  - una autoridad de aprobaciones bajo **otro principal** (root o un usuario de sistema, contra los
    ADR 0025 y 0027: hace falta un ADR nuevo y lo decide el propietario).
- 42 §3 afirma que «no need to interrupt the owner to choose safer interim stop conditions». Con
  un bloqueo sin salida, ese «interino» es de hecho la eliminación de OL3 nativo. Es una reducción
  silenciosa del alcance de DN-15.

**Corrección mínima:**

1. Reescribir C-A01 como una disyuntiva explícita entre dos opciones:
   - **(A) Perfil nativo restringido:** el propietario acepta qué capacidades pierde, enumeradas
     (escritura fuera del worktree, bus de sesión, keyring, `ssh-agent`, red).
   - **(B) Árbitro de aprobaciones bajo otro principal:** el árbitro y su almacén no son
     escribibles por el usuario, la presencia es humana (hardware o autenticación del sistema) y
     hace falta un ADR que reemplace las partes pertinentes de 0011, 0025 y 0027. Con (B), los
     agentes nativos siguen irrestrictos, y el producto solo garantiza la autenticidad de _sus
     decisiones_, no la contención de los efectos nativos.
2. Escribir para cada opción un criterio de salida comprobable: qué rutas, buses y sockets deben
   ser inaccesibles y qué capacidades deben seguir funcionando.
3. Elevarlo como decisión del propietario (ver §6).

#### EA-03 — TS-SEC-OL3-01 daría un falso verde: la superficie no son solo los sockets [C]

**Afecta:** 42 §4 TS-SEC-OL3-01; 20 §16; C-A01 («every exposed host/gateway socket»).

**Evidencia:**

- ADR 0025 (líneas 57 a 79): unidad `systemd --user` bajo la configuración del usuario; binario del
  usuario; «Codex: found in the authority's own PATH».
- ADR 0011: el estado está en `$XDG_STATE_HOME/wyss-console/control.sqlite3`.
- ADR 0018, citado en 10 §2 (línea 46): «The provider's binary is trusted to enforce its own
  sandbox».

**Contraejemplo:** un paso nativo en un namespace que oculta los tres sockets pasa TS-SEC-OL3-01.
Pero el mismo paso puede:

- anteponer un `codex` falso en el `PATH` del servicio;
- reescribir `ExecStart` en la unidad;
- editar `control.sqlite3` mientras la autoridad no corre;
- o cambiar `~/.codex/config.toml`, que gobierna el sandbox del runtime gestionado.

Todo es del mismo UID.

**Corrección mínima:** TS-SEC-OL3-01 debe enumerar y probar, además de los sockets:

- la unidad, el binario, el `PATH` y el estado de la autoridad;
- la configuración del proveedor gestionado;
- el bus de sesión y `systemctl --user`;
- el keyring;
- y la persistencia diferida (archivos rc y autostart).

Su resultado esperado depende de EA-02: con (A), denegación; con (B), integridad de las decisiones
aunque esos archivos cambien.

### Altos

#### EA-04 — LP4: las correcciones contradicen DN-44, se contradicen entre sí y la puerta no es una frontera [C/D]

**Afecta:** 42 C-A02 (líneas 62 a 70); 16 §5.2 GR2 (línea 602) y LP4 (línea 692); 22 DN-44
(líneas 90 y 712); 35 §7 (líneas 210 y 214); 36 §10; 20 TS-SEC-REM-01.

**Contradicciones confirmadas:**

| Documento y línea                        | Qué dice                                                                                                              |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| 16:602 (GR2)                             | `terminal.write` y `managed.turns`: 7 días                                                                            |
| 16:692 y 16 §13                          | «no default seven-day terminal input grant»                                                                           |
| 22:90, 22:712 y 22:713                   | DN-44 «with a 7-day grant»; los motivos (línea 713) dicen «Giving objectives to native agents **from away** needs it» |
| 42:68                                    | La concesión de escritura vence «after **lock**/logout»                                                               |
| 16:692, 35:214, 36 §10, 20 TS-SEC-REM-01 | Solo logout, o «verified local login present»                                                                         |

**Por qué la puerta no protege:**

- «Hay un login local» mide el estado de logind, no la presencia del propietario.
- La comprueba un gateway del mismo UID. Una primera sesión LP4 puede reescribir su binario, su
  unidad o su almacén de concesiones. 42 admite que el almacén es manipulable.
- Contra un LP4 comprometido, la puerta no aporta nada. Solo limita el uso honesto.

**Por qué reduce el alcance:**

- El caso de uso que 22 cita para DN-44 es controlar agentes **desde fuera**, con la pantalla del
  host bloqueada.
- Con la regla de 42 (bloqueo de pantalla), LP4 queda inutilizable para ese caso por defecto.
- 32 §7 dice que los defaults no reducen el alcance de forma permanente. Pero no hay criterio para
  pasar a «LP4-away»: es «OPEN» y «another reviewed policy».

**Qué sí protege** (es correcto mantenerlo):

- la atestación y la autenticación por uso de la clave del dispositivo;
- una concesión corta por claim;
- el aviso de cuenta completa;
- y la honestidad sobre la revocación (TS-SEC-REM-02).

**Corrección mínima:**

1. Unificar en un solo lugar la regla (logout, o logout más bloqueo) y el TTL de concesión y de
   claim, y propagarla a 16 GR2, SM-GRANT y 22 DN-44.
2. Declarar que la puerta de login es una política para un gateway honesto, no un control frente a
   un compromiso.
3. Llevar a decisión del propietario si LP4 con la pantalla bloqueada es el caso principal (ver §6).

#### EA-05 — La concurrencia gestionada sigue exigida en 02, 20 y 23, con pares de aceptación contradictorios [C]

**Afecta:** IR-04; 02 FR-ORC-06 y FR-MOD-11; 20 §14 (TS-ORC-04 y AC-FINAL-06, línea 599) y §14.1
(línea 649, «two parallel managed sessions»); 23:258; TS-ORC-14 (34 línea 299, no registrado en
20 ni 23); 20 §16 TS-SEC-MAN-01.

**Contraejemplo:** para un mismo binario, AC-FINAL-06 exige que dos sesiones gestionadas
concurrentes no compartan identidades. TS-SEC-MAN-01 exige que la segunda se serialice o se
rechace. El primero no puede pasar si el segundo pasa. Nada en 20 ni en 23 marca cuál está
bloqueado.

**Reducción silenciosa:**

- ADR 0015 fija como máximo un turno gestionado activo _globalmente_.
- Si OL3 serializa sus pasos gestionados, un pipeline en curso impide al propietario usar
  cualquier sesión gestionada interactiva.
- Ningún documento lo dice, ni lo muestra la UX de 13.

**Corrección mínima:**

1. Marcar BLOCKED(B-P0-03) en FR-ORC-06, FR-MOD-11, TS-ORC-04, TS-ORC-14, AC-FINAL-06, AC-FINAL-21
   y 23:258.
2. Declarar la exclusión mutua entre pipelines y sesiones gestionadas interactivas como un estado
   visible.

#### EA-06 — Motor de políticas de Wyss: la evidencia de 40 §5 es incompleta y no apunta al riesgo real [C en código / H en explotación]

**Afecta:** IR-05; 40 §5; 17 §14; 42 §2 (gate «Pinned policy code review»).

**Lectura de `SaintWyss/wyss@0761dcd`** (no ejecutado):

1. **La evidencia no está fijada a un commit.** 40 §5 cita líneas de un repositorio privado sin
   commit. 42 exige una revisión de código fijada a una revisión.
2. **40 §5 omite el control efectivo del shell.**
   - `src/execution/shell.py:57-62` y `:103`: una allowlist de binarios fail-closed.
   - `src/bootstrap/cli.py:508`: el cableado de producción construye `ShellAdapter(audit, guard)`
     sin allowlist. Por lo tanto **ningún `shell.run` se ejecuta** en ese commit.
   - `shell.py:189`: exec por argv, sin shell.

   El contraejemplo de IR-05 (comando reordenado aprobado) no es alcanzable con ese cableado. Sí lo
   sería con cualquier binario que se agregue a la allowlist, porque el segundo muro
   (`guard.py:31-39`) reevalúa con el mismo motor de globs (`engine.py:81-90` y `:129-140`).
   Ejemplos:
   - `env` o `find -exec` evaden `shell-privilege-escalation`, que solo mira el objetivo;
   - `|  bash` con dos espacios, `| zsh` o `bash -c "$(curl …)"` evaden `shell-fetch-pipe-exec`;
   - `rm -r -f` o `find -delete` evaden `shell-recursive-delete`.

3. **No se detectó el riesgo real de las operaciones tipadas.**
   - `src/execution/filesystem.py:136-182` resuelve la ruta y la evalúa.
   - Luego `:292-296` escribe con `mkdir(parents=True)` y `write_text`, que **siguen enlaces
     simbólicos en el momento de escribir**.

   Un proceso concurrente del mismo UID (un agente nativo) puede cambiar un componente por un
   enlace entre el chequeo y la escritura. Es un TOCTOU [H en explotación]. El riesgo no se limita
   al shell: afecta al «closed typed vocabulary» que 42 propone como remedio.

4. **La política vive en archivos del mismo UID.**
   - `wyss-self-modify` (`builtin_deny.py:27-32`) solo cubre acciones `file.*` emitidas _a través
     de Wyss_.
   - Un agente nativo concurrente puede editar el código, la configuración TOML o `audit.db` de
     Wyss directamente.
   - Es la misma clase de fallo que IR-01, y 17 §14 no la menciona.
5. **El vocabulario tipado no está enumerado.** «Typed allowlisted effects still pass full Wyss
   policy» (TS-SEC-AGT-01) no se puede verificar sin una lista cerrada con sus parámetros.

**Corrección mínima:**

1. Fijar 40 §5 a `0761dcd`, o al commit que se adopte.
2. Describir la allowlist y su estado vacío.
3. Enumerar el vocabulario tipado y sus invariantes de ruta: abrir por descriptor con
   `O_NOFOLLOW` o `openat2(RESOLVE_NO_SYMLINKS)`, que es justamente CTL-09 de 11 T-07.
4. Ampliar E-HRM-09 con TOCTOU de rutas y con la auto-modificación de la política por un proceso
   concurrente.
5. Declarar que, bajo DN-06=D, la política de Wyss es de nuevo un control para componentes
   honestos, no una frontera frente a agentes nativos.

#### EA-07 — El worktree es un canal de handoff que la confirmación nativa no revisa [C/H]

**Afecta:** IR-03; 34 §3.3 y `submit_handoff`; 42 C-A03 («observed source context»); TS-ORC-08.

**Contraejemplo:**

1. En un DAG A (gestionado) → B (nativo), el paso A puede escribir en el worktree dentro de su
   sandbox. Escribe `.claude/settings.json` con un hook, `CLAUDE.md`, `AGENTS.md`, `opencode.json`
   o `.git/hooks/*`.
2. La confirmación por lanzamiento de B revisa «the actual handoff input», es decir, los bytes del
   `HandoffEnvelope`.
3. Al arrancar, B ejecuta los hooks de proyecto o sigue las reglas modificadas. El handoff
   «data-only» hacia un paso gestionado se volvió control sobre el nativo, por el sistema de
   archivos.

TS-ORC-08 cubre hooks globales o una versión de proveedor cambiada, no la configuración residente
en el worktree.

**Corrección mínima:** la confirmación nativa por lanzamiento debe mostrar y fijar el diff desde la
aprobación del DAG de toda configuración ejecutable residente en el worktree:

- hooks, reglas y servidores MCP del proveedor de destino;
- `.git/hooks` y `core.hooksPath`.

Un cambio posterior invalida la confirmación. Hace falta un caso TS-ORC nuevo.

#### EA-08 — Idempotencia remota: contradicción en 16 e identidad de intent indefinida [C]

**Afecta:** IR-06; 16 §5.2.5 (línea 614); 35 §6 (líneas 182 a 188); ADR 0011 (intent receipts).

**Evidencia:**

- 16:614 sigue diciendo que el gateway guarda ids 24 h y responde «never a second effect». La
  memoria se pierde al reiniciar, que es justamente IR-06.
- 35 delega en los «durable intent receipts» de la autoridad. ADR 0011 los tiene (líneas 21 a 24),
  pero deduplican por la _identidad de intent que llega a la autoridad_.
- Ningún documento dice si esa identidad la acuña el dispositivo, de forma estable entre
  reintentos, o el gateway, de forma nueva en cada solicitud.

**Contraejemplo:**

1. El teléfono envía `managed.turns` con `request_id` R.
2. El gateway acuña un intent I1, la autoridad lo aplica y el gateway cae antes del ACK.
3. Tras reiniciar, el teléfono reintenta R. La memoria está vacía, así que el gateway acuña I2.
4. La autoridad ve un intent nuevo: dos turnos.

**Corrección mínima:**

1. La identidad del intent hacia la autoridad debe ser determinista desde (dispositivo,
   `request_id`), o la debe acuñar el dispositivo, que la firma en el step-up.
2. Corregir 16:614.
3. Ampliar TS-SEC-REM-03 con `managed.turns` y `approvals.respond`, no solo entrada de terminal.

#### EA-09 — Los estados nuevos son etiquetas sin máquina de estados [C]

**Afecta:** IR-07, IR-08, IR-12; 34 §3.1 SM-ORC; 05 SM nativo (líneas 203 a 223); 27; 07 §8;
13 (líneas 958 y 1040).

**Evidencia:** `ORPHANS_POSSIBLE`, `HOST_UNAVAILABLE` (como estado de pipeline),
`VAULT_LOCKED_NO_SESSION`, `OUTCOME_UNKNOWN`, `AWAITING_NATIVE_REVIEW` y `BLOCKED_TRUST_BOUNDARY`
aparecen solo en 16, 20, 34 a 37 y 42. No figuran en ninguna tabla de transiciones.

**Consecuencias:**

- `ORPHANS_POSSIBLE` no tiene salida. Como el escape del cgroup no es detectable, o el paso queda
  bloqueado para siempre, o se sale por un «revisé» del propietario que nadie especificó.
- La copia de 13 para `vault_locked` pide desbloquear cuando no hay sesión.

**Corrección mínima:** integrar cada estado en su SM, con guardas, salida, texto de UX y prueba. La
salida de `ORPHANS_POSSIBLE` es una acusación de recibo explícita del propietario, registrada como
tal, nunca una inferencia.

### Medios

#### EA-10 — Parser fuera de proceso: faltan contrapresión y bucle de caídas [C/H]

35 §2 fija bien la separación. Pero no especifica:

- **Parser colgado:** qué hace el dueño del PTY si el parser se cuelga sin caer. Debe seguir
  drenando el PTY y descartar hacia el parser con un tope; si no, los agentes se bloquean al
  escribir.
- **Reinicio con entrada venenosa:** cómo se reinicia el parser. Reinyectar el anillo de bytes
  puede repetir la entrada que lo tumbó. Hace falta un modo degradado de byte-replay tras N caídas.
- **Límites de memoria por helper:** cómo se aplican. La delegación del controlador de memoria de
  cgroup v2 al `systemd --user` en CachyOS no está verificada [H].

Además, 16:208 («it moves bytes») contradice DN-47=B.

#### EA-11 — Atestación Android: contrato no verificable; LP4 en PC sin regla [C/H]

35 §7 y 42 exigen «verified per-use authenticated key properties», pero no enumeran:

- `attestationSecurityLevel` y `keymasterSecurityLevel`;
- `userAuthType` y `authTimeout`;
- `rootOfTrust` y `verifiedBootState`;
- la cadena hasta la raíz de Google y su lista de revocación, que requiere red [H];
- quién las verifica (un gateway del mismo UID, cuyo almacén de raíces es manipulable).

DN-44 incluye un «second PC», y para un PC con LP4 no hay ninguna regla de presencia ni de
atestación.

#### EA-12 — Propagación incompleta de identificadores [C]

- 23:273 a 291 cita «33 C02, C05, C12, C13», que no existen tras renombrar a RC.
- 23:317 cita AC-CFG-AUTH-01, que no está definido en 20.
- 16 MS-01 a 10 y 37 MA-01 a 10 son dos inventarios distintos sin uno normativo para TS-REM-18 y
  AC-F10b.

## 4. Revisión focalizada pedida por el propietario

| Tema                                        | Dictamen                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Privilegios del mismo UID                   | Reconocidos en 33, 34, 35, 16 §13 y 42, pero no en 11, TS-ORC-11, 16:700 ni 34:82. La superficie real incluye archivos, unidades, binarios, el `PATH` y la configuración del proveedor, no solo sockets (EA-01 y EA-03)                                                                                                                                          |
| Aislamiento de agentes nativos irrestrictos | **Imposible** frente a la autoridad del mismo usuario. El plan no lo dice: presenta el desbloqueo como una elección técnica pendiente (EA-02). Solo existen un nativo restringido o un árbitro bajo otro principal; ambas son decisiones del propietario y la segunda exige un ADR contra 0025 y 0027                                                            |
| Aprobaciones OL3                            | Corregido: la aprobación de DAG ya no equivale a lanzamiento nativo. Pero la aprobación de DAG, la confirmación nativa y las aprobaciones gestionadas son igual de falsificables desde el mismo UID, y la auditoría las atribuye al propietario (EA-01). Falta el worktree como canal (EA-07)                                                                    |
| Concurrencia gestionada                     | Bloqueada en 34 y 36, todavía exigida en 02, 20 y 23. La serialización bajo ADR 0015 bloquea el uso interactivo durante los pipelines, sin decirlo (EA-05)                                                                                                                                                                                                       |
| LP4                                         | La honestidad sobre la revocación está bien. El default «solo con login» es una política, no una frontera, contradice DN-44 («from away») y se define distinto en 42 que en 16, 35 y 36. Los 7 días siguen en 16 GR2 y 22 (EA-04)                                                                                                                                |
| Persistencia remota                         | La revocación ya no promete restaurar la integridad (C-A02; TS-SEC-REM-02). No hay contrato de recuperación verificable («inspect persistence…» no es un procedimiento). La persistencia de trabajo con lingering y la escritura LP4 al desloguear quedan contradictorias por EA-04. La deduplicación tras reiniciar sigue rota para intents gestionados (EA-08) |
| Seguridad del motor de políticas de Wyss    | La evidencia de 40 §5 no está fijada y es incompleta. El riesgo real es el TOCTOU de rutas y la auto-modificación por un proceso del mismo UID, no el glob de un shell que en producción no ejecuta nada (EA-06)                                                                                                                                                 |

## 5. Contradicciones nuevas o persistentes introducidas por las correcciones

1. 42 C-A02 (bloqueo de pantalla y logout) frente a 16, 35, 36 y 20 (solo logout).
2. 16 internamente:
   - GR2 (7 días) frente a la fila LP4 y §13;
   - la línea 700 frente a §13 (línea 881);
   - la línea 614 frente a 35 §6.
3. 20 §14 TS-ORC-04 y AC-FINAL-06 frente a 20 §16 TS-SEC-MAN-01. Y 34 TS-ORC-11 frente a 20 §16
   TS-SEC-OL3-01.
4. 42 §3 («no need to interrupt the owner») frente a 32 §7 y 38 §7. Los defaults «interinos» sin
   criterio de salida equivalen a reducir el alcance de DN-15 y DN-44.
5. C-A01 opción (a) frente a RC-11, TS-FINAL-15 y ADR 0025 (EA-02).
6. 23 frente a 33 por los identificadores C renombrados (EA-12).

## 6. Decisiones que realmente necesita el propietario

Solo las que la evidencia no puede resolver:

1. **Modelo de confianza para OL3 nativo (EA-02).**
   - **(A) Perfil nativo restringido:** acepta una lista explícita de capacidades perdidas.
   - **(B) Árbitro de aprobaciones bajo otro principal:** requiere root o un usuario de sistema y un
     ADR que reemplace partes de 0011, 0025 y 0027. Los nativos siguen irrestrictos y solo se
     garantiza la autenticidad de las decisiones.
   - **(C) Sin OL3 nativo automático:** OL3 solo encadena pasos gestionados o de propuesta, y esto
     se registra como una reducción aceptada de DN-15.
2. **Valor de las aprobaciones mientras corren agentes nativos o LP4 (EA-01).** Aceptar por
   escrito que son advisory, o exigir (B).
3. **LP4 con la pantalla bloqueada o el usuario deslogueado (EA-04).** Si el caso «desde fuera» de
   DN-44 es el principal y, en ese caso, qué riesgo residual se acepta (TTL, dispositivos,
   atestación), sabiendo que la puerta la aplica un gateway del mismo UID.
4. **Exclusión mutua entre pipelines y sesiones gestionadas interactivas** mientras rija ADR 0015
   (EA-05). Aceptarla como estado visible o priorizar el ADR de concurrencia.
5. **Android sin atestación de hardware y PC con LP4 (EA-11).** Negarlo o aceptar «solo posesión».
6. **DN-05 y DN-31:** siguen abiertos, sin cambios.

## 7. Gates que solo cierra la evidencia (NO EJECUTADOS en esta auditoría)

- **TS-SEC-OL3-01**, ampliado según EA-03. TS-SEC-OL3-02, más el caso del worktree de EA-07.
- **TS-SEC-REM-01 a 03**, ampliado a intents gestionados según EA-08.
- **TS-SEC-HOST-01 a 03**, más el escape por `systemd-run --user` y la contrapresión del parser
  (EA-10).
- **TS-SEC-MOB-01:** en un dispositivo físico, con los campos enumerados (EA-11).
- **E-HRM-01 a 09**, más el TOCTOU de rutas y la auto-modificación de la política (EA-06).
- **TS-SEC-MAN-01 y el ADR de concurrencia.**
- **TV-29 y DN-31** en el sistema de archivos real, con autorización explícita.
- **Revisor tercero independiente** (B-P0-06): sigue abierto. Este informe no lo cierra (§0).

## 8. Filas de 31 §2 afectadas

**Etiquetas**, en este orden: cobertura, profundidad, decisiones, trazabilidad, verificación.

| #   | Subsistema                        | Etiquetas         | Bloqueador restante                   |
| --- | --------------------------------- | ----------------- | ------------------------------------- |
| 3   | Lanzador nativo                   | C · P · B · P · P | EA-01, EA-02, EA-07                   |
| 4   | Registro y supervisión nativos    | C · P · B · P · P | EA-09 (`ORPHANS_POSSIBLE` sin salida) |
| 7   | Trabajo gestionado a escala       | P · P · B · P · P | EA-05                                 |
| 10  | Protocolo de escritura            | C · P · B · P · P | B-P0-01 sin cambios                   |
| 12  | Bóveda                            | C · P · B · P · P | EA-09 (`VAULT_LOCKED_NO_SESSION`)     |
| 14  | Autoría de reglas, hooks y skills | C · P · P · P · P | EA-12                                 |
| 17  | Orquestación y handoff            | C · P · B · P · P | EA-01, EA-02, EA-07, EA-09            |
| 18  | Modelos                           | C · P · B · P · P | EA-05 (FR-MOD-11)                     |
| 20  | Modelo de amenazas                | C · P · P · P · P | EA-01: el 11 no se actualizó          |
| 23  | Terminales del host               | C · P · B · P · P | EA-09, EA-10                          |
| 24  | Acceso remoto                     | C · P · B · P · P | EA-04, EA-08, EA-11; DN-05            |
| 25  | Wyss Agent y Hermes               | P · P · B · P · P | EA-06                                 |

Las 17 filas restantes no se reevaluaron. Conservan el valor de la revisión de `00b18fa` §4 o de
31 §2. No es un juicio nuevo.

## 9. Documentos, commits y código efectivamente revisados

- **Rama, en `906822f`:**
  - completos: 42, 38, 34, el informe de `00b18fa` y 40;
  - el diff completo `00b18fa..906822f` de 00, 16, 17, 20, 23, 32, 33, 34, 35, 36, 37, 38, 40 y 42;
  - por secciones: 16 §4.1, §4.11, §5.2 a §5.4 y §13; 20 §3.4, §14 y §16; 22 DN-44; 23 §6 y §7;
    10 §2; 11 §2 y §7; 05 SM nativo; 13, líneas 958 y 1040.
- **ADR:** 0011 (completo hasta las líneas 60), 0012:8, 0015:9-10, 0019:222-232, 0020:286-300,
  0023:234, 0025:55-85, 0027:1-30; el estado de 0029, 0030 y 0031 (Proposed).
- **Código original de Wyss, en `0761dcd`** (clonado en solo lectura en `/home/user/wyss`, fuera de
  este repositorio):
  - `packages/wyss-core/src/core/policy/engine.py` y `builtin_deny.py`;
  - `src/execution/shell.py`, `guard.py`, `filesystem.py` y `git.py`;
  - `src/bootstrap/cli.py:495-515` y `src/bootstrap/config.py:55-82`;
  - `src/agents/configured_agent.py:100-130`.

## 10. Confirmación

- **No se implementó nada.** No se modificó código de aplicación ni ningún ADR, configuración,
  credencial o servicio.
- **No se ejecutó nada:** ni proveedores, ni Wyss, ni Hermes, ni pruebas o experimentos, ni en
  equipos del propietario ni en contenedores.
- **PR y `main`:** no se fusionó ninguna PR y no se tocó `main` ni la rama
  `docs/console-planning-review-2026-10-13`.
- **Comandos usados:** `git` (log, diff, show y clone en solo lectura de `SaintWyss/wyss`), `grep`,
  `sed`, `cat` y `find`.
- **Archivo creado:** únicamente este informe, en un único commit documental.
