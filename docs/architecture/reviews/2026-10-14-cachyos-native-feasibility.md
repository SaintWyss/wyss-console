# Verificación nativa de capacidades y fixtures — CachyOS KDE

Fecha: 2026-10-14. Plataforma observada: **CachyOS nativo**, no contenedor ni guest.
Repositorio `SaintWyss/wyss-console`; rama `docs/console-planning-review-2026-10-13`.
Base documental fijada: **`384f66392e5bd0585501837a9fc59613430b1127`**.
La suite principal se ejecutó a las **2026-10-14T05:33:51Z**.

## 1. Resultado y alcance

**PASS de disponibilidad y funcionamiento de las primitivas sintéticas detalladas abajo;
NOT_PROVEN para la arquitectura de producción. El REJECT del plan no se levanta.**

Se siguió primero [46](../46-CACHYOS-NATIVE-VERIFICATION-HANDOFF.md), y luego se leyeron
[43](../43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md),
[44](../44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md) y
[42](../42-POST-REJECT-CORRECTIVE-PLAN.md). El bloqueo Landlock del contenedor de 44 **no
se reproduce aquí**: este kernel ofrece Landlock ABI 7 y aplica restricciones sin privilegios.
Bubblewrap y namespaces también funcionan con fixtures locales. Ninguna VM es requisito
para estos resultados y no se utilizó Ubuntu.

Resultados principales:

- Landlock denegó nuevos accesos a archivos ficticios protegidos, incluso por symlink y desde
  un descendiente; permitió el proyecto, una política obligatoria de solo lectura y un token
  ficticio con refresh en el lugar.
- **Landlock ABI 7 por sí solo no bloqueó el socket Unix por pathname de la autoridad
  ficticia.** Tampoco revocó un descriptor abierto antes de entrar al dominio. Es un **FAIL
  del objetivo de aislamiento si se pretendiera usar solo ese perfil**, no un fallo del ensayo.
- Bubblewrap, con una vista mínima de mounts y sin exponer la autoridad, impidió resolver
  el archivo/socket ficticios; el worker pudo escribir en su proyecto y leer su política.
  **Un bind read-only de un socket sí permitió conectar y enviar datos.** Ocultar el canal
  importa; montarlo read-only no constituye una prohibición de IPC.
- El refresh por reemplazo de archivo falla con permiso/bind de un único token y funciona
  concediendo el directorio ficticio dedicado. Eso acota una incompatibilidad real de diseño;
  no prueba cómo refrescan credenciales los proveedores instalados.
- La separación por otro UID sigue sin validación nativa para Wyss Console. Además, esta
  cuenta tiene acceso al socket de Docker de un daemon root: la hipótesis de atacante sin
  autoridad administrativa necesita revisión antes de afirmar que otro UID protege la autoridad.
- `openat2`, creación de namespaces y PTY funcionaron. Los ensayos de paths demuestran
  protección acotada de resolución y una carrera sintética, no seguridad de transacciones
  Btrfs, snapshots, sincronización o código Wyss real.

**Autoría:** Codex realizó esta pasada y es autor del informe anterior en `141b748`.
No se presenta este documento como aprobación independiente de aquel informe ni de su propio
trabajo. Es evidencia nativa nueva, con límites explícitos.

## 2. Guardas y clasificación

| Clase                         | Uso en este informe                                                                         |
| ----------------------------- | ------------------------------------------------------------------------------------------- |
| READ_ONLY_HOST_FACT           | Dato obtenido de metadatos del kernel, OS, paquetes, mounts o managers existentes           |
| OBSERVED_ON_CACHYOS_SYNTHETIC | Resultado de procesos sin privilegios host y datos exclusivamente ficticios                 |
| BLOCKED_UNSAFE                | Prueba no ejecutada porque necesitaría privilegios/cambios o podría afectar el sistema real |
| BLOCKED_ENVIRONMENT           | Interfaz no disponible o acceso rechazado bajo las restricciones actuales                   |
| NOT_RUN                       | No se ensayó esa garantía; no se infiere de otra prueba                                     |

Todos los fixtures pertenecieron al UID 1000, en un directorio aleatorio `0700` bajo `/tmp`.
La comparación cross-mount usó además otro directorio propio `0700` bajo `/dev/shm`.
Solo contenían marcadores `DUMMY_AUTHORITY`, `NO_REAL_SECRET`, `MOCK_REQUIRED_POLICY` y datos
de la misma clase. Hubo sockets **AF_UNIX de pathname** dentro de esos directorios, modo `0600`;
ningún listener TCP/UDP ni contacto con sockets reales de aprobación, Docker, keyring o proveedores.

Los subprocessos usaron Python `-I -S`, entorno mínimo, cierre de FD salvo los tres canales
estándar y timeout de ocho segundos. El ensayo explícito de FD preabierto abrió únicamente
un archivo ficticio dentro del propio worker. Las denegaciones Landlock se aplicaron solo
a hijos desechables; el proceso de Codex y el escritorio no fueron restringidos. Los namespaces
y mounts de bubblewrap fueron privados y desaparecieron con sus procesos. No se crearon
usuarios del host ni se otorgaron capacidades en su namespace inicial.

Las pruebas fueron pequeñas: archivos de decenas de bytes, lotes PTY de cuatro bytes,
sin fuzz agresivo, presión de memoria, fork bomb ni carreras contra datos reales. No se
modificaron controles de cgroups, servicios, linger, grupos, paquetes o configuración de KDE.

## 3. Inventario real de CachyOS

Todos los datos de esta sección son **READ_ONLY_HOST_FACT**, tomados durante esta pasada.

