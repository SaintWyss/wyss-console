# Decisiones vigentes

- **Teclado.** Ctrl+C copia la selección y nunca interrumpe el PTY. Ctrl+Shift+C envía ETX, el byte que el terminal interpreta como interrupción en su modo habitual. Ctrl+V pega: el texto pierde controles y marcas de pegado incrustadas, va entre marcas de pegado seguro cuando el programa lo pidió y, si no, pide confirmación ante cualquier salto de línea.
- **Motor.** `libghostty-vt` procesa el estado VT de la ventana. GTK4 dibuja las celdas. El auxiliar del host mantiene capturas de pantalla y permite recuperarse ante una discontinuidad en el búfer.
- **Temas.** CSS propio con una [tabla de tokens por tema](../src/bin/wyss-console-native/theme.rs). Ajustes ofrece Oscuro, Claro, Alto contraste y [ocho paletas de terceros](../THIRD-PARTY.md).
- **Presupuestos.** En una prueba de PTY local, la meta es ≤183 MB RSS para la ventana con un terminal, ≤5 MB marginales por terminal y ≤1,3 s para procesar 100 MB de salida. Para medir la ruta con host, registrá el costo del IPC, las resincronizaciones y el RSS de cada proceso por separado.
- **Credenciales.** SQLite y los archivos de configuración no reciben el valor en claro. Los registros se sellan con cifrado autenticado y la llave de sellado se guarda en Secret Service. La interfaz lista metadatos y huellas. Si falta la llave, devuelve un error y permite reemplazar la credencial.
- **Host.** Socket en un directorio 0700 y modo 0600; los dos extremos verifican el UID del otro. Hasta 64 conexiones, 48 de espera larga; el resto recibe `busy` y reintenta. La ventana solo arranca un `wyss-host` hermano que pase la verificación de dueño y permisos.
- **Auxiliar de pantalla.** `wyss-screen` corre con Landlock, seccomp (sin señales, ptrace ni `fcntl`/`ioctl` de dueño asíncrono) y rlimits; el host sobrevive a su muerte.
- **Sesiones de agente.** El acceso total exige un token de vista previa aleatorio, de un solo uso y con vencimiento de 2 minutos. Los homes de preset recuperan `mcpServers` y `hooks` del dueño en cada lanzamiento.
- **Sincronización de Arsenal.** Esta PC lleva un diario privado de lo que publicó; lo que llega a la carpeta compartida, aunque diga venir de esta PC, queda pendiente hasta que el dueño lo acepte, atado al contenido exacto que vio.
- **Token OAuth.** El arranque por shell usa brevemente un archivo con permisos 0600 para entregar el token.
- **Hooks del proyecto.** Claude Code carga los hooks de `.claude/settings.json` del repositorio y Wyss los deja pasar sin filtrar.
- **Alcance de la aprobación.** La huella de aprobación excluye el contenido de los scripts que invoquen los comandos.