| Capacidad                           | Resultado observado                                                                                                                                             | Interpretación acotada                                                                                           |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Distribución                        | `/etc/os-release`: `ID=cachyos`, `ID_LIKE=arch`, `BUILD_ID=rolling`                                                                                             | Host solicitado                                                                                                  |
| Kernel                              | `6.18.55-2-cachyos-lts`, x86_64; build 2026-10-09                                                                                                               | No extrapolar desde el kernel del contenedor de 44                                                               |
| Desktop                             | `XDG_CURRENT_DESKTOP=KDE`, `XDG_SESSION_TYPE=x11`; sesión 2                                                                                                     | Esta ejecución ocurre durante una sesión local; no se probó logout                                               |
| Entorno                             | PID 1 `systemd`; `systemd-detect-virt` y `--container`: `none`                                                                                                  | Evidencia conjunta de ejecución nativa                                                                           |
| Principal actual                    | UID/EUID 1000, GID 1000; `CapEff=0`, `NoNewPrivs=0`, `Seccomp=0`                                                                                                | El runner no tenía capacidades host; esto no elimina privilegios delegados por grupos/sockets                    |
| LSM activos                         | `capability,landlock,lockdown,yama,bpf`                                                                                                                         | Landlock activo; flags de compilación de systemd no son evidencia de una política AppArmor/SELinux activa        |
| Kernel config                       | `CONFIG_SECURITY_LANDLOCK=y`; USER_NS, NAMESPACES, PID_NS, NET_NS, CGROUPS, MEMCG, CGROUP_PIDS, SECCOMP y SECCOMP_FILTER = `y`                                  | Interfaces compiladas; los ensayos distinguen disponibilidad de funcionamiento                                   |
| Landlock ABI                        | `landlock_create_ruleset(NULL,0,VERSION)` = **7**, errno 0                                                                                                      | Se puede crear y aplicar un dominio; no asumir derechos de ABI posteriores                                       |
| User namespaces                     | `unprivileged_userns_clone=1`; `user.max_user_namespaces=62346`                                                                                                 | Creación rootless permitida; observada en CN-03/04                                                               |
| Bubblewrap                          | `0.13.0`, paquete `0.13.0-1.1`; binario root:root, `0755`, sin file capabilities informadas por getcap                                                          | Instalado y usado sin sudo ni setuid                                                                             |
| Util-linux                          | `2.42.4`, paquete `2.42.4-1.1`                                                                                                                                  | `unshare` disponible                                                                                             |
| systemd                             | Paquete `262-1`; managers system/user reportan `262-1-arch`; `/usr/lib/systemd/systemd --version` = 262                                                         | El binario no estaba en PATH como `systemd`, pero está instalado; no es una dependencia faltante                 |
| Manager de usuario                  | `systemctl --user is-system-running`: `running`, rc 0                                                                                                           | Accesible en la sesión actual; no se probó iniciar ni cambiar una unidad                                         |
| Cgroups                             | cgroup v2; controladores raíz: `cpuset cpu io memory hugetlb pids rdma misc dmem`                                                                               | No se escribieron knobs ni crearon subgrupos                                                                     |
| Cgroup de la sesión de herramientas | Bajo `user@1000.service/...WarpOss...`; controladores `cpu memory pids`; localmente `memory.max=max`, `memory.high=max`, `memory.oom.group=0`, `pids.max=18703` | Valores de ese grupo, no garantía sobre todos los ancestros ni sobre futuros workers                             |
| Delegación visible                  | `os.access(W_OK)` positivo para varios knobs del cgroup actual y directorio del manager de usuario; raíz cgroup no writable                                     | Metadatos de acceso, no prueba de límites aislados; cambiar el grupo actual afectaría procesos ajenos al fixture |
| Btrfs tools                         | `btrfs-progs 7.1-1`                                                                                                                                             | Inspección de B-tree bloqueada sin privilegios; véase §4                                                         |
| Python                              | `3.14.8`                                                                                                                                                        | Runtime de fixtures, no de proveedores                                                                           |
| Compiladores instalados             | GCC `16.2.1+r23+gd564253eb6c8-1`; Clang `23.1.1-1.1`                                                                                                            | No hizo falta compilar ni instalar nada                                                                          |
| Helper CLI `landlock-restrict`      | No encontrado en PATH                                                                                                                                           | No bloqueó pruebas: se usó directamente la syscall con ctypes                                                    |

### 3.1 Autoridad existente y privilegios administrativos heredados

Se consultó metadata limitada de managers y nombres conocidos. `getent passwd` no resolvió
`wyss-control`, `wyss-authority` ni `wyss-home` como **cuentas**. Existe un **grupo** `wyss-home`;
grupo y usuario no son intercambiables. `ps -C wyss-control -o pid,uid,comm` no encontró un
proceso con ese nombre. Esto no prueba ausencia universal de otra autoridad con distinto nombre.

El servicio existente `wyss.service` reporta `User=saintwyss`, `Group=wyss-home`,
`DynamicUser=no`, `ProtectSystem=strict`, activo. Corresponde al Wyss existente; **no se
asume que sea el `wyss-control` futuro ni un principal protegido de Console**. Una restricción
de mounts de su unidad no demuestra que otro proceso irrestricto del mismo UID no pueda
modificar los activos accesibles desde su propia vista del filesystem.

La cuenta actual pertenece a `docker` (GID 956) y `wheel`, entre otros grupos. Se observó:

```text
/run/docker.sock: socket, uid=0 gid=956 mode=0660
os.access(R_OK)=true, os.access(W_OK)=true
docker.service: loaded, active; DynamicUser=no
dockerd: UID real/efectivo/saved/fs = 0; GID = 0
```

**No se conectó al socket, consultó la API de Docker ni ejecutó un contenedor.** La
[documentación de Docker](https://docs.docker.com/engine/install/linux-postinstall/)
advierte que el grupo docker concede privilegios de nivel root. Por tanto, para A no basta
suponer que el atacante de esta cuenta solo tiene DAC ordinario. Hay que modelar ese canal
administrativo y cualquier otro privilegio delegado. La escalada concreta no se intentó;
no se modificaron membresías, daemon, socket ni políticas. `wheel` por sí solo tampoco
prueba una autorización sin contraseña: no se consultaron reglas ni credenciales sudo.

## 4. Filesystem: qué se sabe y qué no

Se usó `findmnt --target <ruta> --output TARGET,FSTYPE,FSROOT,OPTIONS` y `stat` sobre
roots conocidos; no se recorrieron archivos personales.

| Ruta/clase                                              | Observación                                                                                                       | Estado                                                      |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `/`                                                     | Btrfs, FSROOT `/@`, `subvolid=256`, rw/noatime, compress=zstd:1                                                   | READ_ONLY_HOST_FACT                                         |
| Home y repositorio                                      | Btrfs montado en `/home`, FSROOT `/@home`, `subvolid=257`, mismas opciones principales; home UID 1000 modo `0710` | READ_ONLY_HOST_FACT                                         |
| `/tmp`                                                  | tmpfs, distinto mount del home; fixtures privados `0700`                                                          | READ_ONLY_HOST_FACT + ensayos sintéticos                    |
| `/dev/shm`                                              | tmpfs, distinto mount de `/tmp`, nosuid/nodev                                                                     | READ_ONLY_HOST_FACT + un control cross-mount                |
| `/sys/fs/cgroup`                                        | cgroup2, nsdelegate y memory_recursiveprot visibles                                                               | READ_ONLY_HOST_FACT; sin escrituras                         |
| `btrfs subvolume show /home` y `/`                      | rc 1: `Could not search B-tree: Operation not permitted`                                                          | BLOCKED_ENVIRONMENT con estos privilegios; no se usó sudo   |
| `btrfs subvolume list -o /home`                         | rc 1: `can't perform the search: Operation not permitted`                                                         | BLOCKED_ENVIRONMENT; topología completa/snapshots pendiente |
| Snapshots, sync, exclusiones y archivos reales abiertos | No se inspeccionaron reglas de sincronización, contenidos de snapshots ni provider configs                        | NOT_RUN; no inferir DN-31 de la tabla de mounts             |

Los tests de exchange fueron en **tmpfs**, no en el home Btrfs. No se crearon subvolúmenes,
snapshots, mounts persistentes o directorios de prueba dentro del home. Tampoco se probó un
exchange entre subvolúmenes Btrfs. DN-21/31 y L1/L2/L5 siguen abiertos.

## 5. Ensayos sintéticos ejecutados

Todos pertenecen a **OBSERVED_ON_CACHYOS_SYNTHETIC**. Se informan resultados concretos;
PASS aquí se limita a la aserción del fixture, nunca a Wyss Console completo.

### 5.1 Misma cuenta y user namespaces

| Caso             | Procedimiento y expectativa                                                                                                          | Observado                                                                                | Resultado acotado                                                                                         |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| CN-01-SAME-UID   | Child independiente UID 1000 abre archivo `0600` y conecta a socket `0600` bajo directorio `0700` del mismo usuario                  | Lee `DUMMY_AUTHORITY`; servidor recibe `SYNTHETIC_ONLY`; SO_PEERCRED UID/GID = 1000/1000 | Confirmada la limitación de identidad humana por UID; no se llamó una API real                            |
| CN-03-USERNS     | `unshare --user --map-root-user` y lectura de ese archivo ficticio                                                                   | UID interior 0; map `0 1000 1`; lectura permitida                                        | PASS del control negativo: root en userns no creó otro usuario host protegido                             |
| CN-04-NAMESPACES | `unshare --user --map-root-user --mount --pid --fork --net --ipc --uts --cgroup` ejecuta solo Python y lee identidades de namespaces | rc 0, UID interior 0, PID 1; namespaces nuevos user/mnt/pid/net/ipc/uts/cgroup           | PASS de creación rootless; no prueba cuotas, egress de un proveedor ni contención de procesos ya externos |

### 5.2 Landlock ABI 7 aplicado a un worker

Se crearon reglas con `handled_access_fs=0xffff` (derechos 0..15),
`handled_access_net=3` y `scoped=3` (abstract Unix + signals); `PR_SET_NO_NEW_PRIVS=1`.
Se aplicaron mediante `landlock_restrict_self(...,5)` en un child single-threaded.
`5` corresponde a flags de supresión de audit para same-exec/subdominios en esta ABI;
no altera el manager ni su configuración. La red TCP no se ejercitó: incluir derechos en
el ruleset no se cuenta como validación de egress.

Grants: proyecto RW; archivo de política READ_FILE; token ficticio READ_FILE/WRITE_FILE/TRUNCATE
en variante **FILE**, o directorio ficticio dedicado RW en variante **DIRECTORY**. Nada de la
autoridad fue permitido por reglas de archivos. Código/imports del fixture ya estaban cargados
antes de restringirse; no es una prueba de startup dinámico de un proveedor.

| Operación en CN-02                                          | Esperado                                                   | FILE / DIRECTORY observado                        | Interpretación                                                                                                  |
| ----------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| Escribir resultado en proyecto                              | Permitido                                                  | Éxito, 17 bytes, ambas                            | Función útil mínima conservada                                                                                  |
| Leer política obligatoria ficticia                          | Permitido                                                  | `MOCK_REQUIRED_POLICY`, ambas                     | No se la omitió para hacer pasar el test; no se ejecutaron hooks reales                                         |
| Leer/escribir nuevo FD de estado autoridad                  | Denegado                                                   | EACCES 13, ambas                                  | PASS de restricción de archivos                                                                                 |
| Symlink en proyecto apunta a estado autoridad               | Denegado                                                   | EACCES 13, ambas                                  | La concesión del proyecto no permitió leer ese target                                                           |
| Leer token ficticio                                         | Permitido                                                  | `NO_REAL_SECRET`, ambas                           | No se leyó auth real                                                                                            |
| Refresh de token en el lugar                                | Permitido                                                  | Éxito, 20 bytes, ambas                            | Compatible con este mock de escritura in-place                                                                  |
| Refresh creando token.new + os.replace                      | FILE deniega; DIRECTORY permite                            | FILE: EACCES 13; DIRECTORY: `FAKE_REFRESH_RENAME` | FAIL de compatibilidad para refresh por reemplazo con grant de archivo; ampliar al directorio cambia el alcance |
| Descendiente fork lee autoridad                             | Denegado                                                   | EACCES 13, ambas                                  | Restricción heredada en el fixture                                                                              |
| Agregar después ruleset permisivo sobre todo el fixture     | No relajar el anterior                                     | Lectura autoridad sigue EACCES 13, ambas          | El proceso no removió su restricción anterior                                                                   |
| `kill(runner_pid,0)`                                        | Sin entregar señal; permiso hacia dominio externo denegado | EPERM 1, ambas                                    | Control de permiso para signal 0; no se enviaron SIGTERM/SIGKILL a procesos reales                              |
| Leer FD de estado abierto **antes** de restringirse         | Mostrar alcance residual                                   | Lee `DUMMY_AUTHORITY`, ambas                      | FAIL si se supone revocación de FD heredados; cerrar/revisar capacidades antes del sandbox                      |
| Conectar a socket **por pathname** de la autoridad ficticia | Detectar límite de esta ABI                                | Éxito, servidor recibe `SYNTHETIC_ONLY`, ambas    | **FAIL para aislamiento IPC solo con este Landlock**                                                            |

El estado de autoridad no cambió en estas pruebas. Python 3.14 emitió un DeprecationWarning
por el layout ctypes `_pack_`; las syscalls devolvieron éxito y los resultados arriba. Se
conserva como limitación de portabilidad del fixture, no como advertencia de seguridad del host.

La [documentación del kernel](https://docs.kernel.org/userspace-api/landlock.html)
sitúa el control `LANDLOCK_ACCESS_FS_RESOLVE_UNIX` en ABI 9 y TSYNC en ABI 8. Los headers
instalados contienen macros posteriores, pero el runtime responde ABI 7. No se debe habilitar
una propiedad de confinamiento por encontrar su macro. El launcher debe restringirse antes
de crear threads, revisar FD y resolver IPC por mecanismos complementarios. No se cambió el kernel.

### 5.3 Bubblewrap rootless y control de canales

Perfil exacto de flags principales:

```text
bwrap --unshare-all --new-session --die-with-parent --cap-drop ALL --clearenv
  --setenv PATH /usr/bin
  --ro-bind /usr /usr --symlink usr/lib /lib --symlink usr/lib /lib64
  --proc /proc --dev /dev --dir /fixture
  --bind <fixture>/project /fixture/project
  --dir /fixture/provider --ro-bind <fixture>/hooks /fixture/hooks
  --chdir /fixture/project
  <bind del token individual o de su directorio ficticio>
  /usr/bin/python3 -I -S -c <mock fijo>
```

No se expusieron home, `/run`, runtime-dir, X11, session D-Bus, Docker, KWallet ni interfaces
de proveedores. `/usr` se expuso read-only como runtime; su integridad frente al modelo completo
de atacante no fue auditada. `--dev /dev` es privado de bubblewrap; no se otorgó acceso al
árbol de dispositivos del host ni se ejercitó hardware. No se añadió filtro seccomp propio.

| Operación                                                         | FILE / DIRECTORY observado                                                            | Resultado acotado                                                                   |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Control sin sandbox al socket vivo                                | Éxito antes de las variantes; `SYNTHETIC_ONLY`                                        | La denegación no se atribuye a un servidor inexistente                              |
| Proyecto escribible                                               | Éxito, 5 bytes, ambas                                                                 | PASS de uso mínimo                                                                  |
| Política ficticia legible pero no editable                        | Lectura correcta; escritura EROFS 30                                                  | PASS del mount read-only de archivo                                                 |
| Estado y socket por path original no expuesto                     | ENOENT 2, ambas                                                                       | PASS de ocultamiento por vista de mounts en este perfil                             |
| Refresh ficticio in-place                                         | Éxito, 18 bytes, ambas                                                                | PASS del mock                                                                       |
| Crear token.new + replace                                         | FILE: EBUSY 16; DIRECTORY: `FAKE_BWRAP_RENAME`                                        | Bind por archivo rompe ese refresh; bind del directorio dedicado lo conserva        |
| Namespace net                                                     | `/proc/net/dev` muestra solo `lo`, contadores cero                                    | Sin tráfico externo; no demuestra egress selectivo útil                             |
| CN-11: **ro-bind intencional del directorio del socket ficticio** | Child conecta, manda `SYNTHETIC_RO_SOCKET`; peer visto desde host = UID/GID 1000/1000 | Confirmada la insuficiencia de read-only para impedir conexión a un socket expuesto |

Dentro de bubblewrap se vio UID 1000 y `uid_map` inmediato `1000 0 1`. En namespaces
anidados ese mapa no se interpreta directamente como UID del host. CN-11 aportó la observación
desde el servidor externo: el peer host seguía siendo 1000. No se adquirió UID 0 del host
ni se creó un principal protegido de autoridad por cambiar la representación interna.

Esto demuestra una opción B viable para seguir investigando **a nivel de primitivas**, no
un sandbox de proveedor aprobado. El diseño debe revisar cada mount y FD, sockets alternativos,
procfs, señales, red, brokers y plugins. El perfil sin red no satisface inferencia real: ese
requisito sigue NOT_PROVEN y no se elimina del producto para obtener un PASS.

### 5.4 Resolución de paths, exchange y PTY

| Caso                       | Procedimiento                                                                                                                                      | Observado / resultado                                                                            |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| CN-06-OPENAT2 positivo     | Dirfd de raíz ficticia; syscall 437; O_WRONLY/O_CREAT/O_EXCL/O_CLOEXEC, mode 0600; RESOLVE_BENEATH/NO_SYMLINKS/NO_MAGICLINKS (`0x0e`); `nested/ok` | Crea y escribe el archivo; PASS acotado                                                          |
| CN-06 parent/absolute      | Mismos parámetros; `../authority/escape` o path absoluto solo dentro del fixture                                                                   | EXDEV 18; no se crea canary afuera                                                               |
| CN-06 symlink              | Ancestro `redir` apunta al directorio ficticio protegido                                                                                           | ELOOP 40; canary intacto                                                                         |
| CN-07 precheck/use         | Resolve de path permitido; rename del directorio; symlink sustituto a otro directorio ficticio; write por pathname viejo                           | **Redirigido** a archivo dummy de autoridad; reproduce una clase de TOCTOU, no explota Wyss real |
| CN-07 variante anclada     | Dirfd abierto antes del swap con O_DIRECTORY/O_NOFOLLOW; creación relativa O_CREAT/O_EXCL/O_NOFOLLOW                                               | Escribe en el directorio original renombrado; no en destino del symlink                          |
| CN-08 exchange mismo mount | `renameat2(...,RENAME_EXCHANGE=2)` entre dos archivos propios en `/tmp`                                                                            | rc 0; A/B quedan B/A; filesystem **tmpfs**                                                       |
| CN-09 cross-mount          | Mismo exchange entre archivo propio de `/tmp` y archivo propio de `/dev/shm`                                                                       | rc -1, EXDEV 18; ambos contenidos conservados                                                    |
| CN-10 PTY                  | openpty, modo raw, TIOCSWINSZ/TIOCGWINSZ; child fijo lee cuatro bytes PING y responde                                                              | Tamaño 31×97; `MOCK:PING`; rc 0, child reaped                                                    |

El ensayo determinista de swap no ejecutó concurrencia contra un proyecto real. Un único
dirfd no resuelve la política de todos los ancestros, hardlinks, mounts, contenidos mutables
ni rollback. [openat2(2)](https://man7.org/linux/man-pages/man2/openat2.2.html) describe las
restricciones de resolución; aquí solo se comprobaron los casos de la tabla.
El PTY no prueba fidelidad ANSI, parser aislado, OOM, desconexión, linger ni vida tras logout.

## 6. Comparación de arquitecturas con esta evidencia

S1 = autenticidad de aprobación humana; S2 = integridad de autoridad/código/estado/política;
S3 = integridad del resto de la cuenta y proyectos, siguiendo 43.

| Aspecto                            | A: autoridad protegida                                                                          | B: workers restringidos                                                                                                    | Resultado de esta pasada                                                                           |
| ---------------------------------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Principal real distinto            | Requiere instalación/custodia que el atacante no controle                                       | Userns rootless probado conserva vínculo con UID host; Landlock/bwrap restringen procesos específicos                      | A **NOT_PROVEN** nativo; B no crea por sí mismo un UID host protegido                              |
| S1 aprobación humana               | Hace falta emisor protegido que ligue bytes mostrados a la acción; UI mismo UID no basta        | Bloquear IPC de un worker no protege frente a otro proceso irrestricto ni prueba consentimiento                            | Ambos **NOT_PROVEN**; no se implementó firma/Android/native confirmation                           |
| S2 archivos/IPC                    | Otro UID puede ser base, pero depende de binarios/unidades/DB y privilegios reales del atacante | Landlock deniega archivos; ABI 7 deja pathname sockets; bwrap los oculta si no se montan; FD preabiertos siguen importando | B tiene evidencia parcial útil; ningún PASS global de S2                                           |
| Cuenta con administración delegada | Docker root accesible hace inadecuada la hipótesis de atacante meramente DAC sin más análisis   | Perfil mínimo omitió `/run`, credenciales y servicios; no validó toda fuga a un broker privilegiado                        | Restricción crítica de alcance para A; evitar toda promesa basada solo en UID distinto             |
| S3 cuenta completa                 | No contenida si nativos/LP4 quedan irrestrictos                                                 | Solo procesos que realmente entren al perfil quedan acotados                                                               | No llamar aislada a la sesión nativa irrestricta                                                   |
| Auth existente                     | Puede conservar el flujo nativo, pero cómo se supervisa el runtime desde A sigue abierto        | Refresh in-place del mock funciona; reemplazo requiere más permisos y ambos perfiles por archivo lo rompen                 | Compatibilidad real Claude/Codex/Hermes **NOT_PROVEN**; no copiar auth ni ampliar al home completo |
| Hooks/policies                     | Su confianza y ownership deben protegerse                                                       | Lectura mock preservada, mount read-only observado; no se ejecutaron hooks ni MCP                                          | No sacrificar controles obligatorios para conseguir startup                                        |
| Red útil                           | Separar requests/approval/proveedor sigue pendiente                                             | Namespace sin red aísla, pero rompe inferencia; egress selectivo no ensayado                                               | No confundir «sin red» con proveedor funcional                                                     |
| Instalación personal               | User manager operativo no permite asumir creación de otro usuario con User=                     | Los fixtures rootless funcionan sin aprovisionamiento                                                                      | A requiere decisión/validación futura; B permite continuar investigación segura ahora              |
| Recursos/parser/persistencia       | Una autoridad separada no resuelve PTYs, procesos huérfanos ni parser                           | Nuevos PID/namespaces no prueban dominio OOM independiente                                                                 | Cgroups detectados; límites/enforcement, crash, logout y recuperación **NOT_RUN**                  |

**Juicio provisional:** continuar con B como perfil sintético de comparación es viable en este
CachyOS sin instalaciones. A sigue siendo candidata para conservar sesiones nativas completas,
pero necesita redefinir y verificar su modelo de atacante considerando privilegios administrativos
de la cuenta, además del emisor de consentimiento y aprovisionamiento. Ninguna ruta está
seleccionada ni implementada; un híbrido no hereda automáticamente las garantías de ambas.

## 7. Qué falta y qué sería peligroso en este sistema

Estas restricciones no detuvieron L0/L1. Cada fila queda pendiente sin intentar elevar permisos.

| Prueba o cambio                                                                                      | Estado                                                            | Razón y resultado que todavía falta                                                                           |
| ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Crear UID/grupos/ACL para autoridad, usar setuid a otro usuario host, sudo/polkit                    | BLOCKED_UNSAFE                                                    | Cambia identidad/privilegios o exige autorización fuera de esta tarea; otro UID real no se probó aquí         |
| Instalar/modificar unidades, iniciar/reiniciar Wyss u otros servicios, systemd-run, habilitar linger | BLOCKED_UNSAFE                                                    | El usuario prohibió cambios de servicios; no convertir un test transitorio en excepción silenciosa            |
| Ejercitar Docker root, cached sudo o un broker administrativo                                        | BLOCKED_UNSAFE                                                    | Metadatos bastan para identificar el problema de amenaza; no ejecutar escalada ni contenedores                |
| Escribir knobs/mover procesos en cgroups reales                                                      | BLOCKED_UNSAFE                                                    | Algunos knobs son writable, pero el grupo actual pertenece a la sesión/herramientas; afectaría otros procesos |
| OOM, límites agresivos, matar managers/autoridad, logout/reboot/suspend                              | BLOCKED_UNSAFE                                                    | Puede cerrar trabajo real; no necesario para comprobar Landlock/bwrap/paths con fixtures                      |
| PTY parser hostil con presión de memoria o FD hacia servicios reales                                 | BLOCKED_UNSAFE                                                    | No se validó aislamiento suficiente para aceptar ese efecto en el host normal                                 |
| Ampliar btrfs show/list mediante privilegios; crear snapshot/subvolume/mount                         | BLOCKED_ENVIRONMENT / BLOCKED_UNSAFE para el remedio privilegiado | La inspección devolvió EPERM; no alterar el filesystem para completar una tabla                               |
| Races sobre Claude/Codex/Hermes/Wyss reales, snapshots o réplica del home                            | BLOCKED_UNSAFE                                                    | Solo se autorizó dato ficticio en directorios propios; DN-31 requiere otro procedimiento delimitado           |
| Startup real/login/refresh, hooks, MCP, keyring, sesión D-Bus/X11, autenticación móvil               | NOT_RUN                                                           | Prohibidos providers/config/auth reales en esta pasada; el mock no prueba compatibilidad instalada            |
| Human approval firmado, LP4-away, revocación versus STARTING, replay tras crash                      | NOT_RUN                                                           | Requiere contratos y fixtures específicos adicionales; UID/socket y PTY no prueban esas propiedades           |
| Filtro seccomp y egress selectivo                                                                    | NOT_RUN                                                           | Kernel capaz, pero no se diseñó/aplicó ese filtro ni se hizo tráfico de red                                   |
| Composición completa Landlock+bwrap+FD hygiene+cgroups                                               | NOT_RUN                                                           | Se probaron primitivas y perfiles parciales, no una política combinada final                                  |

No se propone una VM como paso genérico. Lo siguiente que puede hacerse sin privilegios
es diseñar y ensayar mocks adicionales de la política combinada, gates de aprobación y
compatibilidad **ficticia**, siempre con los mismos límites. Una prueba de alto riesgo solo
podrá reabrirse con objetivo y autorización específicos; su ausencia no invalida lo ya observado.

## 8. Decisiones del propietario que aún no corresponde inventar

1. Si A requiere aprovisionamiento privilegiado real o cambiar el supuesto de acceso
   administrativo de la cuenta, decidir ese compromiso con un diseño concreto. Este informe
   no autoriza quitar grupos ni modificar Docker/sudo/systemd.
2. Qué funciones nativas debe conservar un perfil B para OL3 y cuáles, si resulta inevitable,
   puede limitar exclusivamente ese perfil. No llamar «nativo irrestricto» al worker restringido.
3. Modelo final de consentimiento, recuperación y LP4-away. Ningún éxito de filesystem equivale
   a aprobación humana, ni la revocación remota restaura una cuenta previamente comprometida.
4. Si un refresh real exige ampliar acceso a credenciales/configuración, evaluar alternativas
   compatibles con DN-14 antes de pedir aceptación de una ampliación concreta.
5. DN-21/31, únicamente con evidencia de una transacción y del entorno real de sincronización/
   snapshots. El EXDEV sintético no decide esa política.

No se vuelve a pedir elegir distribuciones, instalar herramientas disponibles ni crear un guest
para repetir las primitivas que ya funcionaron nativamente.

## 9. Evidencia, reproducción y limpieza

Suite: `python3 -I -S <temp-0700>/probe.py`.
SHA-256 del runner: **`51319049d5fa524e32a18195fb3570f171259130e0cd0a2867badf6f43b046ef`**.
SHA-256 del JSON principal:
`6292acc54e6fc1f0eef1343ed9b76c80e63d4dc61c195228d6bf825818ac8e3c`.
SHA-256 del JSON del control de socket read-only:
`49aecfbbe113a17c9b6236862c620591aba9d8d22417d4c053aba4fba72a76a4`.

Los hashes identifican los artefactos temporales originales; no sustituyen los resultados
de §5. El runner íntegro se conserva como texto en el anexo, sin instalar un harness en el
producto. Para repetirlo hay que crear un nuevo directorio propio `0700` bajo `/tmp`, guardar
allí ese texto como `probe.py` y ejecutar bajo las mismas guardas. El código se limita a
CachyOS x86_64/UID 1000 y no debe presentarse como herramienta portable de producción.

El control CN-11 se reprodujo con el mismo perfil base de bubblewrap, sustituyendo sus binds
de datos por `--ro-bind <dummy-authority> /fixture/exposed`. Un servidor AF_UNIX `0600` propio
estuvo vivo en ese directorio; el child conectó a `/fixture/exposed/mock.sock`, envió el marcador
y cerró; el padre leyó SO_PEERCRED y esperó su salida. No hubo API real ni efecto persistente.

`TemporaryDirectory` eliminó los árboles de datos ficticios de ambas clases de mount; sockets
cerrados y todos los hijos esperados/reaped. Al terminar la documentación se eliminaron únicamente
los tres archivos conocidos del directorio del runner y ese directorio vacío, verificando
su propiedad/identidad. No se hizo limpieza recursiva de una ruta personal o preexistente.

**Cambios documentales:** únicamente este nuevo informe, en un commit de la rama de planificación.
Sin cambios de aplicación, ADR, configuración personal/de proveedor, credenciales, unidades,
paquetes ni `main`. No se ejecutaron Claude Code, Codex App Server, Hermes ni Wyss. No hubo
subagentes, escalada de privilegios o fusión de PR.

## 10. Fuentes y límites documentales

- [46: procedimiento nativo autorizado](../46-CACHYOS-NATIVE-VERIFICATION-HANDOFF.md).
- [43: comparación A/B/S1/S2/S3](../43-AUTHORITY-ISOLATION-FEASIBILITY-MATRIX.md).
- [44: observaciones previas de otro entorno](../44-DISPOSABLE-TRUST-FIXTURE-OBSERVATIONS.md).
- [42: gates que siguen abiertos](../42-POST-REJECT-CORRECTIVE-PLAN.md).
- [Landlock, documentación del kernel](https://docs.kernel.org/userspace-api/landlock.html):
  derechos por ABI, FD y scoping; no evidencia de un proveedor Wyss confinado.
- [Bubblewrap upstream](https://github.com/containers/bubblewrap/blob/main/README.md): la
  herramienta depende de la política de mounts/FD/canales; no selecciona por sí sola el contrato Wyss.
- [openat2(2)](https://man7.org/linux/man-pages/man2/openat2.2.html): resolución anclada y errores.
- [systemd.exec(5)](https://man7.org/linux/man-pages/man5/systemd.exec.5.html): identidad y
  sandbox de servicios; ningún servicio nuevo fue probado.
- [Docker, privilegios del grupo](https://docs.docker.com/engine/install/linux-postinstall/):
  alcance administrativo del socket; sin ejecución de la API ni hardening en esta pasada.

Las consultas upstream fueron lectura documental. Las observaciones locales prevalecen para
disponibilidad/versiones; documentación de una ABI más nueva no se cuenta como capacidad
del kernel observado. Estos datos son de un solo host y de esta sesión, no de todos los
computadores del propietario.

## Anexo — runner sintético ejecutado

<details>
<summary>Código completo del fixture principal, sin componentes del producto</summary>

```python
import ctypes, errno, fcntl, hashlib, json, os, platform, pty, select, socket
import stat, struct, subprocess, sys, tempfile, termios, time, tty
from pathlib import Path

assert os.getuid() == 1000 and os.geteuid() == 1000
assert platform.machine() == 'x86_64'
assert 'ID=cachyos' in Path('/etc/os-release').read_text()
assert 'cachyos' in platform.release()
BASE = Path(__file__).parent
assert BASE.parent == Path('/tmp') and BASE.name.startswith('wyss-cachyos-native.')
assert BASE.stat().st_uid == os.getuid() and stat.S_IMODE(BASE.stat().st_mode) == 0o700
ENV = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C', 'PYTHONDONTWRITEBYTECODE': '1'}
OUT = {'kernel': platform.release(), 'uid': os.getuid(), 'started_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()), 'tests': []}

def record(name, result):
    OUT['tests'].append({'id': name, **result})

def run(cmd, timeout=8, **kwargs):
    try:
        p = subprocess.run(cmd, env=ENV, capture_output=True, text=True, timeout=timeout, close_fds=True, **kwargs)
        return {'rc': p.returncode, 'stdout': p.stdout.strip(), 'stderr': p.stderr.strip()}
    except subprocess.TimeoutExpired:
        return {'timeout': timeout}

LL = r'''
import ctypes, errno, json, os, socket, sys
root, mode, outside_pid = sys.argv[1:]
libc = ctypes.CDLL(None, use_errno=True)
class Rules(ctypes.Structure):
    _fields_=[('fs',ctypes.c_uint64),('net',ctypes.c_uint64),('scoped',ctypes.c_uint64)]
class Beneath(ctypes.Structure):
    _pack_=1
    _fields_=[('access',ctypes.c_uint64),('fd',ctypes.c_int32)]
def syscall(n,*args):
    ret=libc.syscall(n,*args)
    if ret < 0: raise OSError(ctypes.get_errno(),os.strerror(ctypes.get_errno()))
    return ret
MASK=(1<<16)-1
RWFILE=(1<<1)|(1<<2)|(1<<14)
def policy(grants):
    a=Rules(MASK,3,3)
    fd=syscall(444,ctypes.byref(a),ctypes.sizeof(a),0)
    try:
        for path,rights in grants:
            pfd=os.open(path,os.O_PATH|os.O_CLOEXEC)
            try:
                b=Beneath(rights,pfd);syscall(445,fd,1,ctypes.byref(b),0)
            finally:os.close(pfd)
        if libc.prctl(38,1,0,0,0):raise OSError(ctypes.get_errno(),'PR_SET_NO_NEW_PRIVS')
        syscall(446,fd,5)
    finally:os.close(fd)
def attempt(fn):
    try:return {'ok':True,'value':fn()}
    except OSError as e:return {'ok':False,'errno':e.errno,'error':errno.errorcode.get(e.errno)}
def read(path):
    with open(path) as f:return f.read()
def write(path,value):
    with open(path,'w') as f:return f.write(value)
oldfd=os.open(root+'/authority/state',os.O_RDONLY|os.O_CLOEXEC)
auth=root+'/provider' if mode=='directory' else root+'/provider/token'
policy([(root+'/project',MASK),(auth,MASK if mode=='directory' else RWFILE),(root+'/hooks/mandatory',1<<2)])
out={'mode':mode,'uid':os.getuid()}
out['project_write']=attempt(lambda:write(root+'/project/result','SYNTHETIC_PROJECT'))
out['hook_read']=attempt(lambda:read(root+'/hooks/mandatory'))
out['authority_read']=attempt(lambda:read(root+'/authority/state'))
out['authority_write']=attempt(lambda:write(root+'/authority/state','BAD'))
out['symlink_to_authority']=attempt(lambda:read(root+'/project/redirect'))
out['token_read']=attempt(lambda:read(root+'/provider/token'))
out['token_refresh_inplace']=attempt(lambda:write(root+'/provider/token','FAKE_REFRESH_INPLACE'))
def replace():
    write(root+'/provider/token.new','FAKE_REFRESH_RENAME')
    os.replace(root+'/provider/token.new',root+'/provider/token')
    return read(root+'/provider/token')
out['token_refresh_replace']=attempt(replace)
out['preopened_fd_read']=attempt(lambda:os.read(oldfd,128).decode())
os.close(oldfd)
def connect():
    s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.settimeout(2)
    try:s.connect(root+'/authority/mock.sock');s.sendall(b'SYNTHETIC_ONLY');return 'CONNECTED'
    finally:s.close()
out['pathname_socket']=attempt(connect)
out['signal_zero_to_unrestricted_runner']=attempt(lambda:os.kill(int(outside_pid),0))
r,w=os.pipe();pid=os.fork()
if pid==0:
    os.close(r)
    a=attempt(lambda:read(root+'/authority/state'))
    os.write(w,json.dumps(a).encode());os.close(w);os._exit(0)
os.close(w);out['descendant_authority_read']=json.loads(os.read(r,1024));os.close(r);os.waitpid(pid,0)
policy([(root,MASK)])
out['after_more_permissive_ruleset']=attempt(lambda:read(root+'/authority/state'))
print(json.dumps(out,sort_keys=True))
'''

with tempfile.TemporaryDirectory(prefix='fixtures-',dir=BASE) as raw:
    root=Path(raw)
    for d in ['authority','provider','project','hooks']: (root/d).mkdir(mode=0o700)
    for path,value in [('authority/state','DUMMY_AUTHORITY'),('provider/token','NO_REAL_SECRET'),('hooks/mandatory','MOCK_REQUIRED_POLICY')]:
        p=root/path;p.write_text(value);p.chmod(0o600)
    (root/'project/redirect').symlink_to(root/'authority/state')
    listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    listener.bind(str(root/'authority/mock.sock'));os.chmod(root/'authority/mock.sock',0o600);listener.listen(4);listener.settimeout(3)
    try:
        baseline="import os,socket,sys,json; r=sys.argv[1]; s=socket.socket(socket.AF_UNIX); s.connect(r+'/authority/mock.sock'); s.sendall(b'SYNTHETIC_ONLY'); s.close(); print(json.dumps({'uid':os.getuid(),'state':open(r+'/authority/state').read()}))"
        r=run([sys.executable,'-I','-S','-c',baseline,str(root)])
        peer,_=listener.accept()
        with peer:
            r['peer_credentials']=struct.unpack('3i',peer.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))[1:]
            r['received']=peer.recv(64).decode()
        record('CN-01-SAME-UID',r)
        for mode in ['file','directory']:
            (root/'provider/token').write_text('NO_REAL_SECRET')
            r=run([sys.executable,'-I','-S','-c',LL,str(root),mode,str(os.getpid())])
            if r.get('rc')==0:
                r['observation']=json.loads(r.pop('stdout'))
                if r['observation']['pathname_socket'].get('ok'):
                    peer,_=listener.accept()
                    with peer:r['mock_received']=peer.recv(64).decode()
            record('CN-02-LANDLOCK-'+mode.upper(),r)
    finally: listener.close()
    assert (root/'authority/state').read_text()=='DUMMY_AUTHORITY'

    ns="import json,os,sys; print(json.dumps({'uid_inside':os.getuid(),'uid_map':open('/proc/self/uid_map').read().strip(),'dummy_read':open(sys.argv[1]).read()}))"
    record('CN-03-USERNS',run(['/usr/bin/unshare','--user','--map-root-user',sys.executable,'-I','-S','-c',ns,str(root/'authority/state')]))
    nsall="import json,os; print(json.dumps({'uid':os.getuid(),'pid':os.getpid(),'namespaces':{n:os.readlink('/proc/self/ns/'+n) for n in ['user','mnt','pid','net','ipc','uts','cgroup']}}))"
    record('CN-04-NAMESPACES',run(['/usr/bin/unshare','--user','--map-root-user','--mount','--pid','--fork','--net','--ipc','--uts','--cgroup',sys.executable,'-I','-S','-c',nsall]))

    # No real home, runtime socket, D-Bus, display or network is bound into the mock worker.
    bwbase=['/usr/bin/bwrap','--unshare-all','--new-session','--die-with-parent','--cap-drop','ALL','--clearenv','--setenv','PATH','/usr/bin','--ro-bind','/usr','/usr','--symlink','usr/lib','/lib','--symlink','usr/lib','/lib64','--proc','/proc','--dev','/dev','--dir','/fixture','--bind',str(root/'project'),'/fixture/project','--dir','/fixture/provider','--ro-bind',str(root/'hooks'),'/fixture/hooks','--chdir','/fixture/project']
    bwcode=r'''
import errno,json,os,socket,sys
denied=sys.argv[1]
def attempt(fn):
    try:return {'ok':True,'value':fn()}
    except OSError as e:return {'ok':False,'errno':e.errno,'error':errno.errorcode.get(e.errno)}
def read(p):
    with open(p) as f:return f.read()
def write(p,v):
    with open(p,'w') as f:return f.write(v)
out={'uid_inside':os.getuid(),'uid_map':read('/proc/self/uid_map').strip(),'net_interfaces':os.listdir('/sys/class/net') if os.path.exists('/sys/class/net') else read('/proc/net/dev').splitlines()[2:]}
out['project_write']=attempt(lambda:write('/fixture/project/bwrap-result','DUMMY'))
out['hook_read']=attempt(lambda:read('/fixture/hooks/mandatory'))
out['hook_write']=attempt(lambda:write('/fixture/hooks/mandatory','BAD'))
out['outside_state']=attempt(lambda:read(denied+'/state'))
def connect():
    s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.settimeout(2)
    try:s.connect(denied+'/mock.sock');return 'CONNECTED'
    finally:s.close()
out['outside_path_socket']=attempt(connect)
out['token_read']=attempt(lambda:read('/fixture/provider/token'))
out['token_refresh_inplace']=attempt(lambda:write('/fixture/provider/token','FAKE_BWRAP_INPLACE'))
def replace():
    write('/fixture/provider/token.new','FAKE_BWRAP_RENAME')
    os.replace('/fixture/provider/token.new','/fixture/provider/token')
    return read('/fixture/provider/token')
out['token_refresh_replace']=attempt(replace)
print(json.dumps(out,sort_keys=True))
'''
    (root/'authority/mock.sock').unlink()
    live=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    live.bind(str(root/'authority/mock.sock'));os.chmod(root/'authority/mock.sock',0o600);live.listen(4);live.settimeout(3)
    try:
        control=run([sys.executable,'-I','-S','-c',baseline,str(root)])
        peer,_=live.accept()
        with peer:control['received']=peer.recv(64).decode()
        record('CN-05-BWRAP-CONTROL',control)
        for mode in ['file','directory']:
            (root/'provider/token').write_text('NO_REAL_SECRET')
            auth=['--bind',str(root/'provider/token'),'/fixture/provider/token'] if mode=='file' else ['--bind',str(root/'provider'),'/fixture/provider']
            cmd=bwbase+auth+['/usr/bin/python3','-I','-S','-c',bwcode,str(root/'authority')]
            r=run(cmd)
            if r.get('rc')==0:r['observation']=json.loads(r.pop('stdout'))
            record('CN-05-BWRAP-'+mode.upper(),r)
    finally:live.close()
    assert (root/'authority/state').read_text()=='DUMMY_AUTHORITY'
    assert (root/'hooks/mandatory').read_text()=='MOCK_REQUIRED_POLICY'

    libc=ctypes.CDLL(None,use_errno=True)
    class How(ctypes.Structure):
        _fields_=[('flags',ctypes.c_uint64),('mode',ctypes.c_uint64),('resolve',ctypes.c_uint64)]
    safe=root/'resolution';safe.mkdir();(safe/'nested').mkdir();(safe/'redir').symlink_to(root/'authority')
    dfd=os.open(safe,os.O_DIRECTORY|os.O_RDONLY|os.O_CLOEXEC)
    resolutions={}
    try:
        for key,path in [('valid','nested/ok'),('parent_escape','../authority/escape'),('symlink','redir/escape'),('absolute',str(root/'authority/escape'))]:
            h=How(os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_CLOEXEC,0o600,0x08|0x04|0x02)
            ctypes.set_errno(0);fd=libc.syscall(437,dfd,path.encode(),ctypes.byref(h),ctypes.sizeof(h))
            e=ctypes.get_errno();resolutions[key]={'ok':fd>=0,'errno':e,'error':errno.errorcode.get(e)}
            if fd>=0:os.write(fd,b'DUMMY');os.close(fd)
    finally:os.close(dfd)
    resolutions['outside_canary_unchanged']=not (root/'authority/escape').exists()
    record('CN-06-OPENAT2',resolutions)

    work=root/'swap';work.mkdir();(work/'src').mkdir();dest=work/'src/file'
    approved=dest.resolve();dfd=os.open(work/'src',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
    try:
        (work/'src').rename(work/'saved');(work/'src').symlink_to(root/'authority')
        approved.write_text('DUMMY_REDIRECTED')
        fd=os.open('anchored',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600,dir_fd=dfd)
        os.write(fd,b'DUMMY_ANCHORED');os.close(fd)
    finally:os.close(dfd)
    record('CN-07-PATH-SWAP',{'pathname_redirected':(root/'authority/file').read_text()=='DUMMY_REDIRECTED','anchored_in_saved':(work/'saved/anchored').read_text()=='DUMMY_ANCHORED','anchored_outside_absent':not (root/'authority/anchored').exists()})

    a=root/'exchange-a';b=root/'exchange-b';a.write_text('A');b.write_text('B')
    ctypes.set_errno(0);ret=libc.renameat2(-100,str(a).encode(),-100,str(b).encode(),2);e=ctypes.get_errno()
    record('CN-08-RENAME-EXCHANGE',{'rc':ret,'errno':e,'a':a.read_text(),'b':b.read_text(),'filesystem':'tmpfs'})
    with tempfile.TemporaryDirectory(prefix='wyss-cachyos-cross-',dir='/dev/shm') as raw2:
        other=Path(raw2)/'cross';other.write_text('C')
        ctypes.set_errno(0);ret=libc.renameat2(-100,str(a).encode(),-100,str(other).encode(),2);e=ctypes.get_errno()
        record('CN-09-CROSS-MOUNT',{'rc':ret,'errno':e,'error':errno.errorcode.get(e),'a':a.read_text(),'other':other.read_text()})
    assert not Path(raw2).exists()

    master,slave=pty.openpty()
    child=None
    try:
        tty.setraw(slave)
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',31,97,0,0))
        size=struct.unpack('HHHH',fcntl.ioctl(slave,termios.TIOCGWINSZ,b'\0'*8))[:2]
        child=subprocess.Popen([sys.executable,'-I','-S','-c',"import os; x=os.read(0,4); os.write(1,b'MOCK:'+x)"],stdin=slave,stdout=slave,stderr=slave,env=ENV,close_fds=True)
        os.close(slave);slave=-1;os.write(master,b'PING');ready=bool(select.select([master],[],[],3)[0]);data=os.read(master,128).decode() if ready else ''
        child.wait(timeout=3)
        record('CN-10-PTY',{'dimensions':size,'output':data,'rc':child.returncode,'child_reaped':True})
    finally:
        if child is not None and child.poll() is None:child.kill();child.wait()
        os.close(master)
        if slave>=0:os.close(slave)
assert not root.exists()
OUT['cleanup']={'fixture_tree_removed':not root.exists(),'cross_mount_tree_removed':not Path(raw2).exists(),'all_child_processes_waited':True,'no_service_operations':True}
OUT['script_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
(BASE/'results.json').write_text(json.dumps(OUT,indent=2,sort_keys=True)+'\n')
print(json.dumps(OUT,indent=2,sort_keys=True))
```

</details>
