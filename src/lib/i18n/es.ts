/**
 * Spanish for the window's own text. The key is the English sentence exactly as it is written
 * where it is used; `tests/i18n.test.ts` fails when a sentence in `src/` has no entry here, when
 * an entry is no longer used, or when the `{name}` places of the two do not match.
 *
 * Product and provider names (Wyss Console, Arsenal, Workspace, Codex, Claude Code) are not
 * translated, and neither are the words the product uses as names of things (Inference, Local
 * Tools, Native, Managed).
 */
export const ES: Readonly<Record<string, string>> = {
  // Shell: navigation
  Terminals: 'Terminales',
  'Agents & activity': 'Agentes y actividad',
  Capabilities: 'Capacidades',
  System: 'Sistema',
  Console: 'Consola',
  'Local terminal workspace': 'Terminales locales',
  Workspace: 'Workspace',
  'Agents running in your terminals': 'Agentes que corren en tus terminales',
  Sessions: 'Sesiones',
  'Managed agent sessions': 'Sesiones de agentes Managed',
  Agents: 'Agentes',
  'Work with Codex in a session': 'Trabajar con Codex en una sesión',
  Pipelines: 'Pipelines',
  'Structured handoff and finite OL3 coordination':
    'Traspaso estructurado y coordinación OL3 acotada',
  'Remote PCs': 'PCs remotas',
  'Hosts and sessions · transport not selected':
    'Equipos y sesiones · transporte sin elegir',
  Approvals: 'Aprobaciones',
  'Actions waiting for a decision': 'Acciones que esperan una decisión',
  Events: 'Eventos',
  'What the authority recorded': 'Lo que registró la autoridad',
  Arsenal: 'Arsenal',
  'Installed capabilities': 'Capacidades instaladas',
  Configuration: 'Configuración',
  'What the agents are set up with': 'Con qué están configurados los agentes',
  Models: 'Modelos',
  'What is offered, set, selected and in use':
    'Qué se ofrece, qué está fijado, elegido y en uso',
  Profiles: 'Perfiles',
  'What a session starts with': 'Con qué arranca una sesión',
  Credentials: 'Credenciales',
  'API keys, kept sealed': 'Claves de API, guardadas selladas',
  Settings: 'Ajustes',
  'Preferences and about': 'Preferencias y acerca de',
  Runtimes: 'Runtimes',
  'One isolated runtime per managed session':
    'Un runtime aislado por cada sesión Managed',
  Providers: 'Proveedores',
  'What each provider profile can do, and why not':
    'Qué puede hacer cada perfil de proveedor, y por qué no',
  Projects: 'Proyectos',
  'Folders, workspaces and the sessions they show':
    'Carpetas, workspaces y las sesiones que muestran',
  Primary: 'Principal',
  '{count} waiting': '{count} en espera',
  'Expand the sidebar': 'Expandir la barra lateral',
  'Collapse the sidebar': 'Contraer la barra lateral',

  // Shell: top bar
  'Terminal tools': 'Herramientas de terminal',
  'Split side by side': 'Dividir lado a lado',
  'Split side by side (Ctrl+Shift+D)': 'Dividir lado a lado (Ctrl+Shift+D)',
  'Split one above the other': 'Dividir una arriba de la otra',
  'Split, one above the other (Ctrl+Shift+E)':
    'Dividir, una arriba de la otra (Ctrl+Shift+E)',
  'Keyboard shortcuts': 'Atajos de teclado',
  'Launches from your files in ~/.config/wyss-console/launch':
    'Lanzamientos de tus archivos en ~/.config/wyss-console/launch',
  Launch: 'Lanzar',

  // Shell: status
  Status: 'Estado',
  Terminal: 'Terminal',
  'Local terminal. It runs with or without the authority.':
    'Terminal local. Funciona con o sin la autoridad.',
  'Protocol 1 authority': 'Autoridad de protocolo 1',
  'Protocol 2 authority': 'Autoridad de protocolo 2',
  'Authority P1': 'Autoridad P1',
  'Authority P2': 'Autoridad P2',
  'Authority status': 'Estado de la autoridad',
  Details: 'Detalles',
  'No terminals': 'Sin terminales',
  Error: 'Error',
  Starting: 'Iniciando',
  Running: 'En ejecución',
  Closing: 'Cerrando',
  Exited: 'Terminó',
  Closed: 'Cerrada',
  Failed: 'Falló',
  Offline: 'Sin conexión',
  Reconnecting: 'Reconectando',
  Incompatible: 'Incompatible',
  Rejected: 'Rechazada',
  Unsafe: 'No segura',
  Connecting: 'Conectando',
  Unavailable: 'No disponible',
  Connected: 'Conectada',
  Syncing: 'Sincronizando',
  Stopped: 'Detenida',
  Ready: 'Listo',
  Initializing: 'Inicializando',
  Stopping: 'Deteniendo',
  Unknown: 'Desconocido',
  Installing: 'Instalando',
  'Failed to start': 'No pudo iniciar',
  'Not checked yet': 'Todavía no se revisó',

  // Shell: authority and lifecycle
  'The authority is not running. Local terminals do not depend on it.':
    'La autoridad no está corriendo. Las terminales locales no dependen de ella.',
  'The authority stopped answering. Reconnecting.':
    'La autoridad dejó de responder. Reconectando.',
  'The authority speaks another protocol or storage version.':
    'La autoridad habla otro protocolo o versión de almacenamiento.',
  'The authority broke the control protocol and was disconnected.':
    'La autoridad rompió el protocolo de control y se desconectó.',
  'The runtime directory or control socket is not private.':
    'El directorio de runtime o el socket de control no son privados.',
  'Looking for the Wyss authority.': 'Buscando la autoridad Wyss.',
  'The desktop could not read the authority state.':
    'El escritorio no pudo leer el estado de la autoridad.',
  'Reading the canonical authority state.':
    'Leyendo el estado canónico de la autoridad.',
  'The console is closing its authority link.':
    'La consola está cerrando su conexión con la autoridad.',
  '{count} session': '{count} sesión',
  '{count} sessions': '{count} sesiones',
  'Wyss authority {version}': 'Autoridad Wyss {version}',
  'Wyss authority': 'Autoridad Wyss',
  'protocol {protocol}': 'protocolo {protocol}',
  'sessions: {prepared} prepared, {closed} closed':
    'sesiones: {prepared} preparadas, {closed} cerradas',
  'event head {sequence}': 'último evento {sequence}',
  'Codex status is known only while the authority is connected.':
    'El estado de Codex solo se conoce mientras la autoridad está conectada.',
  '{name} {version}': '{name} {version}',
  '{name}': '{name}',
  'desired {state}': 'deseado {state}',
  '{count} model': '{count} modelo',
  '{count} models': '{count} modelos',
  'managed by the authority': 'administrado por la autoridad',
  'Installing the Wyss authority for this account.':
    'Instalando la autoridad Wyss para esta cuenta.',
  'Starting the Wyss authority.': 'Iniciando la autoridad Wyss.',
  'The Wyss authority is starting. Waiting for it to answer.':
    'La autoridad Wyss está iniciando. Esperando que responda.',
  'started by Wyss Console': 'iniciada por Wyss Console',
  'started outside Wyss Console': 'iniciada fuera de Wyss Console',
  'a newer authority is installed the next time it starts':
    'se instala una autoridad más nueva la próxima vez que arranque',
  'update pending': 'actualización pendiente',
  'An incompatible Wyss authority is running.':
    'Hay corriendo una autoridad Wyss incompatible.',
  'An incompatible Wyss authority is running':
    'Hay corriendo una autoridad Wyss incompatible',
  'It is this installation’s own, from another version. It is replaced the next time it starts; it is not restarted under work that may be running.':
    'Es de esta instalación, de otra versión. Se reemplaza la próxima vez que arranque; no se reinicia mientras puede haber trabajo corriendo.',
  'It was not started by this installation, so it is left exactly as it is. Wyss Console cannot work with it.':
    'No la inició esta instalación, así que se deja tal como está. Wyss Console no puede trabajar con ella.',
  'The Wyss authority is not running': 'La autoridad Wyss no está corriendo',
  'The Wyss authority could not start': 'La autoridad Wyss no pudo iniciar',
  'This build': 'Esta compilación',
  Service: 'Servicio',
  'No Wyss authority is running. Automatic authority management is disabled: opening the GUI does not install or start an authority. Start wyss-control yourself and it is used as soon as it answers.':
    'No hay ninguna autoridad Wyss corriendo. La administración automática está desactivada: abrir la GUI no instala ni inicia una autoridad. Iniciá wyss-control vos mismo y se usa en cuanto responda.',
  'The authority packaged with this application could not be trusted: it is missing, it is not the one this application was built with, or it is in a folder others can write to. Nothing was installed.':
    'No se pudo confiar en la autoridad empaquetada con esta aplicación: falta, no es la que se compiló con ella, o está en una carpeta en la que otros pueden escribir. No se instaló nada.',
  'The authority was not installed: the place it is installed in is not safely yours (a link, another owner, or a folder others can write to). Nothing was changed.':
    'La autoridad no se instaló: el lugar donde se instalaría no es seguro para vos (un enlace, otro dueño o una carpeta en la que otros pueden escribir). No se cambió nada.',
  'A service definition Wyss Console did not write is in the place of its own. It was left as it is, and the authority was not started.':
    'Hay una definición de servicio que Wyss Console no escribió en el lugar que le corresponde. Se dejó como está y la autoridad no se inició.',
  'The authority could not be installed. Nothing is half installed: what was there before is still in place.':
    'No se pudo instalar la autoridad. No quedó nada a medio instalar: lo que había antes sigue en su lugar.',
  'No user service manager answered, so the authority could not be started. Wyss Console starts its authority through the service manager of your login session.':
    'Ningún administrador de servicios de usuario respondió, así que no se pudo iniciar la autoridad. Wyss Console la inicia a través del administrador de servicios de tu sesión.',
  'The service manager did not start the authority.':
    'El administrador de servicios no inició la autoridad.',
  'The authority was started and ended before it answered.':
    'La autoridad se inició y terminó antes de responder.',
  'The authority ended repeatedly and is no longer being restarted.':
    'La autoridad terminó varias veces y ya no se reinicia.',
  'The authority is not running: it was stopped, or it ended and was not restarted.':
    'La autoridad no está corriendo: se detuvo, o terminó y no se reinició.',
  'The authority is running and did not answer in time.':
    'La autoridad está corriendo y no respondió a tiempo.',
  'The control socket of the authority is not private to you, so it is not used.':
    'El socket de control de la autoridad no es privado para vos, así que no se usa.',
  'Packaged: carries its own authority':
    'Empaquetado: trae su propia autoridad',
  'Development: uses an authority that is already running':
    'Desarrollo: usa una autoridad que ya está corriendo',
  'Packaged, but its authority cannot be found':
    'Empaquetado, pero no se encuentra su autoridad',
  Installed: 'Instalada',
  'Installed by another version': 'Instalada por otra versión',
  'A service definition that is not Wyss Console’s':
    'Una definición de servicio que no es de Wyss Console',
  'Not safely yours': 'No es de tu propiedad de forma segura',
  'No service manager answered':
    'No respondió ningún administrador de servicios',

  // Settings
  'Reading preferences': 'Leyendo preferencias',
  'Preferences unavailable': 'Preferencias no disponibles',
  'Saving…': 'Guardando…',
  'Saved on this computer': 'Guardado en esta computadora',
  'Preferences could not be read, so the defaults are shown. Nothing is saved until they can be.':
    'No se pudieron leer las preferencias, así que se muestran los valores por defecto. No se guarda nada hasta que se puedan leer.',
  'Try again': 'Reintentar',
  'Preferences change how Wyss Console looks and where it opens. They never change what Codex may do: sessions start in Inference, Local Tools is chosen for each session, and every command or file change it proposes waits for your approval.':
    'Las preferencias cambian cómo se ve Wyss Console y dónde abre. Nunca cambian lo que Codex puede hacer: las sesiones arrancan en Inference, Local Tools se elige en cada sesión, y cada comando o cambio de archivo que propone espera tu aprobación.',
  Appearance: 'Apariencia',
  'Color theme': 'Tema de color',
  Dark: 'Oscuro',
  Light: 'Claro',
  'High contrast': 'Alto contraste',
  'Changes appearance only. It does not change provider or approval permissions.':
    'Solo cambia la apariencia. No cambia permisos de proveedores ni de aprobaciones.',
  Interface: 'Interfaz',
  Language: 'Idioma',
  'The language of Wyss Console itself. What an agent, a provider or the system writes is shown as it arrives.':
    'El idioma de Wyss Console. Lo que escribe un agente, un proveedor o el sistema se muestra como llega.',
  'Open on': 'Abrir en',
  'The view shown when Wyss Console starts.':
    'La vista que se muestra cuando arranca Wyss Console.',
  'Font size': 'Tamaño de fuente',
  'History kept': 'Historial guardado',
  '{count} lines': '{count} líneas',
  'How far back each terminal can scroll. Larger values use more memory.':
    'Hasta dónde se puede volver atrás en cada terminal. Los valores más grandes usan más memoria.',
  Cursor: 'Cursor',
  Block: 'Bloque',
  Bar: 'Barra',
  Underline: 'Subrayado',
  'Blinking cursor': 'Cursor parpadeante',
  'These apply to every terminal, open or new. What a terminal runs is not a preference.':
    'Se aplican a todas las terminales, abiertas o nuevas. Lo que corre una terminal no es una preferencia.',
  Time: 'Hora',
  'Follow the system': 'Seguir al sistema',
  '12-hour': '12 horas',
  '24-hour': '24 horas',
  'Show first': 'Mostrar primero',
  'All events': 'Todos los eventos',
  Runtime: 'Runtime',
  Turns: 'Turnos',
  Security: 'Seguridad',
  'Only narrows what is shown. The authority records every event either way.':
    'Solo acota lo que se muestra. La autoridad registra todos los eventos igual.',
  'Authority · protocol 1': 'Autoridad · protocolo 1',
  Retry: 'Reintentar',
  'Retrying…': 'Reintentando…',
  'The protocol 1 authority keeps its sessions, approvals and history, and keeps running when this window is closed. Local terminals and these preferences work without it. Managed runtimes use a separate protocol 2 connection, shown in Runtimes.':
    'La autoridad de protocolo 1 conserva sus sesiones, aprobaciones e historial, y sigue corriendo cuando se cierra esta ventana. Las terminales locales y estas preferencias funcionan sin ella. Los runtimes Managed usan una conexión aparte de protocolo 2, que se ve en Runtimes.',
  About: 'Acerca de',
  'Version {version}': 'Versión {version}',
  'Not available': 'No disponible',
  Build: 'Compilación',
  Platform: 'Plataforma',
  'Authority protocol': 'Protocolo de la autoridad',
  Reset: 'Restablecer',
  'Reset all preferences to their defaults?':
    '¿Restablecer todas las preferencias a sus valores por defecto?',
  'Your sessions, history, approvals, workspaces and Codex login are not touched.':
    'No se tocan tus sesiones, historial, aprobaciones, workspaces ni el inicio de sesión de Codex.',
  'Reset preferences': 'Restablecer preferencias',
  Cancel: 'Cancelar',
  'Reset preferences…': 'Restablecer preferencias…',
  'Restores the defaults on this screen. Nothing else is deleted.':
    'Restaura los valores por defecto de esta pantalla. No se borra nada más.',
  'Your saved preferences could not be read':
    'No se pudieron leer tus preferencias guardadas',
  'They were set aside and the defaults are in use. Change a preference to save a new set, or reset to start clean.':
    'Se apartaron y se usan los valores por defecto. Cambiá una preferencia para guardar un conjunto nuevo, o restablecé para empezar de cero.',
  'Saved by a newer version of Wyss Console':
    'Guardadas por una versión más nueva de Wyss Console',
  'These preferences were left as they are and the defaults are in use. They cannot be changed from this version until they are reset.':
    'Estas preferencias se dejaron como están y se usan los valores por defecto. Desde esta versión no se pueden cambiar hasta restablecerlas.',
  'Your saved preferences were not trusted':
    'No se confió en tus preferencias guardadas',
  'The file is not private to you, so it was not read and the defaults are in use. Changing a preference replaces it.':
    'El archivo no es privado tuyo, así que no se leyó y se usan los valores por defecto. Cambiar una preferencia lo reemplaza.',
  'Preferences cannot be saved here': 'Acá no se pueden guardar preferencias',
  'The place they are kept in is not safely yours, so the defaults are in use and changes are not kept.':
    'El lugar donde se guardan no es tuyo de forma segura, así que se usan los valores por defecto y los cambios no se conservan.',
  'That value is not allowed.': 'Ese valor no está permitido.',
  'These preferences were saved by a newer version. Reset them to change them from this version.':
    'Estas preferencias las guardó una versión más nueva. Restablecelas para cambiarlas desde esta versión.',
  'The preference could not be saved. Nothing was changed.':
    'No se pudo guardar la preferencia. No cambió nada.',
  'Wyss Console is busy. Try again in a moment.':
    'Wyss Console está ocupada. Probá de nuevo en un momento.',
  Authority: 'Autoridad',
  'Not running': 'No está corriendo',
  'Codex runtime': 'Runtime de Codex',
  'Not available while offline': 'No disponible sin conexión',
  Installation: 'Instalación',
  'Managed by Wyss Console': 'Administrada por Wyss Console',
  'Started outside Wyss Console': 'Iniciada fuera de Wyss Console',
  'Installed by Wyss Console': 'Instalada por Wyss Console',
  'Not installed': 'No instalada',
  'Not known yet': 'Todavía no se sabe',
  Compatibility: 'Compatibilidad',
  'This Wyss authority is not compatible with this version of the app.':
    'Esta autoridad Wyss no es compatible con esta versión de la app.',
  Compatible: 'Compatible',
  'Checked when the authority answers':
    'Se comprueba cuando la autoridad responde',
  'Authority version': 'Versión de la autoridad',
  'Not reported': 'No informada',
  Protocol: 'Protocolo',
  Update: 'Actualización',
  'A newer authority is installed the next time it starts':
    'Se instala una autoridad más nueva la próxima vez que arranque',

  // App shell and diagnostics
  'Skip to main content': 'Saltar al contenido principal',
  'Session {id}': 'Sesión {id}',
  'Foundation ready': 'Foundation lista',
  'Application information from IPC': 'Información de la aplicación desde IPC',
  'Unable to load application information. Restart Wyss Console to try again.':
    'No se pudo cargar la información de la aplicación. Reiniciá Wyss Console para intentar de nuevo.',
  'Loading application information…': 'Cargando información de la aplicación…',
  Name: 'Nombre',
  Version: 'Versión',
  Schema: 'Esquema',
  'PTY diagnostic — Sprint 1A': 'Diagnóstico de PTY — Sprint 1A',
  'Temporary backend smoke test. No terminal renderer. Preview capped at 8,192 characters.':
    'Prueba de humo temporal del backend. Sin renderer de terminal. La vista previa se limita a 8.192 caracteres.',
  'Run PTY smoke': 'Ejecutar prueba de PTY',
  'Running PTY smoke…': 'Ejecutando prueba de PTY…',
  'PTY smoke failed; backend cleanup requested.':
    'La prueba de PTY falló; se pidió limpiar el backend.',
  'PTY smoke PASS — create, input, output, resize, close.':
    'Prueba de PTY APROBADA — crear, escribir, leer, cambiar tamaño, cerrar.',
  'No PTY created.': 'No se creó ningún PTY.',
  'Shell:': 'Shell:',
  'Cwd:': 'Directorio:',
  'PID (observed):': 'PID (observado):',
  'TerminalId:': 'Id de terminal:',
  'State:': 'Estado:',
  'bytes received:': 'bytes recibidos:',

  // Agent: the turn view and the presenter that words its states
  Agent: 'Agente',
  'Open sessions': 'Abrir sesiones',
  'Active session': 'Sesión activa',
  'Turn being started': 'Turno que se está iniciando',
  'No session is open.': 'No hay ninguna sesión abierta.',
  'Open a Ready session from Sessions to give Codex an objective in its project.':
    'Abrí una sesión en estado Listo desde Sesiones para darle a Codex un objetivo en su proyecto.',
  'Go to Sessions': 'Ir a Sesiones',
  'Close {name}': 'Cerrar {name}',
  Refresh: 'Actualizar',
  'Connecting to the authority…': 'Conectando con la autoridad…',
  'What is shown was read before the connection was lost.':
    'Lo que se muestra se leyó antes de que se cortara la conexión.',
  'Reload turns': 'Recargar turnos',
  'Loading older turns…': 'Cargando turnos anteriores…',
  'No turns in the part of the history just searched. There is more further back.':
    'No hay turnos en el tramo del historial que se acaba de buscar. Hay más hacia atrás.',
  'Keep looking': 'Seguir buscando',
  'Load older turns': 'Cargar turnos anteriores',
  '{count} older turns are shown, the most this view keeps.':
    'Se muestran {count} turnos anteriores, el máximo que guarda esta vista.',
  'Start over from the newest': 'Empezar otra vez desde el más nuevo',
  "This is the start of this session's history.":
    'Este es el comienzo del historial de esta sesión.',
  'Reading turns…': 'Leyendo turnos…',
  'No turns yet.': 'Todavía no hay turnos.',
  'Write an objective below. The authority decides whether the turn may start and what Codex is allowed to do in it.':
    'Escribí un objetivo abajo. La autoridad decide si el turno puede empezar y qué puede hacer Codex en él.',
  Turn: 'Turno',
  Accepted: 'Aceptado',
  You: 'Vos',
  'The input of this turn is not shown: Wyss keeps no conversation text.':
    'La entrada de este turno no se muestra: Wyss no guarda texto de conversación.',
  'The beginning of this output is no longer shown.':
    'Ya no se muestra el principio de esta salida.',
  'Output here was not kept by the authority.':
    'La autoridad no guardó esta salida.',
  'This turn produced no answer.': 'Este turno no produjo respuesta.',
  'The authority no longer holds the output of this turn.':
    'La autoridad ya no tiene la salida de este turno.',
  'Thinking…': 'Pensando…',
  'Waiting for approval': 'Esperando aprobación',
  'Cancelling…': 'Cancelando…',
  'Read again': 'Leer de nuevo',
  'Asking the authority to start the turn…':
    'Pidiéndole a la autoridad que inicie el turno…',
  Dismiss: 'Descartar',
  'New turn': 'Turno nuevo',
  Objective: 'Objetivo',
  'What should Codex do in this project?':
    '¿Qué debería hacer Codex en este proyecto?',
  'Enter to run · Shift+Enter for a new line':
    'Enter para ejecutar · Shift+Enter para una línea nueva',
  'Cancel turn': 'Cancelar turno',
  Run: 'Ejecutar',
  Completed: 'Completado',
  Interrupted: 'Interrumpido',
  Indeterminate: 'Indeterminado',
  'Stopped at your request.': 'Detenido a tu pedido.',
  'Its execution lease expired, so the authority stopped it.':
    'Venció su concesión de ejecución, así que la autoridad lo detuvo.',
  'Its execution lease was revoked, so the authority stopped it.':
    'Se revocó su concesión de ejecución, así que la autoridad lo detuvo.',
  'The session was closed while it ran.': 'La sesión se cerró mientras corría.',
  'Codex was stopped while it ran.': 'Codex se detuvo mientras corría.',
  'The authority shut down while it ran.':
    'La autoridad se apagó mientras corría.',
  'The authority restarted while it ran. What Codex did after that is not known.':
    'La autoridad se reinició mientras corría. No se sabe qué hizo Codex después.',
  'Codex stopped unexpectedly.': 'Codex se detuvo de forma inesperada.',
  'Codex reported that the turn failed.': 'Codex informó que el turno falló.',
  'The authority could not confirm whether Codex received the turn.':
    'La autoridad no pudo confirmar si Codex recibió el turno.',
  'Codex tried to act outside what this session allows, and the authority stopped it.':
    'Codex intentó actuar fuera de lo que permite esta sesión, y la autoridad lo detuvo.',
  'Codex did not confirm the cancel in time. What it did is not known.':
    'Codex no confirmó la cancelación a tiempo. No se sabe qué hizo.',
  'The approval it waited on expired undecided.':
    'La aprobación que esperaba venció sin decisión.',
  'Codex asked for an action this session cannot allow.':
    'Codex pidió una acción que esta sesión no puede permitir.',
  'Command proposed': 'Comando propuesto',
  'Command started': 'Comando iniciado',
  'Command output': 'Salida del comando',
  'Command finished': 'Comando terminado',
  'Command failed': 'Comando con error',
  'File change proposed': 'Cambio de archivo propuesto',
  'File change started': 'Cambio de archivo iniciado',
  'File change applied': 'Cambio de archivo aplicado',
  'File change failed': 'Cambio de archivo con error',
  'Approval requested': 'Aprobación pedida',
  Activity: 'Actividad',
  'Approval accepted': 'Aprobación aceptada',
  'Approval declined': 'Aprobación rechazada',
  'Approval cancelled': 'Aprobación cancelada',
  'Approval expired undecided': 'Aprobación vencida sin decidir',
  'Approval no longer applies': 'La aprobación ya no aplica',
  'Approval outcome unknown': 'Resultado de la aprobación desconocido',
  'Approval closed': 'Aprobación cerrada',
  'Codex asked to run a command. The authority holds the turn until that is decided, or until {until}.':
    'Codex pidió ejecutar un comando. La autoridad retiene el turno hasta que se decida, o hasta {until}.',
  'Codex asked to change files. The authority holds the turn until that is decided, or until {until}.':
    'Codex pidió cambiar archivos. La autoridad retiene el turno hasta que se decida, o hasta {until}.',
  'The authority did not answer.': 'La autoridad no respondió.',
  'This console and the authority speak different versions.':
    'Esta consola y la autoridad hablan versiones distintas.',
  'The console sent a request it should not have. Nothing changed.':
    'La consola envió un pedido que no debía enviar. No cambió nada.',
  'The authority no longer has it.': 'La autoridad ya no lo tiene.',
  'It changed after it was shown. It has been read again.':
    'Cambió después de mostrarse. Se volvió a leer.',
  'The authority refused: it is no longer allowed.':
    'La autoridad lo rechazó: ya no está permitido.',
  'The session has no workspace. Activate it in Sessions.':
    'La sesión no tiene workspace. Activala en Sesiones.',
  'The session is not attached to a running Codex. Resume it in Sessions.':
    'La sesión no está conectada a un Codex en ejecución. Reanudala en Sesiones.',
  'The authority refused to start Codex.':
    'La autoridad rechazó iniciar Codex.',
  'A turn is running. The runtime is not restarted under it.':
    'Hay un turno corriendo. El runtime no se reinicia mientras tanto.',
  'The session is attached in the other mode: its conversation exists only there.':
    'La sesión está conectada en el otro modo: su conversación existe solo ahí.',
  'The authority did not accept the workspace.':
    'La autoridad no aceptó el workspace.',
  'The session already has a workspace binding.':
    'La sesión ya tiene un workspace asociado.',
  'The authority denied it: no execution lease allows a turn for this session.':
    'La autoridad lo denegó: ninguna concesión de ejecución permite un turno en esta sesión.',
  'The authority is busy with another operation. Try again in a moment.':
    'La autoridad está ocupada con otra operación. Probá de nuevo en un momento.',
  'Codex refused it.': 'Codex lo rechazó.',
  'The authority could not confirm what Codex did. Nothing is sent again by itself.':
    'La autoridad no pudo confirmar lo que hizo Codex. No se vuelve a enviar nada por sí solo.',
  'That request was already used for something else.':
    'Ese pedido ya se usó para otra cosa.',
  'Something unexpected happened. Nothing is assumed to have changed.':
    'Pasó algo inesperado. No se asume que haya cambiado nada.',
  'The authority did not answer, so it is not known whether the turn started. Retrying repeats the same request: it cannot start a second turn.':
    'La autoridad no respondió, así que no se sabe si el turno empezó. Reintentar repite el mismo pedido: no puede iniciar un segundo turno.',
  'The input is empty or longer than a turn can carry.':
    'La entrada está vacía o es más larga de lo que puede tener un turno.',
  'The authority no longer has this session.':
    'La autoridad ya no tiene esta sesión.',
  'The session changed after it was shown. It has been read again.':
    'La sesión cambió después de mostrarse. Se volvió a leer.',
  'The session is closed, so the authority refused.':
    'La sesión está cerrada, así que la autoridad la rechazó.',
  'Another turn is running. The authority runs one turn at a time.':
    'Hay otro turno corriendo. La autoridad corre un turno a la vez.',
  'Codex refused the turn.': 'Codex rechazó el turno.',
  'The authority could not confirm whether Codex received the turn. Nothing is sent again by itself.':
    'La autoridad no pudo confirmar si Codex recibió el turno. No se vuelve a enviar nada por sí solo.',
  'That request was already used for another input.':
    'Ese pedido ya se usó para otra entrada.',
  'The turn was not started. {reason}': 'El turno no se inició. {reason}',
  'The authority did not answer, so it is not known whether the cancel was taken. The turn is shown as the authority reports it.':
    'La autoridad no respondió, así que no se sabe si la cancelación se tomó. El turno se muestra como lo informa la autoridad.',
  'The authority no longer has this turn.':
    'La autoridad ya no tiene este turno.',
  'The turn changed after it was shown. Try again.':
    'El turno cambió después de mostrarse. Probá de nuevo.',
  'The turn had already ended.': 'El turno ya había terminado.',
  'The cancel was not taken. {reason}': 'La cancelación no se tomó. {reason}',
  'This could not be read: the authority did not answer.':
    'No se pudo leer: la autoridad no respondió.',
  'This could not be read: the authority no longer has it.':
    'No se pudo leer: la autoridad ya no lo tiene.',
  'This could not be read. {reason}': 'No se pudo leer. {reason}',
  'This could not be read. Nothing is shown rather than something unverified.':
    'No se pudo leer. No se muestra nada en lugar de algo sin verificar.',
  'Older turns could not be loaded: the authority did not answer.':
    'No se pudieron cargar los turnos anteriores: la autoridad no respondió.',
  'The history changed while it was being read, so the place reached in it is no longer good. Reload the turns to start again from the newest.':
    'El historial cambió mientras se leía, así que el lugar al que se llegó ya no sirve. Recargá los turnos para empezar otra vez desde el más nuevo.',
  'Older turns could not be loaded: the authority no longer has this session.':
    'No se pudieron cargar los turnos anteriores: la autoridad ya no tiene esta sesión.',
  'Older turns could not be loaded. {reason}':
    'No se pudieron cargar los turnos anteriores. {reason}',
  'Older turns could not be loaded. Nothing is shown rather than something unverified.':
    'No se pudieron cargar los turnos anteriores. No se muestra nada en lugar de algo sin verificar.',
  'The authority is unavailable: a turn cannot be started.':
    'La autoridad no está disponible: no se puede iniciar un turno.',
  'Starting the turn…': 'Iniciando el turno…',
  'Retry or dismiss the turn that was not started.':
    'Reintentá o descartá el turno que no se inició.',
  'The session could not be read.': 'No se pudo leer la sesión.',
  'Reading the session…': 'Leyendo la sesión…',
  'This session is closed.': 'Esta sesión está cerrada.',
  'A turn is running. The authority runs one at a time.':
    'Hay un turno corriendo. La autoridad corre uno a la vez.',
  'This session has no workspace. Activate it in Sessions.':
    'Esta sesión no tiene workspace. Activala en Sesiones.',
  'This session is not attached to Codex. Resume it in Sessions.':
    'Esta sesión no está conectada a Codex. Reanudala en Sesiones.',
  'The workspace binding of this session is not settled.':
    'La asociación de workspace de esta sesión no está definida.',
  'The authority reports that a turn cannot start now.':
    'La autoridad informa que ahora no se puede iniciar un turno.',
  'The input is longer than one turn can carry.':
    'La entrada es más larga de lo que puede tener un turno.',

  // Sessions: the managed session view and the presenter that words it
  'Managed sessions': 'Sesiones Managed',
  'Connecting to the authority': 'Conectando con la autoridad',
  'Authority unavailable': 'Autoridad no disponible',
  'New session': 'Nueva sesión',
  'Activate {name}': 'Activar {name}',
  'Activate session': 'Activar sesión',
  'New managed session': 'Nueva sesión Managed',
  Project: 'Proyecto',
  'No folder selected': 'Ninguna carpeta elegida',
  Mode: 'Modo',
  'The authority decides whether the folder can be a workspace, starts Codex if it is not running, and binds the session to it. A session keeps the mode it is created in.':
    'La autoridad decide si la carpeta puede ser un workspace, arranca Codex si no está corriendo y asocia la sesión a él. Una sesión conserva el modo con el que se creó.',
  'Local Tools gives Codex more to work with, not more authority: every command and file change waits for your approval. The authority runs one Codex runtime, in one mode and for one folder at a time.':
    'Local Tools le da a Codex más con qué trabajar, no más autoridad: cada comando y cambio de archivo espera tu aprobación. La autoridad corre un solo runtime de Codex, en un modo y para una carpeta a la vez.',
  'Change folder': 'Cambiar carpeta',
  'Choose folder': 'Elegir carpeta',
  'Restart Codex & activate': 'Reiniciar Codex y activar',
  Activate: 'Activar',
  'Create & activate': 'Crear y activar',
  'The authority is unavailable': 'La autoridad no está disponible',
  'Sessions are read from the authority and cannot be created, activated, resumed or closed until it is connected. The Console keeps working.':
    'Las sesiones se leen de la autoridad y no se pueden crear, activar, reanudar ni cerrar hasta que esté conectada. La consola sigue funcionando.',
  'Reading sessions…': 'Leyendo sesiones…',
  'No managed sessions yet.': 'Todavía no hay sesiones Managed.',
  'Create a session to let Wyss supervise Codex in one of your projects.':
    'Creá una sesión para que Wyss supervise Codex en uno de tus proyectos.',
  Updated: 'Actualizada',
  'Only the first 2,000 sessions can be listed.':
    'Solo se pueden listar las primeras 2.000 sesiones.',
  'Newest sessions': 'Sesiones más nuevas',
  'Older sessions · page {page}': 'Sesiones más viejas · página {page}',
  Newer: 'Más nuevas',
  Older: 'Más viejas',
  'Session details': 'Detalles de la sesión',
  'Hide details': 'Ocultar detalles',
  'Confirm close': 'Confirmar cierre',
  'Close this session? It cannot be reopened, and its execution lease is revoked.':
    '¿Cerrar esta sesión? No se puede reabrir, y su concesión de ejecución queda revocada.',
  'Close session': 'Cerrar sesión',
  'Keep open': 'Mantenerla abierta',
  'Confirm discard': 'Confirmar descarte',
  'Discard the uncertain attempt? The authority records that it was abandoned and stops holding the session for it. If Codex did create a thread for that attempt, the empty thread is left behind. The session can then be activated again.':
    '¿Descartar el intento incierto? La autoridad registra que se abandonó y deja de retener la sesión por él. Si Codex sí creó un hilo para ese intento, el hilo vacío queda. Después la sesión se puede volver a activar.',
  'Discard attempt': 'Descartar intento',
  'Keep it': 'Conservarlo',
  'Confirm mode': 'Confirmar modo',
  'Enable Local Tools for {name}?': '¿Habilitar Local Tools para {name}?',
  'Use Inference for {name}?': '¿Usar Inference para {name}?',
  'Enable Local Tools': 'Habilitar Local Tools',
  'Use Inference': 'Usar Inference',
  'Activation outcome uncertain': 'Resultado de la activación incierto',
  'The authority asked Codex to bind a workspace and could not confirm what Codex did. Nothing asks again by itself: a second request could create a second thread.':
    'La autoridad le pidió a Codex asociar un workspace y no pudo confirmar qué hizo. Nada vuelve a pedirlo por sí solo: una segunda solicitud podría crear un segundo hilo.',
  'Open agent': 'Abrir agente',
  Resume: 'Reanudar',
  Close: 'Cerrar',
  'This session runs in Inference: Codex is offered no tool. Local Tools needs a session of its own, because a conversation stays in the mode it was created in.':
    'Esta sesión corre en Inference: a Codex no se le ofrece ninguna herramienta. Local Tools necesita una sesión propia, porque una conversación se queda en el modo en que se creó.',
  'New Local Tools session': 'Nueva sesión de Local Tools',
  'This session runs in Local Tools. Every command and file change Codex proposes waits for your approval.':
    'Esta sesión corre en Local Tools. Cada comando y cambio de archivo que propone Codex espera tu aprobación.',
  'Activate binds a project folder and attaches the session to Codex.':
    'Activar asocia una carpeta de proyecto y conecta la sesión a Codex.',
  Session: 'Sesión',
  Revision: 'Revisión',
  Created: 'Creada',
  None: 'Ninguna',
  "The authority reports a session's mode while it is attached.":
    'La autoridad informa el modo de la sesión mientras está conectada.',
  '{prepared} prepared · {closed} closed':
    '{prepared} preparadas · {closed} cerradas',
  Prepared: 'Preparada',
  'No workspace': 'Sin workspace',
  'No workspace is bound to this session.':
    'Esta sesión no tiene un workspace asociado.',
  Binding: 'Asociando',
  'The authority is binding a workspace to this session.':
    'La autoridad está asociando un workspace a esta sesión.',
  Uncertain: 'Incierto',
  'The authority could not confirm whether the workspace binding was created.':
    'La autoridad no pudo confirmar si se creó la asociación de workspace.',
  Detached: 'Desconectada',
  'Bound to a workspace and not attached to the running agent.':
    'Asociada a un workspace y no conectada al agente en ejecución.',
  'Bound to a workspace and attached to the running agent.':
    'Asociada a un workspace y conectada al agente en ejecución.',
  'Text only. Codex is offered no tool.':
    'Solo texto. No se le ofrece ninguna herramienta a Codex.',
  'Codex may propose commands and file changes in this folder. Each one waits for your approval.':
    'Codex puede proponer comandos y cambios de archivos en esta carpeta. Cada uno espera tu aprobación.',
  'Model in use: not reported by the runtime':
    'Modelo en uso: el runtime no lo informó',
  'Model in use: not known while detached':
    'Modelo en uso: no se conoce mientras está desconectada',
  ', effort {level}': ', esfuerzo {level}',
  'Model in use: {model}{effort} (the runtime said it rerouted a turn from {from}, {reason}; whether later turns stay on it is not documented)':
    'Modelo en uso: {model}{effort} (el runtime dijo que redirigió un turno desde {from}, {reason}; no está documentado si los turnos siguientes siguen en él)',
  'for a safety reason it named': 'por un motivo de seguridad que nombró',
  'for a reason this app does not know': 'por un motivo que esta app no conoce',
  'Model in use: {model}{effort} (reported by the runtime when the session was resumed)':
    'Modelo en uso: {model}{effort} (informado por el runtime cuando la sesión se reanudó)',
  'Model in use: {model}{effort} (reported by the runtime when the session was started)':
    'Modelo en uso: {model}{effort} (informado por el runtime cuando la sesión se inició)',
  'Not attached': 'Sin conectar',
  'Codex runs in Local Tools for {folder}':
    'Codex corre en Local Tools para {folder}',
  'a workspace': 'un workspace',
  'Codex runs in Inference': 'Codex corre en Inference',
  'This restarts the managed Codex runtime in Local Tools for this workspace. Sessions attached now are detached. A session created in Inference cannot be resumed in Local Tools. Every command and file change still waits for your approval.':
    'Esto reinicia el runtime de Codex administrado en Local Tools para este workspace. Las sesiones conectadas ahora quedan desconectadas. Una sesión creada en Inference no se puede reanudar en Local Tools. Cada comando y cambio de archivo sigue esperando tu aprobación.',
  'This restarts the managed Codex runtime in Inference. Sessions attached now are detached. A session created in Local Tools cannot be resumed in Inference.':
    'Esto reinicia el runtime de Codex administrado en Inference. Las sesiones conectadas ahora quedan desconectadas. Una sesión creada en Local Tools no se puede reanudar en Inference.',
  '{current} now, and the authority runs one Codex runtime. Activating this session restarts it in {to}, and the sessions attached to it are detached.':
    '{current} ahora, y la autoridad corre un solo runtime de Codex. Activar esta sesión lo reinicia en {to}, y las sesiones conectadas a él quedan desconectadas.',
  'Local Tools for this folder': 'Local Tools para esta carpeta',
  '{restart} {kept}': '{restart} {kept}',
  'A session created in {from} cannot be resumed in {to}.':
    'Una sesión creada en {from} no se puede reanudar en {to}.',
  'Resuming session…': 'Reanudando la sesión…',
  'Closing session…': 'Cerrando la sesión…',
  'Discarding the uncertain attempt…': 'Descartando el intento incierto…',
  'Preparing session…': 'Preparando la sesión…',
  'Stopping the managed runtime…': 'Deteniendo el runtime administrado…',
  'Starting Codex…': 'Iniciando Codex…',
  'Creating workspace binding…': 'Creando la asociación de workspace…',
  'Attaching…': 'Conectando…',
  'Starting Codex in {mode}…': 'Iniciando Codex en {mode}…',
  'The session is not ready': 'La sesión no está lista',
  'The session is not ready in that mode':
    'La sesión no está lista en ese modo',
  'The session was not resumed': 'La sesión no se reanudó',
  'The session was not closed': 'La sesión no se cerró',
  'The attempt was not discarded': 'El intento no se descartó',
  'The authority did not answer, so the result is unknown. Retrying repeats the same request safely.':
    'La autoridad no respondió, así que el resultado es desconocido. Reintentar repite la misma solicitud sin riesgo.',
  'This session has no workspace binding to resume.':
    'Esta sesión no tiene una asociación de workspace para reanudar.',
  'The authority denied it because the agent runtime is not ready.':
    'La autoridad lo denegó porque el runtime del agente no está listo.',
  'A turn is running, and the runtime it runs on is not restarted under it. Wait for it or cancel it.':
    'Hay un turno en curso y el runtime en el que corre no se reinicia por debajo. Esperá a que termine o cancelalo.',
  'This session is attached in the other mode: its conversation exists only there.':
    'Esta sesión está conectada en el otro modo: su conversación existe solo ahí.',
  'The authority did not accept that folder as a workspace. Nothing was bound.':
    'La autoridad no aceptó esa carpeta como workspace. No se asoció nada.',
  'The session already has a workspace binding. It has been read again.':
    'La sesión ya tiene una asociación de workspace. Se volvió a leer.',
  'The authority denied it: no execution lease allows this for the session.':
    'La autoridad lo denegó: ninguna concesión de ejecución permite esto para la sesión.',
  'The agent refused to resume this session.':
    'El agente rechazó reanudar esta sesión.',
  'The authority could not confirm what the agent did. Check the session before trying again.':
    'La autoridad no pudo confirmar qué hizo el agente. Revisá la sesión antes de volver a intentarlo.',
  'That request was already used for something else. Try again.':
    'Esa solicitud ya se usó para otra cosa. Probá de nuevo.',
  'The authority refused: the session no longer allows it.':
    'La autoridad rechazó: la sesión ya no lo permite.',
  'Codex is not ready, so the session could not be bound or attached.':
    'Codex no está listo, así que la sesión no se pudo asociar ni conectar.',
  'Codex refused the request.': 'Codex rechazó la solicitud.',
  'Codex did not come up in that mode. The runtime is as the authority reports it.':
    'Codex no arrancó en ese modo. El runtime queda como lo informa la autoridad.',
  'Codex refused to resume this session in that mode. A session resumes only in the mode it was created in. The runtime now runs in the mode that was asked for.':
    'Codex rechazó reanudar esta sesión en ese modo. Una sesión solo se reanuda en el modo en que se creó. El runtime ahora corre en el modo que se pidió.',
  'This session has no workspace to run a mode for.':
    'Esta sesión no tiene un workspace en el que correr un modo.',
  'Codex refused to resume this session. A session resumes only in the mode it was created in: if Codex runs in the other one now, change the mode for this session instead.':
    'Codex rechazó reanudar esta sesión. Una sesión solo se reanuda en el modo en que se creó: si Codex corre ahora en el otro, cambiá el modo de esta sesión.',
  'The authority holds no uncertain attempt for this session any more.':
    'La autoridad ya no tiene ningún intento incierto para esta sesión.',
  '{action}. The authority refused to start Codex in Local Tools: it could not set up or prove the isolation that mode requires. Nothing runs without it.':
    '{action}. La autoridad rechazó iniciar Codex en Local Tools: no pudo preparar ni comprobar el aislamiento que ese modo exige. Nada corre sin él.',
  'Activation outcome uncertain: the authority could not confirm whether Codex created the workspace binding. Nothing is sent again by itself.':
    'Resultado de la activación incierto: la autoridad no pudo confirmar si Codex creó la asociación de workspace. Nada se vuelve a enviar por sí solo.',
  '{action}. {reason}': '{action}. {reason}',
  'Sessions could not be read: the authority did not answer.':
    'No se pudieron leer las sesiones: la autoridad no respondió.',
  'Sessions could not be read. {reason}':
    'No se pudieron leer las sesiones. {reason}',
  'Sessions could not be read. Nothing is shown rather than something unverified.':
    'No se pudieron leer las sesiones. No se muestra nada en lugar de algo no verificado.',
  'That folder cannot be used: its path is too long or cannot be written as text.':
    'Esa carpeta no se puede usar: su ruta es demasiado larga o no se puede escribir como texto.',
  'The folder dialog is already open.':
    'El diálogo de carpetas ya está abierto.',
  'The folder dialog could not be opened.':
    'No se pudo abrir el diálogo de carpetas.',
  'A closed session cannot be resumed.':
    'Una sesión cerrada no se puede reanudar.',
  'Ready: attached to the running agent.':
    'Lista: conectada al agente en ejecución.',
  'Resume needs a workspace bound to the session.':
    'Reanudar necesita un workspace asociado a la sesión.',
  'Resume has to wait until the workspace binding is settled.':
    'Reanudar tiene que esperar a que la asociación de workspace esté definida.',
  'The agent runtime is not ready, so there is nothing to attach to.':
    'El runtime del agente no está listo, así que no hay a qué conectarse.',

  // Models: what is offered, configured, selected and reported in use
  'Four different things, kept apart: what a runtime':
    'Cuatro cosas distintas, separadas: lo que un runtime',
  offers: 'ofrece',
  ', what an agent’s files are': ', lo que los archivos de un agente tienen',
  configured: 'configurado',
  'with, what a profile': 'con, lo que un perfil',
  selects: 'selecciona',
  'for a session it starts, and what a runtime':
    'para una sesión que arranca, y lo que un runtime',
  'reported in use': 'informó en uso',
  '. None is filled from another. A default in a list, a value in a file or a name passed at start says what was asked for, never what answered.':
    '. Ninguna se completa con otra. Un valor por defecto en una lista, un valor en un archivo o un nombre pasado al arrancar dice lo que se pidió, nunca lo que respondió.',
  'Offered by the managed Codex runtime':
    'Ofrecidos por el runtime de Codex administrado',
  'Reading the runtime’s list…': 'Leyendo la lista del runtime…',
  'The last read failed. What is shown is older.':
    'La última lectura falló. Lo que se muestra es anterior.',
  'The runtime listed no model.': 'El runtime no listó ningún modelo.',
  'Models the runtime offers': 'Modelos que ofrece el runtime',
  'default of this runtime': 'predeterminado de este runtime',
  'Provider {provider}': 'Proveedor {provider}',
  'Efforts:': 'Esfuerzos:',
  'not reported by this authority version':
    'no informado por esta versión de la autoridad',
  'none listed': 'ninguno listado',
  '{effort} (default)': '{effort} (predeterminado)',
  '{effort}': '{effort}',
  'Configured in the agents’ own files':
    'Configurados en los archivos propios de los agentes',
  'Not read yet. The Configuration view reads these files.':
    'Todavía no se leyó. La vista de Configuración lee estos archivos.',
  'No model setting was found in the places read.':
    'No se encontró ninguna configuración de modelo en los lugares leídos.',
  'Configured models': 'Modelos configurados',
  'not shown here': 'no se muestra acá',
  'As the file says; it applies to new sessions, and what a running session does with a change is not known.':
    'Tal como dice el archivo; se aplica a las sesiones nuevas, y no se sabe qué hace una sesión en curso con un cambio.',
  'Selected by a profile': 'Seleccionados por un perfil',
  'No profile names a model or an effort.':
    'Ningún perfil nombra un modelo ni un esfuerzo.',
  'Models selected by profiles': 'Modelos seleccionados por perfiles',
  '(template)': '(plantilla)',
  'Model:': 'Modelo:',
  'Effort:': 'Esfuerzo:',
  'inherited from {from}': 'heredado de {from}',
  'the agent’s own choice': 'la elección propia del agente',
  'Reported in use': 'Informados en uso',
  'Managed sessions have not been read yet.':
    'Todavía no se leyeron las sesiones Managed.',
  'No managed session is attached.': 'No hay ninguna sesión Managed conectada.',
  'Models reported by the runtime': 'Modelos informados por el runtime',
  'The list could not be read: the authority did not answer.':
    'No se pudo leer la lista: la autoridad no respondió.',
  'The list could not be read: the authority speaks another version than this app.':
    'No se pudo leer la lista: la autoridad habla otra versión que esta app.',
  'The list could not be read: the authority is busy. Try again.':
    'No se pudo leer la lista: la autoridad está ocupada. Probá de nuevo.',
  'The list could not be read. Nothing is shown rather than something unverified.':
    'No se pudo leer la lista. No se muestra nada en lugar de algo no verificado.',
  'This is the runtime’s own list. A model being listed does not mean your account can use it: that is not known here.':
    'Esta es la lista propia del runtime. Que un modelo aparezca en la lista no quiere decir que tu cuenta pueda usarlo: eso no se sabe acá.',
  'The managed Codex runtime is not running, so there is no list. A list of a runtime that has stopped is never shown. Start a managed session and it is listed again.':
    'El runtime de Codex administrado no está corriendo, así que no hay lista. Nunca se muestra la lista de un runtime que se detuvo. Iniciá una sesión Managed y la lista vuelve a aparecer.',
  'Listed once by the managed Codex runtime (version {version}) when it started, in {mode}. Read from the authority at {at}.':
    'Listado una vez por el runtime de Codex administrado (versión {version}) al arrancar, en {mode}. Leído de la autoridad el {at}.',
  'Listed once by the managed Codex runtime when it started, in {mode}. Read from the authority at {at}.':
    'Listado una vez por el runtime de Codex administrado al arrancar, en {mode}. Leído de la autoridad el {at}.',
  'The runtime’s list continued past what was taken: this list is partial.':
    'La lista del runtime siguió más allá de lo que se tomó: esta lista está incompleta.',
  'Whether this list is complete is not reported by this authority version.':
    'Esta versión de la autoridad no informa si la lista está completa.',
  'The runtime marks it for replacement by {by}, retiring {retires}.':
    'El runtime marca que lo reemplaza {by}, y se retira el {retires}.',
  'The runtime marks it for replacement by {by}.':
    'El runtime marca que lo reemplaza {by}.',
  'A profile passes these names to the agent when a session starts. Wyss Console cannot check them: the agent says so in its own session if it does not know one. Nothing changes the agent’s own files.':
    'Un perfil pasa estos nombres al agente cuando arranca una sesión. Wyss Console no puede comprobarlos: si el agente no conoce uno, lo dice en su propia sesión. Nada cambia los archivos propios del agente.',
  'Agents you start in a terminal do not report the model they use, and this app adds no hook and reads no terminal to find out. What you see for them is what was asked for and what their files say, never what is in use.':
    'Los agentes que arrancás en una terminal no informan el modelo que usan, y esta app no agrega ningún hook ni lee la terminal para saberlo. Lo que se ve de ellos es lo que se pidió y lo que dicen sus archivos, nunca lo que está en uso.',

  // Credentials: the API keys the desktop keeps sealed
  'API credentials': 'Credenciales de API',
  'API keys for the AI services, kept sealed on this computer (the key that seals them is in your Secret Service). A key is typed in a window of its own and is never shown again, never sent to this page and never written unsealed. Only a name, the service and a short checksum are listed. Nothing here starts an agent with a key.':
    'Claves de API de los servicios de IA, guardadas selladas en esta computadora (la clave que las sella está en tu Secret Service). Una clave se escribe en una ventana propia y nunca se vuelve a mostrar, nunca se envía a esta página y nunca se escribe sin sellar. Solo se listan un nombre, el servicio y un checksum corto. Acá nada arranca un agente con una clave.',
  'The name must be 1 to 60 characters, without control characters.':
    'El nombre tiene que tener de 1 a 60 caracteres, sin caracteres de control.',
  'The key may not be empty, hold spaces or line breaks, or be longer than 4096 characters.':
    'La clave no puede estar vacía, tener espacios o saltos de línea, ni superar los 4096 caracteres.',
  'That key is not kept any more.': 'Esa clave ya no está guardada.',
  'There are too many keys (100).': 'Hay demasiadas claves (100).',
  'No key store (the Secret Service) is available, so nothing was kept. A key is never kept unsealed.':
    'No hay un almacén de claves (el Secret Service) disponible, así que no se guardó nada. Una clave nunca se guarda sin sellar.',
  'The key that sealed this record is gone from the key store. Replace the key.':
    'La clave que selló este registro ya no está en el almacén de claves. Reemplazá la clave.',
  'The record is damaged. Replace the key.':
    'El registro está dañado. Reemplazá la clave.',
  'The Console could not write its own state.':
    'La consola no pudo escribir su propio estado.',
  'Something went wrong in the desktop.': 'Algo salió mal en el escritorio.',
  Other: 'Otro',
  'Add a key': 'Agregar una clave',
  'Enter the key…': 'Ingresar la clave…',
  'Reading…': 'Leyendo…',
  'No keys yet': 'Todavía no hay claves',
  'Add one above. Its value is asked for in a separate window.':
    'Agregá una arriba. Su valor se pide en una ventana aparte.',
  Keys: 'Claves',
  'New name': 'Nombre nuevo',
  checksum: 'checksum',
  'kept {date}': 'guardada el {date}',
  'changed {date}': 'cambiada el {date}',
  'Save name': 'Guardar nombre',
  'Replace key…': 'Reemplazar clave…',
  Rename: 'Renombrar',
  Check: 'Verificar',
  'Delete…': 'Eliminar…',
  'Key kept, sealed.': 'Clave guardada, sellada.',
  'Key replaced.': 'Clave reemplazada.',
  'Renamed.': 'Renombrada.',
  'Key deleted.': 'Clave eliminada.',
  'The key opens and matches its checksum.':
    'La clave se abre y coincide con su checksum.',
  'The record is damaged: replace the key.':
    'El registro está dañado: reemplazá la clave.',

  // Remote: the view that says what is not connected yet
  'M7 · REMOTE GATEWAY': 'M7 · PUERTA REMOTA',
  'Unavailable · OD01 transport choice is open. This view sends no network requests and has no paired hosts.':
    'No disponible · la elección de transporte OD01 está abierta. Esta vista no envía pedidos de red y no tiene equipos emparejados.',
  'Remote areas': 'Áreas remotas',
  Hosts: 'Equipos',
  Pairing: 'Emparejamiento',
  Devices: 'Dispositivos',
  'No remote host is enrolled. Authentication, pairing, connection type and grant expiry are unavailable until a transport is approved and independently reviewed.':
    'No hay ningún equipo remoto registrado. La autenticación, el emparejamiento, el tipo de conexión y el vencimiento de las concesiones no están disponibles hasta que un transporte se apruebe y lo revise alguien independiente.',
  'Connect to a host': 'Conectar con un equipo',
  'Pairing requires two distinct device keys, a pinned host identity, QR transcript proof and explicit host confirmation. No key or enrollment is created from this view.':
    'El emparejamiento requiere dos claves de dispositivo distintas, una identidad de host fijada, la prueba de la transcripción del QR y la confirmación explícita del host. Desde esta vista no se crea ninguna clave ni registro.',
  'Begin pairing': 'Empezar el emparejamiento',
  'Remote projects and workspaces will be listed only from an authenticated host. No local path or cached project is shown here.':
    'Los proyectos y workspaces remotos se listarán solo desde un host autenticado. Acá no se muestra ninguna ruta local ni proyecto en caché.',
  'Native and Managed sessions need host identity, plane, generation and observation age. No remote session metadata is cached.':
    'Las sesiones Native y Managed necesitan identidad de host, plano, generación y antigüedad de la observación. No se guarda en caché ningún metadato de sesiones remotas.',
  'Disconnected · no output is available. View and input grants are separate; terminal.write grants full host-account control. Locked-host full-account input remains blocked.':
    'Desconectado · no hay salida disponible. Las concesiones de ver y de escribir son separadas; terminal.write da control total de la cuenta del host. La entrada con cuenta completa en un host bloqueado sigue bloqueada.',
  'Input must report accepted bytes, partial writes or OUTCOME_UNKNOWN. A lost acknowledgement is never replayed; new user input is required after reconnect.':
    'La entrada tiene que informar bytes aceptados, escrituras parciales o OUTCOME_UNKNOWN. Una confirmación perdida nunca se reenvía: después de reconectar hace falta una entrada nueva del usuario.',
  'Attach read-only terminal': 'Adjuntar terminal de solo lectura',
  'Claim terminal input': 'Reclamar la entrada de la terminal',
  'Pipeline metadata is unavailable offline. M6 keeps prompts and provider output out of durable journals; a remote view will fetch through the reviewed host protocol.':
    'Los metadatos de los pipelines no están disponibles sin conexión. M6 mantiene los prompts y la salida de los proveedores fuera de los diarios durables; una vista remota va a traer datos por el protocolo de host revisado.',
  'Managed approval decisions must pass through the existing wyss-control authority and its per-operation presence check. This view cannot approve while disconnected.':
    'Las decisiones de aprobación Managed tienen que pasar por la autoridad wyss-control existente y su verificación de presencia por operación. Esta vista no puede aprobar mientras esté desconectada.',
  Approve: 'Aprobar',
  Deny: 'Rechazar',
  'No paired-device list exists. Revocation, expiry and host-confirmed grant changes will be shown only after authenticated host negotiation.':
    'No existe una lista de dispositivos emparejados. Las revocaciones, los vencimientos y los cambios de concesión confirmados por el host se mostrarán solo después de una negociación autenticada con el host.',
  'Transport candidates': 'Candidatos de transporte',
  'H1 · owner-configured overlay: Android VPN lifecycle and third-party control plane; Tailscale reference pricing is $0 Personal (up to 6 users), $8/user/month Standard, $18/user/month Premium.':
    'H1 · overlay configurado por el dueño: ciclo de vida de la VPN de Android y plano de control de terceros; el precio de referencia de Tailscale es $0 Personal (hasta 6 usuarios), $8/usuario/mes Standard, $18/usuario/mes Premium.',
  'H2 · iroh/QUIC: first-party Kotlin/Android bindings and end-to-end TLS; public relays are free/rate-limited/no-SLA, hosted relays start at $19/month, self-hosting transfers infrastructure and operations to Wyss.':
    'H2 · iroh/QUIC: bindings propios de Kotlin/Android y TLS de extremo a extremo; los relays públicos son gratuitos, con límite de tasa y sin SLA; los relays alojados arrancan en $19/mes; alojarlo uno mismo pasa la infraestructura y la operación a Wyss.',
  'No CPU, RAM, battery, idle, latency or roaming measurements exist for either candidate. OD01 remains an owner decision. OD02 keeps locked-host full-account input blocked.':
    'No hay mediciones de CPU, RAM, batería, reposo, latencia ni roaming para ninguno de los dos candidatos. OD01 sigue siendo una decisión del dueño. OD02 mantiene bloqueada la entrada con cuenta completa en un host bloqueado.',
  'Disconnected · no action is queued or sent.':
    'Desconectado · no se encola ni se envía ninguna acción.',
  // Configuration: the items view, its changes and history, and the sentences of its presenter
  item: 'elemento',
  items: 'elementos',
  from: 'de',
  places: 'lugares',
  Reading: 'Leyendo',
  'This is what Claude Code and Codex are set up with, read from their own files. Nothing changes until you confirm a plan, and Claude Code hooks and skills, Codex skills and MCP servers, and OpenCode MCP servers and plugins can be switched here. You can also write a Claude Code hook or an MCP server for Codex or OpenCode, in a separate window. Secrets in commands, environments and headers are never shown.':
    'Esto es lo que tienen configurado Claude Code y Codex, leído de sus propios archivos. No cambia nada hasta que confirmes un plan, y los hooks y skills de Claude Code, las skills y los servidores MCP de Codex, y los servidores MCP y plugins de OpenCode se pueden activar o desactivar desde acá. También podés escribir un hook de Claude Code o un servidor MCP para Codex u OpenCode, en una ventana aparte. Los secretos de comandos, entornos y encabezados nunca se muestran.',
  'Change to confirm': 'Cambio para confirmar',
  'What would change': 'Qué cambiaría',
  'Another account you named in': 'Otra cuenta que nombraste en',
  'can also write this file’s folder. It is accepted because you named it.':
    'también puede escribir la carpeta de este archivo. Se acepta porque la nombraste.',
  'This entry holds values that are not shown here.':
    'Esta entrada tiene valores que no se muestran acá.',
  'Prediction, not a reading: managed Inference':
    'Predicción, no una lectura: Inference administrado',
  now: 'ahora',
  'and this change does not alter that.': 'y este cambio no lo altera.',
  'and would': 'y',
  'after it.': 'después de este cambio.',
  'Waiting for the confirmation window…':
    'Esperando la ventana de confirmación…',
  'Applying…': 'Aplicando…',
  'Continue to confirm': 'Seguir para confirmar',
  Apply: 'Aplicar',
  'Preparing the change…': 'Preparando el cambio…',
  'This could not be read again just now. What is shown is from the last successful read.':
    'No se pudo volver a leer ahora mismo. Lo que se muestra es de la última lectura exitosa.',
  'There are more items than this app reads at once. Some are not listed.':
    'Hay más elementos de los que esta app lee de una vez. Algunos no aparecen.',
  'Reading configuration…': 'Leyendo configuración…',
  'There is no configuration to read here':
    'Acá no hay configuración para leer',
  'The app could not find where your account keeps its files. Nothing is shown, which does not mean there is nothing configured.':
    'La app no encontró dónde guarda tu cuenta sus archivos. No se muestra nada, lo que no quiere decir que no haya nada configurado.',
  'The configuration could not be read': 'No se pudo leer la configuración',
  View: 'Vista',
  Items: 'Elementos',
  'Turned off': 'Desactivados',
  History: 'Historial',
  'Add a hook…': 'Agregar un hook…',
  'Add an MCP server…': 'Agregar un servidor MCP…',
  'New instruction file…': 'Nuevo archivo de instrucciones…',
  'Choose the project whose instruction files and skills are listed and may be changed.':
    'Elegí el proyecto cuyos archivos de instrucciones y skills se listan y se pueden cambiar.',
  'Choose another project.': 'Elegí otro proyecto.',
  'Choose project…': 'Elegir proyecto…',
  'Project: {project}': 'Proyecto: {project}',
  'No project': 'Sin proyecto',
  'Add a hook': 'Agregar un hook',
  'New instruction file': 'Nuevo archivo de instrucciones',
  'Add an MCP server': 'Agregar un servidor MCP',
  'Choose when it runs. The command is typed next, in a separate window.':
    'Elegí cuándo corre. El comando se escribe después, en una ventana aparte.',
  Event: 'Evento',
  'Matcher (optional)': 'Coincidencia (opcional)',
  'Start from a sealed template (asks for the file and its passphrase; what it holds is shown for review before anything is planned)':
    'Empezar desde una plantilla sellada (pide el archivo y su frase de paso; lo que tiene se muestra para revisar antes de planear algo)',
  'Creates a file that does not exist yet. The name (where one is needed) and the text are typed next, in a separate window; nothing is written until you review the plan and confirm it separately. Undo removes it again.':
    'Crea un archivo que todavía no existe. El nombre (cuando hace falta) y el texto se escriben después, en una ventana aparte; no se escribe nada hasta que revises el plan y lo confirmes por separado. Deshacer lo borra de nuevo.',
  Where: 'Dónde',
  'Project “{project}”': 'Proyecto “{project}”',
  'What to create': 'Qué crear',
  'Start from a sealed template (asks for the file and its passphrase)':
    'Empezar desde una plantilla sellada (pide el archivo y su frase de paso)',
  'Name the server and say what it is. What it starts or connects to is typed next, in a separate window.':
    'Poné nombre al servidor y decí qué es. Lo que arranca o a lo que se conecta se escribe después, en una ventana aparte.',
  Kind: 'Tipo',
  'Starts a program': 'Arranca un programa',
  'Connects to an address': 'Se conecta a una dirección',
  'Continue…': 'Continuar…',
  'Reading the history…': 'Leyendo el historial…',
  'No changes yet': 'Todavía no hay cambios',
  'Changes you make here are listed, without their content.':
    'Los cambios que hagas acá se listan, sin su contenido.',
  Changes: 'Cambios',
  '· an undo': '· un deshacer',
  'Not finished': 'No terminado',
  Undo: 'Deshacer',
  'Older changes': 'Cambios anteriores',
  'Import a kept hook…': 'Importar un hook guardado…',
  'Nothing is turned off': 'No hay nada desactivado',
  'A hook you turn off is removed from its file and kept here, so it can be put back exactly.':
    'Un hook que desactivás se quita de su archivo y queda guardado acá, así se puede restaurar tal cual.',
  'Turned-off hooks': 'Hooks desactivados',
  Kept: 'Guardado el',
  encrypted: 'cifrado',
  'not encrypted (no secret in it)': 'sin cifrar (no tiene secretos)',
  '· holds hidden values': '· tiene valores ocultos',
  '· kept past the usual time; it stays until you decide':
    '· guardado más tiempo del habitual; queda hasta que decidas',
  'Put back': 'Restaurar',
  'Export…': 'Exportar…',
  Forget: 'Olvidar',
  'Filter by agent': 'Filtrar por agente',
  'All agents': 'Todos los agentes',
  'Filter by kind': 'Filtrar por tipo',
  Everything: 'Todo',
  'Search by name': 'Buscar por nombre',
  'Found in': 'Encontrado en',
  'In the file': 'En el archivo',
  'In use': 'En uso',
  'Changing it': 'Cambiarlo',
  'You can do it here, with a plan you confirm.':
    'Podés hacerlo acá, con un plan que confirmás.',
  'You can change it here: you review the text in a separate window, then confirm a plan.':
    'Podés cambiarlo acá: revisás el texto en una ventana aparte y después confirmás un plan.',
  Fingerprint: 'Huella',
  'Change it…': 'Cambiar…',
  'Export template…': 'Exportar plantilla…',
  'Apply template…': 'Aplicar plantilla…',
  value: 'valor',
  'Nothing to show': 'Nada para mostrar',
  'No hooks, skills, servers, rules or model settings were found in the places that were read.':
    'No se encontraron hooks, skills, servidores, reglas ni ajustes de modelo en los lugares que se leyeron.',
  'Nothing matches this filter.': 'Nada coincide con este filtro.',
  'Configuration items': 'Elementos de configuración',
  '· values hidden': '· valores ocultos',
  'Where this was read from': 'De dónde se leyó',
  problem: 'problema',
  problems: 'problemas',
  'Its folder is shared with a group, so it is only read.':
    'Su carpeta es compartida con un grupo, así que solo se lee.',
  Hooks: 'Hooks',
  Skills: 'Skills',
  'MCP servers': 'Servidores MCP',
  'Rule files': 'Archivos de reglas',
  Plugins: 'Plugins',
  Hook: 'Hook',
  Skill: 'Skill',
  'MCP server': 'Servidor MCP',
  'Rule file': 'Archivo de reglas',
  'Model setting': 'Ajuste de modelo',
  Plugin: 'Plugin',
  'On in the file': 'Activado en el archivo',
  'Off in the file': 'Desactivado en el archivo',
  'Not in the file': 'No está en el archivo',
  Parked: 'Apartado',
  'Could not be read': 'No se pudo leer',
  'Confirmed active': 'Activo confirmado',
  'Confirmed inactive': 'Inactivo confirmado',
  'As the provider documents it': 'Según documenta el proveedor',
  'Cannot be confirmed without running the provider':
    'No se puede confirmar sin ejecutar el proveedor',
  'Your account': 'Tu cuenta',
  'Managed settings': 'Ajustes administrados',
  'User settings': 'Ajustes del usuario',
  'Account file (MCP servers only)':
    'Archivo de la cuenta (solo servidores MCP)',
  'User skills folder': 'Carpeta de skills del usuario',
  'User rules': 'Reglas del usuario',
  'Project settings': 'Ajustes del proyecto',
  'Project local settings': 'Ajustes locales del proyecto',
  'Project MCP file': 'Archivo MCP del proyecto',
  'Project skills folder': 'Carpeta de skills del proyecto',
  'Project rules': 'Reglas del proyecto',
  'Administrator requirements': 'Requisitos del administrador',
  'Managed configuration': 'Configuración administrada',
  'User configuration': 'Configuración del usuario',
  'User hooks file': 'Archivo de hooks del usuario',
  'User instructions': 'Instrucciones del usuario',
  'User skills folder (~/.agents/skills)':
    'Carpeta de skills del usuario (~/.agents/skills)',
  'Project configuration': 'Configuración del proyecto',
  'Project instructions': 'Instrucciones del proyecto',
  'Set by the system; this app never changes it':
    'Lo define el sistema; esta app nunca lo cambia',
  'Owned by the provider; changed only through its own command':
    'Es del proveedor; solo se cambia con su propio comando',
  'Produced by another tool': 'Lo genera otra herramienta',
  'Its folder can be written by a group: read, never changed':
    'Su carpeta la puede escribir un grupo: solo se lee, nunca se cambia',
  'Read-only for now: changing it is not available yet':
    'Solo lectura por ahora: cambiarlo todavía no está disponible',
  'environment value': 'valor de entorno',
  header: 'encabezado',
  'address credentials': 'credenciales de la dirección',
  'secret field': 'campo secreto',
  token: 'token',
  'It is not there.': 'No está.',
  'It could not be read.': 'No se pudo leer.',
  'It kept changing while it was being read. It will be read again.':
    'Cambió mientras se leía. Se va a leer de nuevo.',
  'It holds credentials, so it is never opened.':
    'Tiene credenciales, así que nunca se abre.',
  'Where it leads is outside the places this app may read.':
    'A donde apunta está fuera de los lugares que esta app puede leer.',
  'Its place is not safe to read from: someone else owns or can change it.':
    'Su lugar no es seguro para leer: otro lo tiene o lo puede cambiar.',
  'It is behind too many links.': 'Está detrás de demasiados enlaces.',
  'It is not a regular file.': 'No es un archivo regular.',
  'It is larger than the bounds this app reads.':
    'Es más grande de lo que esta app lee.',
  'It is not valid: reading stopped at line {line}, column {column}.':
    'No es válido: la lectura se detuvo en la línea {line}, columna {column}.',
  'Read. A conflict copy of this file sits beside it, so it cannot be changed from here until that copy is dealt with.':
    'Leído. Hay una copia en conflicto de este archivo al lado, así que no se puede cambiar desde acá hasta resolver esa copia.',
  'Read. {entries}': 'Leído. {entries}',
  '{count} entry was not recognised and left untouched.':
    '{count} entrada no se reconoció y quedó sin tocar.',
  '{count} entries were not recognised and left untouched.':
    '{count} entradas no se reconocieron y quedaron sin tocar.',
  'Read.': 'Leído.',
  'Not present.': 'No está presente.',
  'Not read: Codex loads a project’s own configuration only for a project you trust.':
    'No se leyó: Codex carga la configuración propia de un proyecto solo para un proyecto de confianza.',
  empty: 'vacío',
  none: 'ninguno',
  'hidden ({kind})': 'oculto ({kind})',
  'Change this instruction file': 'Cambiar este archivo de instrucciones',
  'Create this instruction file': 'Crear este archivo de instrucciones',
  'Delete this instruction file (kept encrypted, so it can be put back)':
    'Borrar este archivo de instrucciones (queda guardado cifrado, para poder restaurarlo)',
  'Turn off this hook': 'Desactivar este hook',
  'Put this hook back': 'Volver a poner este hook',
  'Turn off every hook': 'Desactivar todos los hooks',
  'Allow hooks again': 'Volver a permitir los hooks',
  'Turn off this skill': 'Desactivar esta skill',
  'Turn on this skill': 'Activar esta skill',
  'Turn off this Codex skill': 'Desactivar esta skill de Codex',
  'Turn on this Codex skill': 'Activar esta skill de Codex',
  'Turn off this Codex MCP server': 'Desactivar este servidor MCP de Codex',
  'Turn on this Codex MCP server': 'Activar este servidor MCP de Codex',
  'Turn off this OpenCode MCP server':
    'Desactivar este servidor MCP de OpenCode',
  'Turn on this OpenCode MCP server': 'Activar este servidor MCP de OpenCode',
  'Turn off this OpenCode plugin': 'Desactivar este plugin de OpenCode',
  'Add this hook': 'Agregar este hook',
  'Change this hook’s command': 'Cambiar el comando de este hook',
  'Add this OpenCode MCP server': 'Agregar este servidor MCP de OpenCode',
  'Change this OpenCode MCP server': 'Cambiar este servidor MCP de OpenCode',
  'Delete this OpenCode MCP server (kept, so it can be put back)':
    'Borrar este servidor MCP de OpenCode (queda guardado, para poder restaurarlo)',
  'Take back what you wrote': 'Quitar lo que escribiste',
  'Add this Codex MCP server': 'Agregar este servidor MCP de Codex',
  'Change this Codex MCP server': 'Cambiar este servidor MCP de Codex',
  'Delete this Codex MCP server (kept, so it can be put back)':
    'Borrar este servidor MCP de Codex (queda guardado, para poder restaurarlo)',
  'This can switch off something that stops an action. You will be asked to confirm in a separate window.':
    'Esto puede desactivar algo que frena una acción. Te van a pedir confirmar en una ventana aparte.',
  'This writes, or puts back, something that runs programs. You will be asked to confirm in a separate window.':
    'Esto escribe, o vuelve a poner, algo que ejecuta programas. Te van a pedir confirmar en una ventana aparte.',
  'Written and read back. Reload and use by a running provider are not observed.':
    'Escrito y releído. No se observa la recarga ni el uso por un proveedor en ejecución.',
  'A new Claude Code session follows the file (checked). A session that is already running is documented to pick it up; that has not been checked.':
    'Una sesión nueva de Claude Code sigue el archivo (verificado). Una sesión que ya está corriendo, según la documentación, lo toma; eso no se verificó.',
  'A new Codex process shows the server as the file says (checked). A Codex session that is already running is not known to follow it.':
    'Un proceso nuevo de Codex muestra el servidor como dice el archivo (verificado). No se sabe si una sesión de Codex que ya está corriendo lo sigue.',
  'A new OpenCode process reads the file. That a running OpenCode follows it has not been checked.':
    'Un proceso nuevo de OpenCode lee el archivo. No se verificó que un OpenCode en ejecución lo siga.',
  'A new Codex process lists the skill as the file says (checked, through its own skills/list). A Codex session that is already running is not known to follow it.':
    'Un proceso nuevo de Codex lista la skill como dice el archivo (verificado, con su propio skills/list). No se sabe si una sesión de Codex que ya está corriendo la sigue.',
  'A new Claude Code session refuses to run a skill that is switched off (checked). Skills that come from a plugin are not affected. What a running session does is not known.':
    'Una sesión nueva de Claude Code no ejecuta una skill desactivada (verificado). Las skills que vienen de un plugin no se afectan. No se sabe qué hace una sesión que ya está corriendo.',
  'What this keeps is stored on this computer, encrypted, with a key held by your system’s credential store. If that store is reset or its key is lost, it cannot be put back from here.':
    'Lo que se guarda queda en esta computadora, cifrado, con una clave que tiene el almacén de credenciales del sistema. Si ese almacén se resetea o se pierde su clave, desde acá no se puede restaurar.',
  'What this keeps is stored on this computer as a private file that is not encrypted: it holds no secret, and no credential store could be reached.':
    'Lo que se guarda queda en esta computadora como archivo privado sin cifrar: no tiene ningún secreto, y no se pudo acceder a ningún almacén de credenciales.',
  'Done. The file was changed and read back.':
    'Listo. El archivo se cambió y se volvió a leer.',
  'Cancelled in the confirmation window. Nothing was changed.':
    'Cancelado en la ventana de confirmación. No se cambió nada.',
  'The file changed after this was planned, so nothing was written. Plan it again.':
    'El archivo cambió después de planear esto, así que no se escribió nada. Volvé a planearlo.',
  'The entry is no longer there.': 'La entrada ya no está.',
  'The entry is not the one that was planned. Nothing was written.':
    'La entrada no es la que se planeó. No se escribió nada.',
  'A conflict copy of this file exists beside it. Resolve that first; nothing was written.':
    'Hay una copia en conflicto de este archivo al lado. Resolvela primero; no se escribió nada.',
  'The plan was too old and was dropped. Plan it again.':
    'El plan era muy viejo y se descartó. Volvé a planearlo.',
  'Your system’s credential store is not available, so an entry that may hold a secret is not kept. Nothing was written.':
    'El almacén de credenciales del sistema no está disponible, así que no se guarda una entrada que puede tener un secreto. No se escribió nada.',
  'The kept entry cannot be read. Nothing was written.':
    'La entrada guardada no se puede leer. No se escribió nada.',
  'The file could not be written. It is as it was.':
    'No se pudo escribir el archivo. Quedó como estaba.',
  'The file was written, but what it holds is not what was planned. Check it.':
    'El archivo se escribió, pero lo que tiene no es lo que se planeó. Revisalo.',
  'The history could not be written, so nothing was changed.':
    'No se pudo escribir el historial, así que no se cambió nada.',
  'Interrupted, then found complete at start.':
    'Interrumpido; al arrancar se encontró completo.',
  'Interrupted before it changed the file.':
    'Interrumpido antes de cambiar el archivo.',
  'Interrupted while Claude Code was writing the same file. Its newer version was put back and nothing of ours was kept.':
    'Interrumpido mientras Claude Code escribía el mismo archivo. Se restauró su versión más nueva y no se guardó nada nuestro.',
  'Interrupted, and the file holds neither version. Check it.':
    'Interrumpido, y el archivo no tiene ninguna de las dos versiones. Revisalo.',
  'Changing the configuration is not available here.':
    'Cambiar la configuración no está disponible acá.',
  'That request is not one the app accepts.':
    'Esa solicitud no es una que la app acepte.',
  'Something went wrong. Nothing was changed.':
    'Algo salió mal. No se cambió nada.',
  'That cannot be changed from here.': 'Eso no se puede cambiar desde acá.',
  'Its file is not one this app may write: it is shared, owned by someone else, or provider-owned.':
    'Su archivo no es uno que esta app pueda escribir: es compartido, de otra persona o del proveedor.',
  'Its file is not there.': 'Su archivo no está.',
  'Its file could not be read.': 'No se pudo leer su archivo.',
  'Its file is not valid JSON, so it is left alone.':
    'Su archivo no es JSON válido, así que no se toca.',
  'The entry is not the one that was planned, or has a value this app does not know how to change.':
    'La entrada no es la que se planeó, o tiene un valor que esta app no sabe cambiar.',
  'It already is that way.': 'Ya está así.',
  'Something already has that name. It was not touched, and nothing was written.':
    'Ya existe algo con ese nombre. No se tocó nada y no se escribió nada.',
  'A different entry is where this one would go. Nothing was written.':
    'Hay otra entrada donde iría esta. No se escribió nada.',
  'Your system’s credential store is not available, and this entry may hold a secret, so it is not turned off individually. You can turn off every hook, or edit the file yourself.':
    'El almacén de credenciales del sistema no está disponible, y esta entrada puede tener un secreto, así que no se desactiva por separado. Podés desactivar todos los hooks o editar el archivo vos mismo.',
  'The kept entry is damaged and was not used.':
    'La entrada guardada está dañada y no se usó.',
  'The key that protected this entry is gone, so it cannot be put back from here.':
    'Se perdió la clave que protegía esta entrada, así que desde acá no se puede restaurar.',
  'That plan is gone. Plan it again.':
    'Ese plan ya no está. Volvé a planearlo.',
  'That cannot be undone from here.': 'Eso no se puede deshacer desde acá.',
  'That file is not a kept entry, or the passphrase is wrong.':
    'Ese archivo no es una entrada guardada, o la frase de paso es incorrecta.',
  'A passphrase is needed.': 'Hace falta una frase de paso.',
  'That is not something that can be written: an event this app does not know, an empty or multi-line command, an unusable name or address.':
    'Eso no se puede escribir: un evento que esta app no conoce, un comando vacío o de varias líneas, o un nombre o una dirección que no sirven.',
  'Cancelled. Nothing was changed.': 'Cancelado. No se cambió nada.',
  'That folder cannot be the project: it must be your own, inside your home, with no link on the way and nobody else able to write it or the folders above it.':
    'Esa carpeta no puede ser el proyecto: tiene que ser tuya, dentro de tu carpeta personal, sin enlaces en el camino y sin que otro pueda escribirla ni las carpetas de arriba.',
  'can start': 'puede arrancar',
  'cannot start: a hook is enabled in Codex’s files':
    'no puede arrancar: hay un hook activado en los archivos de Codex',
  'cannot start: an MCP server’s name or count cannot be proved':
    'no puede arrancar: no se puede comprobar el nombre ni la cantidad de un servidor MCP',
  'Exported. Keep the file and the passphrase safe.':
    'Exportado. Guardá el archivo y la frase de paso en un lugar seguro.',
  'Imported. It is listed under Turned off.':
    'Importado. Figura en Desactivados.',
  'The plan was dropped because it was too old.':
    'Se descartó el plan porque era muy viejo.',
  'Turn off': 'Desactivar',
  'Turn on': 'Activar',
  'Claude Code skill': 'Skill de Claude Code',
  'Claude Code rule (rules/…)': 'Regla de Claude Code (rules/…)',
  'Claude Code CLAUDE.md': 'CLAUDE.md de Claude Code',
  'Codex skill (~/.agents/skills)': 'Skill de Codex (~/.agents/skills)',
  'Codex AGENTS.md': 'AGENTS.md de Codex',
  'Codex AGENTS.override.md': 'AGENTS.override.md de Codex',
  'OpenCode AGENTS.md': 'AGENTS.md de OpenCode',

  // Events: the authority's record, the presenter that words each event and the window's labels
  All: 'Todos',
  Lease: 'Concesión',
  Approval: 'Aprobación',
  'Needs a look': 'Hay que revisar',
  Isolation: 'Aislamiento',
  session: 'sesión',
  '{count} loaded': '{count} cargados',
  'newest first': 'más recientes primero',
  '{count} shown': '{count} mostrados',
  'Newest events': 'Eventos más recientes',
  'Turn {id}': 'Turno {id}',
  Clear: 'Quitar',
  'Filter events': 'Filtrar eventos',
  'Events, newest first': 'Eventos, más recientes primero',
  'Event details': 'Detalles del evento',
  'Events of an earlier stream': 'Eventos de un stream anterior',
  'Load older': 'Cargar anteriores',
  'Older events exist and are not loaded.':
    'Hay eventos anteriores que no están cargados.',
  'New events available': 'Hay eventos nuevos',
  'No loaded event matches.': 'Ningún evento cargado coincide.',
  'Filters apply to the events loaded here, not to all of history.':
    'Los filtros se aplican a los eventos cargados acá, no a todo el historial.',
  'Authority offline. Showing previously loaded history: it was read before the connection was lost and nothing newer is known.':
    'Autoridad sin conexión. Se muestra el historial cargado antes: se leyó antes de que se cortara la conexión y no se sabe nada más nuevo.',
  'Showing previously loaded history. It is being read again.':
    'Se muestra el historial cargado antes. Se está leyendo de nuevo.',
  'The authority restarted. It kept the same event stream: the history below continues, and what it ended on the way is recorded in it.':
    'La autoridad se reinició. Conservó el mismo stream de eventos: el historial de abajo sigue, y lo que quedó a medio camino consta en él.',
  "Events are the authority's own record and are read from it. Nothing is shown until it is connected. Your local terminals keep working.":
    'Los eventos son el registro propio de la autoridad y se leen de ella. No se muestra nada hasta que esté conectada. Tus terminales locales siguen funcionando.',
  'Reading events…': 'Leyendo eventos…',
  'No events yet.': 'Todavía no hay eventos.',
  'The authority records every session, runtime, turn, approval and lease change it commits. Nothing has been recorded in this stream.':
    'La autoridad registra cada cambio de sesión, runtime, turno, aprobación y concesión que confirma. Todavía no se registró nada en este stream.',
  'New authority event stream': 'Nuevo stream de eventos de la autoridad',
  'The events above belong to the stream the authority holds now. The ones below were read from another stream (':
    'Los eventos de arriba pertenecen al stream que la autoridad tiene ahora. Los de abajo se leyeron de otro stream (',
  ') and are not continued by them: positions of the two are unrelated.':
    ') y no continúan en ellos: las posiciones de ambos no tienen relación entre sí.',
  'Lease until': 'Concesión hasta',
  Position: 'Posición',
  'in stream {id}': 'en el stream {id}',
  'This record was read from a stream the authority no longer holds here. What it is about cannot be opened or read from this authority.':
    'Este registro se leyó de un stream que la autoridad ya no tiene acá. Aquello a lo que se refiere no se puede abrir ni leer desde esta autoridad.',
  'This is a record of what happened then. What it is about may have changed since: that is read from the authority where it lives.':
    'Es un registro de lo que pasó en ese momento. Aquello a lo que se refiere puede haber cambiado desde entonces: eso se lee de la autoridad donde vive.',
  'Open session': 'Abrir sesión',
  'Open in Agents': 'Abrir en Agentes',
  'Show events of this turn': 'Ver eventos de este turno',
  'Show events of this session': 'Ver eventos de esta sesión',
  'Check what it is now': 'Ver cómo está ahora',
  'Open Approvals': 'Abrir Aprobaciones',
  'Now:': 'Ahora:',
  'The authority no longer holds what it was about.':
    'La autoridad ya no tiene aquello a lo que se refería.',
  'What it is about is shown in Approvals.':
    'Aquello a lo que se refiere se ve en Aprobaciones.',
  'Approval detail is no longer available. The record above is still what happened.':
    'El detalle de la aprobación ya no está disponible. El registro de arriba sigue siendo lo que pasó.',
  'The approval could not be read from the authority.':
    'No se pudo leer la aprobación desde la autoridad.',
  'bind a workspace': 'vincular un workspace',
  'attach to Codex': 'conectarse a Codex',
  'start a turn': 'iniciar un turno',
  'run approved commands': 'correr comandos aprobados',
  'apply approved file changes': 'aplicar cambios de archivos aprobados',
  'Codex was not available to start.':
    'Codex no estuvo disponible para arrancar.',
  'The authority could not launch Codex.': 'La autoridad no pudo lanzar Codex.',
  'The connection to Codex failed.': 'Falló la conexión con Codex.',
  'Codex did not answer in time when it started.':
    'Codex no respondió a tiempo al arrancar.',
  'Codex did not speak the protocol the authority expects.':
    'Codex no habló el protocolo que espera la autoridad.',
  'Codex asked for something this runtime is not allowed to answer.':
    'Codex pidió algo que este runtime no puede responder.',
  'Codex refused the configuration it was started with.':
    'Codex rechazó la configuración con la que arrancó.',
  'Codex refused the model catalog it was started with.':
    'Codex rechazó el catálogo de modelos con el que arrancó.',
  'The Codex process ended.': 'El proceso de Codex terminó.',
  'The authority could not record what Codex was doing.':
    'La autoridad no pudo registrar lo que hacía Codex.',
  'Codex acted outside what its isolation allows, and the authority stopped it.':
    'Codex actuó fuera de lo que permite su aislamiento, y la autoridad lo detuvo.',
  'The authority could not prove the isolation of the runtime, and did not let it run.':
    'La autoridad no pudo comprobar el aislamiento del runtime, y no lo dejó correr.',
  'Managed session prepared': 'Sesión Managed preparada',
  'Managed session closed': 'Sesión Managed cerrada',
  'Session bound to a workspace': 'Sesión vinculada a un workspace',
  'Session attached to Codex': 'Sesión conectada a Codex',
  'Workspace binding outcome unknown':
    'Resultado del vínculo con el workspace desconocido',
  'Uncertain binding attempt discarded':
    'Intento de vínculo incierto descartado',
  'Codex runtime start accepted': 'Inicio del runtime de Codex aceptado',
  'Codex runtime stop accepted': 'Detención del runtime de Codex aceptada',
  'Codex runtime request accepted': 'Pedido al runtime de Codex aceptado',
  'Codex runtime ready': 'Runtime de Codex listo',
  'Codex runtime contained': 'Runtime de Codex contenido',
  'Codex runtime stopped': 'Runtime de Codex detenido',
  'Codex runtime ended unexpectedly':
    'El runtime de Codex terminó de forma inesperada',
  'Managed turn admitted': 'Turno Managed admitido',
  'Managed turn running': 'Turno Managed en curso',
  'Turn cancellation requested': 'Se pidió cancelar el turno',
  'Managed turn completed': 'Turno Managed completado',
  'Managed turn interrupted': 'Turno Managed interrumpido',
  'Managed turn failed': 'Turno Managed falló',
  'Managed turn outcome unknown': 'Resultado del turno Managed desconocido',
  'Command approval requested': 'Aprobación de comando pedida',
  'File change approval requested': 'Aprobación de cambio de archivo pedida',
  'Approved action confirmed by Codex': 'Codex confirmó la acción aprobada',
  'Approval denied': 'Aprobación denegada',
  'Execution lease granted': 'Concesión de ejecución otorgada',
  'Execution lease renewed': 'Concesión de ejecución renovada',
  'Execution lease revoked': 'Concesión de ejecución revocada',
  'Execution lease expired': 'Concesión de ejecución vencida',
  'Authority event': 'Evento de la autoridad',
  'a command': 'un comando',
  'the approved command': 'el comando aprobado',
  'a file change': 'un cambio de archivo',
  'the approved file change': 'el cambio de archivo aprobado',
  'an action': 'una acción',
  'the approved action': 'la acción aprobada',
  'The authority admitted the turn under an execution lease. What was asked of Codex is not part of the record.':
    'La autoridad admitió el turno con una concesión de ejecución. Lo que se le pidió a Codex no forma parte del registro.',
  'Codex took the turn and started working on it.':
    'Codex tomó el turno y empezó a trabajar en él.',
  'A cancel of the turn was recorded.':
    'Se registró una cancelación del turno.',
  'The authority recorded how the turn ended.':
    'La autoridad registró cómo terminó el turno.',
  'Codex proposed {about} and the authority held it for a decision. The proposal itself is not part of the record.':
    'Codex propuso {about} y la autoridad retuvo esa propuesta para decidir. La propuesta en sí no forma parte del registro.',
  'The approval of {about} was accepted and the answer was sent to Codex.':
    'Se aceptó la aprobación de {about} y la respuesta se envió a Codex.',
  'Codex reported what it did with {approved}.':
    'Codex informó qué hizo con {approved}.',
  'The approval of {about} was denied. Nothing was run or changed for it.':
    'Se denegó la aprobación de {about}. No se ejecutó ni se cambió nada por ella.',
  'The authority recorded how the approval ended.':
    'La autoridad registró cómo terminó la aprobación.',
  '{lead} It allowed the session to {scopes}.':
    '{lead} Permitió a la sesión {scopes}.',
  'The lease ended because its session was closed.':
    'La concesión terminó porque se cerró su sesión.',
  'The lease was given back.': 'La concesión se devolvió.',
  'The lease reached its time limit.':
    'La concesión llegó a su límite de tiempo.',
  'The authority extended a lease for this session.':
    'La autoridad extendió una concesión para esta sesión.',
  'The authority granted a lease for this session.':
    'La autoridad otorgó una concesión para esta sesión.',
  'The authority accepted a request for the managed Codex runtime and recorded what it should be.':
    'La autoridad aceptó un pedido para el runtime de Codex Managed y registró cómo debe quedar.',
  'Codex {version} started, was checked and is ready.':
    'Codex {version} se inició, se verificó y está listo.',
  'Codex started, was checked and is ready.':
    'Codex se inició, se verificó y está listo.',
  '{exit} The authority meant it to.': '{exit} La autoridad lo quiso así.',
  'A managed session was created. It has no workspace until it is bound.':
    'Se creó una sesión Managed. No tiene workspace hasta que se vincula uno.',
  'The session was closed. Closing is final.':
    'La sesión se cerró. Cerrar es definitivo.',
  'The authority bound the session to a project folder. Which folder is not part of the record.':
    'La autoridad vinculó la sesión a una carpeta de proyecto. Cuál carpeta no forma parte del registro.',
  'The session was attached to the Codex runtime that was running.':
    'La sesión se conectó al runtime de Codex que estaba corriendo.',
  'The authority could not confirm whether Codex created the workspace binding.':
    'La autoridad no pudo confirmar si Codex creó el vínculo con el workspace.',
  'An uncertain binding attempt was given up on explicitly.':
    'Se abandonó explícitamente un intento de vínculo incierto.',
  'The authority recorded an event of a kind this version of the console does not know. Nothing else of it is shown.':
    'La autoridad registró un evento de un tipo que esta versión de la consola no conoce. No se muestra nada más de él.',
  'This is the beginning of the event stream.':
    'Este es el comienzo del stream de eventos.',
  'History before this point is unavailable.':
    'El historial anterior a este punto no está disponible.',
  'Events could not be read: the authority did not answer.':
    'No se pudieron leer los eventos: la autoridad no respondió.',
  'Events could not be read. The authority is busy; try again in a moment.':
    'No se pudieron leer los eventos. La autoridad está ocupada; probá de nuevo en un momento.',
  'The event history changed while it was being read.':
    'El historial de eventos cambió mientras se leía.',
  'Events could not be read. Nothing is shown rather than something unverified.':
    'No se pudieron leer los eventos. No se muestra nada en lugar de algo sin verificar.',
  'Older events could not be read: the authority holds another history than the one shown. The newest events were read again.':
    'No se pudieron leer los eventos anteriores: la autoridad tiene otro historial que el que se muestra. Los eventos más recientes se leyeron de nuevo.',
  'Older events could not be read: the authority did not answer.':
    'No se pudieron leer los eventos anteriores: la autoridad no respondió.',
  'Older events could not be read.':
    'No se pudieron leer los eventos anteriores.',

  // Managed runtimes (F8): the labels, gates, failures and link of the runtime views
  'The runtime could not be found.': 'No se encontró el runtime.',
  'The runtime process could not be started.':
    'No se pudo arrancar el proceso del runtime.',
  'The connection to the runtime broke.':
    'Se cortó la conexión con el runtime.',
  'The runtime did not finish starting in time.':
    'El runtime no terminó de arrancar a tiempo.',
  'The runtime answered in a way the authority does not accept.':
    'El runtime respondió de una forma que la autoridad no acepta.',
  'The runtime asked for something it may not ask for.':
    'El runtime pidió algo que no tiene permitido pedir.',
  'The runtime refused to start.': 'El runtime rechazó arrancar.',
  'The runtime listed models the authority does not accept.':
    'El runtime listó modelos que la autoridad no acepta.',
  'The runtime process exited.': 'El proceso del runtime terminó.',
  'The authority could not record what the runtime did.':
    'La autoridad no pudo registrar lo que hizo el runtime.',
  'The runtime tried to act outside what its session allows, and the authority stopped it.':
    'El runtime intentó actuar fuera de lo que permite su sesión, y la autoridad lo detuvo.',
  'The authority could not prove the runtime was isolated, so it did not use it.':
    'La autoridad no pudo comprobar que el runtime estaba aislado, así que no lo usó.',
  'A resource limit ended it: {key}.':
    'Un límite de recursos lo terminó: {key}.',
  'Binding…': 'Vinculando…',
  'Outcome unknown': 'Resultado desconocido',
  'Workspace bound': 'Workspace vinculado',
  Attached: 'Conectada',
  'Bind a workspace': 'Vincular un workspace',
  'Resume a binding': 'Retomar un vínculo',
  'Start turns': 'Iniciar turnos',
  'Run commands': 'Correr comandos',
  'Change files': 'Cambiar archivos',
  'Lets the session be bound to a workspace folder.':
    'Permite vincular la sesión a una carpeta de workspace.',
  'Lets a bound session be attached to a running runtime again.':
    'Permite volver a conectar una sesión vinculada a un runtime que corre.',
  'Lets a turn start. Without it nothing runs.':
    'Permite iniciar un turno. Sin esto no corre nada.',
  'Lets Codex ask to run a command. Each one still waits for you.':
    'Permite que Codex pida correr un comando. Cada uno igual espera tu aprobación.',
  'Lets Codex ask to change files. Each change still waits for you.':
    'Permite que Codex pida cambiar archivos. Cada cambio igual espera tu aprobación.',
  '{minutes} min': '{minutes} min',
  '{hours} h': '{hours} h',
  'Allowed.': 'Permitido.',
  'The session has no workspace.': 'La sesión no tiene workspace.',
  'The session is not attached to a running runtime.':
    'La sesión no está conectada a un runtime que corre.',
  'The session is attached to a different runtime launch.':
    'La sesión está conectada a otro arranque de runtime.',
  'The session is not attached in a mode that allows this.':
    'La sesión no está conectada en un modo que permita esto.',
  'The session is not attached in Local Tools.':
    'La sesión no está conectada en Local Tools.',
  'The runtime has no safety profile.':
    'El runtime no tiene perfil de seguridad.',
  'A turn is already running.': 'Ya hay un turno corriendo.',
  'The authority has no such session.': 'La autoridad no tiene esa sesión.',
  'The session is closed.': 'La sesión está cerrada.',
  'There is no execution lease.': 'No hay concesión de ejecución.',
  'The execution lease is not active.':
    'La concesión de ejecución no está activa.',
  'The execution lease expired.': 'La concesión de ejecución venció.',
  'The execution lease does not grant this.':
    'La concesión de ejecución no permite esto.',
  'The runtime is not the session’s.': 'El runtime no es el de la sesión.',
  'The runtime is not running.': 'El runtime no está corriendo.',
  'The runtime is not ready.': 'El runtime no está listo.',
  'The runtime has not been attested yet.':
    'Todavía no se verificó el runtime.',
  'The runtime is not a version the authority accepts.':
    'El runtime no es una versión que la autoridad acepta.',
  'The authority is unavailable.': 'La autoridad no está disponible.',
  'This console is not connected to managed runtimes.':
    'Esta consola no está conectada a los runtimes Managed.',
  'The authority did not answer, so the outcome is not known. Nothing was canceled and it may still have happened. The console reads the authority again; check what it holds before sending this again.':
    'La autoridad no respondió, así que no se sabe el resultado. No se canceló nada y puede que haya pasado igual. La consola vuelve a leer la autoridad; revisá qué tiene antes de mandar esto de nuevo.',
  'This authority does not offer that. Nothing changed; try another mode.':
    'Esta autoridad no ofrece eso. No cambió nada; probá otro modo.',
  'What was shown is no longer what the authority holds. It has been read again.':
    'Lo que se mostraba ya no es lo que tiene la autoridad. Se volvió a leer.',
  'The session has no workspace yet.': 'La sesión todavía no tiene workspace.',
  'The session has no ready runtime for this.':
    'La sesión no tiene un runtime listo para esto.',
  'A turn is running. Nothing is stopped under it.':
    'Hay un turno corriendo. No se detiene nada por debajo.',
  'The authority did not accept the folder as a workspace.':
    'La autoridad no aceptó la carpeta como workspace.',
  'The session already has a workspace.': 'La sesión ya tiene un workspace.',
  'The authority denied it: no execution lease allows this action.':
    'La autoridad lo denegó: ninguna concesión de ejecución permite esta acción.',
  'The session already has an active execution lease.':
    'La sesión ya tiene una concesión de ejecución activa.',
  'The authority is at its limit for this and did not take it. Nothing changed. Only one runtime runs at a time: stop the one that is running, then start this one.':
    'La autoridad llegó a su límite para esto y no lo tomó. No cambió nada. Solo corre un runtime a la vez: detené el que corre y después arrancá este.',
  'The agent runtime refused.': 'El runtime del agente rechazó.',
  'The authority could not confirm what the runtime did. Nothing is sent again by itself.':
    'La autoridad no pudo confirmar lo que hizo el runtime. No se vuelve a mandar nada por sí solo.',
  'It was not allowed in the confirmation window, so nothing was sent.':
    'No se permitió en la ventana de confirmación, así que no se mandó nada.',
  'No managed-runtime authority is listening.':
    'No hay ninguna autoridad de runtimes Managed escuchando.',
  'The authority stopped answering.': 'La autoridad dejó de responder.',
  'The authority broke the protocol, so the connection was dropped.':
    'La autoridad rompió el protocolo, así que se cortó la conexión.',
  'The authority’s socket is not this user’s private one.':
    'El socket de la autoridad no es el privado de este usuario.',
  Revoked: 'Revocada',
  Expired: 'Vencida',
  'Expires in {n} s': 'Vence en {n} s',
  'Expires in {n} min': 'Vence en {n} min',
  'Expires in {n} h': 'Vence en {n} h',
  'No runtime yet': 'Todavía no hay runtime',
  '{state} · generation {generation}{profile}':
    '{state} · generación {generation}{profile}',

  // History (F8): the session history panel
  'This session only': 'Solo esta sesión',
  'Narrows the window loaded here, not all of history.':
    'Acota la ventana cargada acá, no todo el historial.',
  'Every session.': 'Todas las sesiones.',
  Newest: 'Más reciente',
  'History is unavailable': 'El historial no está disponible',
  'Reading history…': 'Leyendo el historial…',
  'The authority records every state change it commits.':
    'La autoridad registra cada cambio de estado que confirma.',
  'History before the oldest event shown is not available.':
    'El historial anterior al evento más viejo mostrado no está disponible.',
  'No event in this window belongs to this session.':
    'Ningún evento de esta ventana pertenece a esta sesión.',
  'Newest first': 'Más recientes primero',
  '· {count} window back': '· {count} ventana atrás',
  '· {count} windows back': '· {count} ventanas atrás',

  // Overview (F8): the session overview panel
  Conversation: 'Conversación',
  'The runtime is changing state.': 'El runtime está cambiando de estado.',
  'Start the runtime to work in this session.':
    'Iniciá el runtime para trabajar en esta sesión.',
  'Grant an execution lease so the session may bind a workspace and run turns.':
    'Otorgá una concesión de ejecución para que la sesión pueda asociar un workspace y correr turnos.',
  'Bind a project folder to this session.':
    'Asociá una carpeta de proyecto a esta sesión.',
  'Attach the session to the running runtime.':
    'Conectá la sesión al runtime en ejecución.',
  'The authority could not confirm the last binding attempt. Nothing asks again by itself.':
    'La autoridad no pudo confirmar el último intento de asociación. Nada vuelve a preguntar por sí solo.',
  'The authority is binding the workspace.':
    'La autoridad está asociando el workspace.',
  'Ready. Open the Conversation tab to give Codex an objective.':
    'Listo. Abrí la pestaña Conversación para darle un objetivo a Codex.',
  'Provider support': 'Soporte del proveedor',
  'Claude Code Managed is not implemented in this authority. This session remains Codex; Native use requires a separate, explicit launch.':
    'Claude Code Managed no está implementado en esta autoridad. Esta sesión sigue siendo Codex; el uso Native requiere un lanzamiento aparte y explícito.',
  Generation: 'Generación',
  'None yet': 'Todavía ninguna',
  'Each start is a new generation; an earlier one never comes back.':
    'Cada inicio es una generación nueva; una anterior nunca vuelve.',
  'Isolated for': 'Aislado para',
  Wanted: 'Pedido',
  revision: 'revisión',
  'This session has used every launch the authority allows it. No further runtime is started for it; a new session can be created.':
    'Esta sesión usó todos los lanzamientos que la autoridad permite. No se inicia más runtime para ella; se puede crear una sesión nueva.',
  'Confirm stop': 'Confirmar detención',
  'Stop this runtime? Its process is ended and reaped. A turn that runs is not stopped under it: the authority refuses until that turn ends.':
    '¿Detener este runtime? Su proceso se termina y se libera. Un turno en curso no se corta por debajo: la autoridad lo rechaza hasta que ese turno termine.',
  'Stop runtime': 'Detener runtime',
  'Keep running': 'Seguir corriendo',
  'Local Tools gives Codex more to work with, not more authority: every command and file change waits for your approval. The runtime is isolated for one folder.':
    'Local Tools le da a Codex más con qué trabajar, no más autoridad: cada comando y cada cambio de archivo espera tu aprobación. El runtime queda aislado para una sola carpeta.',
  'Start runtime': 'Iniciar runtime',
  'The runtime is not running but is still wanted, for example after the authority restarted. Stop it to close this session.':
    'El runtime no está corriendo, pero sigue pedido; por ejemplo, después de que la autoridad se reinició. Detenelo para cerrar esta sesión.',
  Folder: 'Carpeta',
  'Folder:': 'Carpeta:',
  'none selected': 'ninguna seleccionada',
  'Execution lease': 'Concesión de ejecución',
  Granted: 'Otorgadas',
  'Valid until': 'Válida hasta',
  '. When it ends or is revoked, a turn that runs under it ends, and so does the approval it waits on.':
    '. Cuando termina o se revoca, el turno que corre bajo ella termina, y también la aprobación que espera.',
  'Confirm revoke': 'Confirmar revocación',
  'Revoke the lease? The turn that runs under it ends and its pending approval is invalidated, in the same commit.':
    '¿Revocar la concesión? El turno que corre bajo ella termina y su aprobación pendiente se invalida, en el mismo paso.',
  'Revoke lease': 'Revocar concesión',
  'Renew for': 'Renovar por',
  Renew: 'Renovar',
  Revoke: 'Revocar',
  Allow: 'Permitir',
  For: 'Por',
  'Grant lease': 'Otorgar concesión',
  'A lease is a permission with an end, never a standing one. It needs a ready runtime':
    'Una concesión es un permiso con fin, nunca permanente. Necesita un runtime listo',
  'start it first': 'iniciarlo primero',
  'Attached as': 'Conectada como',
  'Model in use': 'Modelo en uso',
  'Binding outcome uncertain': 'Resultado de la asociación incierto',
  'The authority asked Codex to create a thread and could not confirm what Codex did. A second request could create a second thread, so nothing asks again by itself.':
    'La autoridad le pidió a Codex que creara un hilo y no pudo confirmar qué hizo Codex. Un segundo pedido podría crear un segundo hilo, así que nada vuelve a pedirlo por sí solo.',
  'Discard the uncertain attempt? The authority records that it was abandoned. If Codex did create a thread for it, the empty thread is left behind. The session can then be bound again.':
    '¿Descartar el intento incierto? La autoridad registra que se abandonó. Si Codex sí creó un hilo para él, el hilo vacío queda. Después la sesión se puede volver a asociar.',
  'Bind workspace': 'Asociar workspace',
  'Attach to runtime': 'Conectar al runtime',
  'The authority decides whether it can be a workspace.':
    'La autoridad decide si puede ser un workspace.',
  'What the authority would allow now': 'Lo que la autoridad permitiría ahora',
  Allowed: 'Permitido',
  'Not now': 'Ahora no',
  'This is a reading, not a permission: the authority checks again at the moment of every action.':
    'Esto es una lectura, no un permiso: la autoridad vuelve a verificar en el momento de cada acción.',
  'Close this session? It cannot be reopened, and its execution lease is revoked. The authority refuses while its runtime runs or a turn is active.':
    '¿Cerrar esta sesión? No se puede reabrir, y su concesión de ejecución se revoca. La autoridad rechaza el cierre mientras su runtime corre o hay un turno activo.',
  'Close…': 'Cerrar…',
  'Stop the runtime first.': 'Detené el runtime primero.',

  // Conversation and approval (F8): the turn transcript and the approval card
  'Recent turns': 'Turnos recientes',
  'Older turns are in History.': 'Los turnos anteriores están en Historial.',
  'No turn is open.': 'No hay ningún turno abierto.',
  'The input of a turn is not shown: Wyss keeps no conversation text.':
    'No se muestra la entrada de un turno: Wyss no guarda texto de conversación.',
  Thinking: 'Pensando',
  'Following the turn…': 'Siguiendo el turno…',
  'The authority stopped answering; reading the turn again…':
    'La autoridad dejó de responder; leyendo el turno de nuevo…',
  'This turn can no longer be followed: the authority does not have it or did not answer. What it did is read from History.':
    'Este turno ya no se puede seguir: la autoridad no lo tiene o no respondió. Lo que hizo se lee desde Historial.',
  'Reading what Codex asked for…': 'Leyendo lo que pidió Codex…',
  'The authority is not connected.': 'La autoridad no está conectada.',
  'A turn is running. Cancel it to start another.':
    'Hay un turno en curso. Cancelalo para iniciar otro.',
  'The objective is too long.': 'El objetivo es demasiado largo.',
  '· gen {generation}': '· gen. {generation}',
  'generation {generation}': 'generación {generation}',
  Command: 'Comando',
  Directory: 'Directorio',
  '· turn': '· turno',
  'Preview truncated:': 'Vista previa truncada:',
  'Show the rest': 'Mostrar el resto',
  'The rest of this change could not be read.':
    'No se pudo leer el resto de este cambio.',
  '{shown} of {total} files shown.': '{shown} de {total} archivos mostrados.',
  'Show more files': 'Mostrar más archivos',
  'The authority no longer holds what this approval was about. It cannot be accepted.':
    'La autoridad ya no tiene aquello a lo que se refería esta aprobación. No se puede aceptar.',
  'Its time ran out at': 'Su plazo venció a las',
  'Waits until': 'Espera hasta',
  '— the authority checks the action again before anything runs.':
    '— la autoridad vuelve a verificar la acción antes de que se ejecute nada.',
  'The runtime this approval was asked on is no longer the live one, so it cannot be decided.':
    'El runtime en el que se pidió esta aprobación ya no es el activo, así que no se puede decidir.',
  'The authority did not answer, so it is not known whether the decision was taken. Retrying repeats the same decision: it cannot decide twice.':
    'La autoridad no respondió, así que no se sabe si la decisión se tomó. Reintentar repite la misma decisión: no puede decidir dos veces.',
  Accept: 'Aceptar',
  Decline: 'Rechazar',
  'Cancel action': 'Cancelar acción',
  'Read the whole change to accept it.':
    'Leé el cambio completo para aceptarlo.',
  'Codex wants to run a command': 'Codex quiere ejecutar un comando',
  'Codex wants to change a file': 'Codex quiere cambiar un archivo',
  'Codex wants to change {count} files':
    'Codex quiere cambiar {count} archivos',
  'Codex wants to change files': 'Codex quiere cambiar archivos',
  'Waiting for your Allow in the confirmation window, then asking the authority…':
    'Esperando tu Permitir en la ventana de confirmación; después se lo pide a la autoridad…',
  'Asking the authority to decline it…':
    'Pidiéndole a la autoridad que la rechace…',
  'Asking the authority to cancel it…':
    'Pidiéndole a la autoridad que la cancele…',
  'It was not accepted.': 'No se aceptó.',
  'It was not declined.': 'No se rechazó.',
  'It was not cancelled.': 'No se canceló.',
  Pending: 'Pendiente',
  Approved: 'Aprobada',
  Denied: 'Rechazada',
  Cancelled: 'Cancelada',
  'No longer applies': 'Ya no aplica',
  'File change': 'Cambio de archivo',
  Add: 'Agregar',
  Delete: 'Borrar',
  Modify: 'Modificar',
  Move: 'Mover',
  'Nobody decided it in time.': 'Nadie la decidió a tiempo.',
  'The execution lease of its turn expired.':
    'La concesión de ejecución de su turno venció.',
  'The execution lease of its turn was revoked.':
    'Se revocó la concesión de ejecución de su turno.',
  'Its turn was cancelled.': 'Su turno se canceló.',
  'Its turn ended.': 'Su turno terminó.',
  'Its session was closed.': 'Su sesión se cerró.',
  'Codex stopped while it waited.': 'Codex se detuvo mientras esperaba.',
  'The authority shut down while it waited.':
    'La autoridad se apagó mientras esperaba.',
  'The authority restarted while it waited.':
    'La autoridad se reinició mientras esperaba.',
  'It was approved, and the authority could not confirm what Codex did with that.':
    'Fue aprobada, y la autoridad no pudo confirmar qué hizo Codex con eso.',
  'Codex asked for something that cannot be approved.':
    'Codex pidió algo que no se puede aprobar.',
  'Codex acted outside what the session allows.':
    'Codex actuó fuera de lo que la sesión permite.',

  // Models (F8): the models view
  'No models to show': 'No hay modelos para mostrar',
  'The models are what a running runtime listed when it started. A runtime that has stopped has no list, and an earlier one is never shown for a later one.':
    'Los modelos son lo que un runtime en ejecución listó al arrancar. Un runtime que se detuvo no tiene lista, y una anterior nunca se muestra para uno posterior.',
  'Reading the models…': 'Leyendo los modelos…',
  'Runtime {version} · generation {generation}.':
    'Runtime {version} · generación {generation}.',
  'The runtime listed more models than the authority took.':
    'El runtime listó más modelos de los que la autoridad tomó.',
  'A listed model is offered, not entitled: nothing here says the account may use it. Protocol 2 has no request that selects a model or a reasoning effort, so a turn uses what the runtime defaults to, and this view only reports what is offered and what the runtime says it uses.':
    'Un modelo listado se ofrece, no está habilitado: nada acá dice que la cuenta pueda usarlo. El protocolo 2 no tiene un pedido que elija un modelo ni un esfuerzo de razonamiento, así que un turno usa el que el runtime tiene por defecto, y esta vista solo informa lo que se ofrece y lo que el runtime dice que usa.',
  'The runtime listed no models.': 'El runtime no listó modelos.',
  'Models offered by the runtime': 'Modelos ofrecidos por el runtime',
  'Runtime default': 'Predeterminado del runtime',
  'Supported reasoning efforts': 'Esfuerzos de razonamiento soportados',
  default: 'predeterminado',
  'no efforts listed': 'no hay esfuerzos listados',
  'The runtime marks': 'El runtime marca',
  'as its replacement': 'como su reemplazo',
  ', retiring {date}': ', que se retira el {date}',

  // Runtimes (F8): the managed runtimes view
  'Managed runtimes': 'Runtimes Managed',
  'Managed runtimes · protocol 2': 'Runtimes Managed · protocolo 2',
  '{prepared} prepared · {closed} closed · protocol 2':
    '{prepared} preparadas · {closed} cerradas · protocolo 2',
  Overview: 'Resumen',
  'Creating the session…': 'Creando la sesión…',
  'Closing the session…': 'Cerrando la sesión…',
  'Starting the runtime…': 'Iniciando el runtime…',
  'Stopping the runtime…': 'Deteniendo el runtime…',
  'Granting the execution lease…': 'Otorgando la concesión de ejecución…',
  'Renewing the execution lease…': 'Renovando la concesión de ejecución…',
  'Revoking the execution lease…': 'Revocando la concesión de ejecución…',
  'Binding the workspace…': 'Asociando el workspace…',
  'Attaching the session…': 'Conectando la sesión…',
  'Discarding the attempt…': 'Descartando el intento…',
  'Asking the authority to cancel the turn…':
    'Pidiéndole a la autoridad que cancele el turno…',
  'Waiting for the runtime to be ready…':
    'Esperando a que el runtime esté listo…',
  'Waiting for the runtime to stop…': 'Esperando a que el runtime se detenga…',
  'The authority is binding the workspace…':
    'La autoridad está asociando el workspace…',
  'Waiting for your answer in the confirmation window. Nothing is allowed until you choose Allow. If you cannot see it, it may be behind another window.':
    'Esperando tu respuesta en la ventana de confirmación. No se permite nada hasta que elijas Permitir. Si no la ves, puede estar detrás de otra ventana.',
  'The authority is not answering.': 'La autoridad no responde.',
  'The console asks again by itself.': 'La consola vuelve a preguntar sola.',
  'What is shown was read before the connection was lost; no action is offered.':
    'Lo que se muestra se leyó antes de perder la conexión; no se ofrece ninguna acción.',
  'Reconnect now': 'Reconectar ahora',
  'The authority restarted or was replaced. Its runtimes, leases and turns are not the ones shown before, so everything was read again and nothing from before is acted on.':
    'La autoridad se reinició o la reemplazaron. Sus runtimes, concesiones y turnos no son los que se mostraban antes, así que se volvió a leer todo y no se actúa sobre nada de antes.',
  'Managed runtimes are read from the authority and cannot be created, started or stopped until it is connected. The Console keeps working.':
    'Los runtimes Managed se leen de la autoridad y no se pueden crear, iniciar ni detener hasta que esté conectada. La consola sigue funcionando.',
  'Reading managed sessions…': 'Leyendo las sesiones Managed…',
  'A session gets a runtime of its own. Create one, start its runtime, give it an execution lease and bind a project folder to work in.':
    'Cada sesión tiene un runtime propio. Creá uno, iniciá su runtime, dale una concesión de ejecución y asociale una carpeta de proyecto donde trabajar.',
  'Select a session': 'Seleccioná una sesión',
  'Its runtime, workspace, execution lease, conversation, models and history are shown here, exactly as the authority holds them.':
    'Su runtime, workspace, concesión de ejecución, conversación, modelos e historial se muestran acá, tal como los tiene la autoridad.',
  'Session sections': 'Secciones de la sesión',
  // ---- arsenal-d ----
  turn: 'turno',
  'Preview truncated: {shown} of {total} bytes shown.':
    'Vista previa cortada: se muestran {shown} de {total} bytes.',
  'The authority no longer holds what this approval was about.':
    'La autoridad ya no tiene lo que describe esta aprobación.',
  'The authority checks the action again before anything runs.':
    'La autoridad vuelve a verificar la acción antes de que se ejecute algo.',
  'Read the whole change to approve it.':
    'Leé el cambio completo para aprobarlo.',
  '{count} waiting for a decision': '{count} esperando una decisión',
  'Approvals are read from the authority and cannot be decided until it is connected. Nothing is approved or denied in the meantime. Your local terminals keep working.':
    'Las aprobaciones se leen de la autoridad y no se pueden decidir hasta que esté conectada. Mientras tanto no se aprueba ni se deniega nada. Tus terminales locales siguen funcionando.',
  'Reading approvals…': 'Leyendo aprobaciones…',
  'Nothing is waiting for a decision.': 'Nada espera una decisión.',
  'In Local Tools, every command and file change Codex proposes stops here until you approve or deny it. Inference sessions propose none.':
    'En Local Tools, cada comando y cambio de archivo que propone Codex se detiene acá hasta que lo apruebes o lo denegues. Las sesiones de Inference no proponen ninguno.',
  'Pending approvals': 'Aprobaciones pendientes',
  'Recently ended': 'Terminadas recientemente',
  'Ended approvals': 'Aprobaciones terminadas',
  'Older approvals are not listed.':
    'Las aprobaciones más viejas no se listan.',
  'Asking the authority to approve it…':
    'Pidiendo a la autoridad que la apruebe…',
  'Asking the authority to deny it…':
    'Pidiendo a la autoridad que la deniegue…',
  'It was not approved.': 'No se aprobó.',
  'It was not denied.': 'No se denegó.',
  'The authority no longer has this approval.':
    'La autoridad ya no tiene esta aprobación.',
  'The approval changed after it was shown. It has been read again.':
    'La aprobación cambió después de mostrarse. Se volvió a leer.',
  'The approval had already ended.': 'La aprobación ya había terminado.',
  'The authority denied it: the lease its turn was admitted under does not allow this action.':
    'La autoridad la denegó: la concesión con la que se admitió su turno no permite esta acción.',
  'Codex is no longer there to answer.': 'Codex ya no está para responder.',
  'The authority could not confirm what Codex did with the answer. Nothing is sent again by itself.':
    'La autoridad no pudo confirmar qué hizo Codex con la respuesta. No se vuelve a enviar nada por sí solo.',
  'That request was already used for another decision.':
    'Ese pedido ya se usó para otra decisión.',
  'Approvals could not be read: the authority did not answer.':
    'No se pudieron leer las aprobaciones: la autoridad no respondió.',
  'Approvals could not be read. The authority is busy; try again in a moment.':
    'No se pudieron leer las aprobaciones. La autoridad está ocupada; probá de nuevo en un momento.',
  'Approvals could not be read. Nothing is shown rather than something unverified.':
    'No se pudieron leer las aprobaciones. No se muestra nada antes que algo sin verificar.',
  Start: 'Iniciar',
  'What to start': 'Qué iniciar',
  'A shell': 'Un shell',
  How: 'Cómo',
  "Native (the agent's own permissions)":
    'Nativo (los permisos propios del agente)',
  'Managed (mediated by the authority) — not available for Claude yet':
    'Managed (mediado por la autoridad): todavía no disponible para Claude',
  'Managed (mediated by the authority)': 'Managed (mediado por la autoridad)',
  'Managed profile — choose one': 'Perfil de Managed: elegí uno',
  'Inference (text only, no tools)': 'Inference (solo texto, sin herramientas)',
  'Local tools (commands and file changes, each waiting for approval)':
    'Local tools (comandos y cambios de archivo, cada uno esperando aprobación)',
  'This is a request. It grants nothing; the authority attaches the session in the profile it allows.':
    'Esto es un pedido. No otorga nada: la autoridad adjunta la sesión con el perfil que permite.',
  'Native profile': 'Perfil nativo',
  Default: 'Por defecto',
  'A file of the older state cannot be read as the schema it claims':
    'Un archivo del estado anterior no se puede leer con el esquema que declara',
  'A file of the older state kept changing while it was read':
    'Un archivo del estado anterior siguió cambiando mientras se leía',
  'A file of the older state is bigger than its bound':
    'Un archivo del estado anterior supera su límite',
  'A registered folder is another spelling of a registered root':
    'Una carpeta registrada es otra forma de escribir una raíz ya registrada',
  'A registered folder is missing or cannot be used (not imported)':
    'Falta una carpeta registrada o no se puede usar (no se importó)',
  'Another account can write in a registered folder, or that could not be checked (not imported)':
    'Otra cuenta puede escribir en una carpeta registrada, o no se pudo comprobar (no se importó)',
  'No terminal host answered: no live session is imported':
    'No respondió ningún host de terminales: no se importa ninguna sesión activa',
  'Something does not fit the limits; nothing is cut to fit':
    'Algo no entra en los límites; no se recorta nada para que entre',
  'An identity from the older state cannot be kept (not imported)':
    'Una identidad del estado anterior no se puede conservar (no se importó)',
  'The result would not be valid': 'El resultado no sería válido',
  'Import from the older windows': 'Importar desde las ventanas anteriores',
  'Projects and workspaces are built from the folders, agents and tabs the older windows already know. The preview reads them and changes nothing; the older windows keep working.':
    'Los proyectos y workspaces se arman con las carpetas, agentes y pestañas que ya conocen las ventanas anteriores. La vista previa los lee y no cambia nada; las ventanas anteriores siguen funcionando.',
  'The older windows changed since the last import. A new preview adds only what is new; nothing already here is changed.':
    'Las ventanas anteriores cambiaron desde la última importación. Una vista previa nueva agrega solo lo nuevo; no se cambia nada de lo que ya está acá.',
  'Preview the import': 'Ver vista previa de la importación',
  'Start empty instead': 'Empezar vacío',
  'Import preview': 'Vista previa de la importación',
  'Update preview': 'Vista previa de la actualización',
  '{count} projects': '{count} proyectos',
  '{count} workspaces': '{count} workspaces',
  '{count} live sessions': '{count} sesiones activas',
  '{placed} panes with a session, {empty} empty':
    '{placed} paneles con sesión, {empty} vacíos',
  '{count} tabs with nothing alive (skipped)':
    '{count} pestañas sin nada activo (se omiten)',
  'Things to know': 'Cosas que conviene saber',
  'I accept that spellings of the same folder become one project':
    'Acepto que distintas formas de escribir la misma carpeta se conviertan en un solo proyecto',
  'The exact older files are kept in a private snapshot first. The older state is never overwritten, and no session is started, stopped or restarted.':
    'Antes se guardan los archivos anteriores exactos en una copia privada. El estado anterior nunca se sobrescribe, y no se inicia, detiene ni reinicia ninguna sesión.',
  Import: 'Importar',
  'Folder missing': 'Falta la carpeta',
  'Folder replaced': 'La carpeta fue reemplazada',
  'Folder not usable': 'La carpeta no se puede usar',
  'Another account can write here': 'Otra cuenta puede escribir acá',
  'Add a folder…': 'Agregar una carpeta…',
  'New folder name': 'Nombre de la carpeta nueva',
  'Create…': 'Crear…',
  Scratch: 'Temporal',
  'no folder': 'sin carpeta',
  '{name} (archived)': '{name} (archivado)',
  'Show archived': 'Mostrar archivados',
  'Actions for {name}': 'Acciones para {name}',
  'Project name': 'Nombre del proyecto',
  Unarchive: 'Desarchivar',
  Archive: 'Archivar',
  'Relink folder…': 'Volver a vincular carpeta…',
  'New workspace': 'Nuevo workspace',
  'Remove, keep history': 'Quitar y conservar el historial',
  'Remove completely': 'Quitar por completo',
  "Removing a project never deletes its folder, a provider's history or a session; it is refused while any of its sessions may still be running.":
    'Quitar un proyecto nunca borra su carpeta, el historial de un proveedor ni una sesión; se rechaza mientras alguna de sus sesiones pueda seguir corriendo.',
  Workspaces: 'Workspaces',
  'Workspace {count}': 'Workspace {count}',
  'Projects and workspaces': 'Proyectos y workspaces',
  'On. Projects and workspaces are shown in their own view. Turning them off goes back to the older windows; nothing running is stopped and the project data is kept.':
    'Activado. Los proyectos y workspaces se muestran en su propia vista. Desactivarlos vuelve a las ventanas anteriores; no se detiene nada que esté corriendo y se conservan los datos de los proyectos.',
  'Off. When turned on, a project is a folder with its own workspaces and sessions. Your sessions and the older windows are not changed, and nothing is started or stopped. Turning it on is confirmed in a window of its own.':
    'Desactivado. Al activarlo, un proyecto es una carpeta con sus propios workspaces y sesiones. Tus sesiones y las ventanas anteriores no cambian, y no se inicia ni detiene nada. Activarlo se confirma en una ventana propia.',
  'Turn off…': 'Desactivar…',
  'Turn on…': 'Activar…',
  Shell: 'Shell',
  Managed: 'Managed',
  'This store was written by a newer version of the Console. It is shown as it is, read-only, and nothing was changed.':
    'Este almacén lo escribió una versión más nueva de la Consola. Se muestra tal cual, en solo lectura, y no se cambió nada.',
  'The store cannot be read as the format it claims. Its exact bytes are kept untouched and nothing was replaced by an empty one.':
    'El almacén no se puede leer con el formato que declara. Sus bytes exactos se conservan sin tocar y no se reemplazó nada por uno vacío.',
  'The store is bigger than the limit. Its exact bytes are kept untouched.':
    'El almacén supera el límite. Sus bytes exactos se conservan sin tocar.',
  'Managed {id}': 'Managed {id}',
  'Agent {id}': 'Agente {id}',
  'Shell {id}': 'Shell {id}',
  'Projects could not be read': 'No se pudieron leer los proyectos',
  'Projects and workspaces are off':
    'Los proyectos y workspaces están desactivados',
  'They can be turned on in Settings. Until then nothing about them is read or run, and your sessions and the older windows are untouched.':
    'Se pueden activar en Ajustes. Hasta entonces no se lee ni se ejecuta nada de ellos, y tus sesiones y las ventanas anteriores no se tocan.',
  'The projects store is read-only':
    'El almacén de proyectos es de solo lectura',
  'Reason: {reason} ({field}). Your sessions and the older windows are not affected.':
    'Motivo: {reason} ({field}). Tus sesiones y las ventanas anteriores no se ven afectadas.',
  'No place to keep projects': 'No hay dónde guardar los proyectos',
  'The Console has no state directory to keep them in.':
    'La Consola no tiene una carpeta de estado donde guardarlos.',
  'No terminal host answers. Sessions are shown as unknown, and nothing is started or recreated.':
    'No responde ningún host de terminales. Las sesiones se muestran como desconocidas, y no se inicia ni se recrea nada.',
  'Unassigned sessions': 'Sesiones sin asignar',
  'Sessions of {name}': 'Sesiones de {name}',
  '{name} is in {folder}.': '{name} está en {folder}.',
  'That is inside {project}.': 'Eso está dentro de {project}.',
  'That is outside the folder of {project}.':
    'Eso está fuera de la carpeta de {project}.',
  'Move it into {project}? It keeps running where it is; only the project it is listed under changes.':
    '¿Moverla a {project}? Sigue corriendo donde está; solo cambia el proyecto bajo el que aparece.',
  'None. History that is not shown in a pane stays here.':
    'Ninguna. El historial que no se muestra en un panel queda acá.',
  'Show in an empty pane': 'Mostrar en un panel vacío',
  'Adopt {name} into a project': 'Incorporar {name} a un proyecto',
  'Adopt into a project…': 'Incorporar a un proyecto…',
  'Import, receipts and going back': 'Importación, comprobantes y cómo volver',
  'Compact finished launch receipts':
    'Compactar los comprobantes de lanzamientos terminados',
  'What going back needs': 'Qué necesita volver atrás',
  'Open another window': 'Abrir otra ventana',
  'Nothing is unaccounted for; projects can be turned off in Settings.':
    'No queda nada sin registrar; los proyectos se pueden desactivar en Ajustes.',
  '{count} launch(es) of unknown outcome must be resolved first.':
    'Primero hay que resolver {count} lanzamiento(s) de resultado desconocido.',
  'Workspace {name}': 'Workspace {name}',
  'Add a pane': 'Agregar un panel',
  'Split top and bottom': 'Dividir arriba y abajo',
  'A launch is in progress': 'Hay un lanzamiento en curso',
  'A launch has an unknown outcome':
    'Un lanzamiento tiene resultado desconocido',
  'Nothing was started again. If it started something, pick it from what is running and not shown anywhere; if nothing started, say so. Nothing here retries.':
    'No se volvió a iniciar nada. Si inició algo, elegilo entre lo que corre y no se muestra en ningún lado; si no inició nada, decilo. Acá no se reintenta nada.',
  'Look for what it started': 'Buscar lo que inició',
  'Nothing is running that is not already shown.':
    'No hay nada corriendo que no esté ya mostrado.',
  'unknown folder': 'carpeta desconocida',
  'This one': 'Este',
  'Nothing started': 'No se inició nada',
  'Ask the Managed owner again': 'Volver a preguntarle al dueño de Managed',
  'Empty pane. Nothing runs here.': 'Panel vacío. Acá no corre nada.',
  'Start…': 'Iniciar…',
  'Remove pane': 'Quitar panel',
  'Resize panes': 'Redimensionar paneles',
  'This workspace has no panes. Add one to start something in it.':
    'Este workspace no tiene paneles. Agregá uno para iniciar algo en él.',
  'Changed somewhere else. The view was refreshed; try again.':
    'Cambió en otro lado. Se actualizó la vista; probá de nuevo.',
  'That no longer matches what is shown. The view was refreshed; nothing was changed.':
    'Eso ya no coincide con lo que se muestra. Se actualizó la vista; no se cambió nada.',
  'This pane is busy with a launch.':
    'Este panel está ocupado con un lanzamiento.',
  'The folder is missing.': 'Falta la carpeta.',
  'The folder was replaced by another one.':
    'La carpeta fue reemplazada por otra.',
  'That folder cannot be used.': 'Esa carpeta no se puede usar.',
  'No terminal host answers. Nothing was started.':
    'No responde ningún host de terminales. No se inició nada.',
  'The terminal host is not the one this session lived in. Nothing was sent.':
    'El host de terminales no es el que alojaba esta sesión. No se envió nada.',
  'Not available in this version or for this choice.':
    'No disponible en esta versión o para esta elección.',
  'That profile is no longer available.': 'Ese perfil ya no está disponible.',
  'The owner of that session refused. Nothing was changed.':
    'El dueño de esa sesión lo rechazó. No se cambió nada.',
  'The outcome is unknown. Nothing was started again; resolve it below.':
    'El resultado es desconocido. No se volvió a iniciar nada; resolvelo abajo.',
  'The store could not be written.': 'No se pudo escribir el almacén.',
  'A limit was reached. Nothing was changed.':
    'Se alcanzó un límite. No se cambió nada.',
  'That change is not valid.': 'Ese cambio no es válido.',
  'The store is from a newer version and is read-only.':
    'El almacén es de una versión más nueva y es de solo lectura.',
  'Another account can write in that folder, or that could not be checked. It is not used.':
    'Otra cuenta puede escribir en esa carpeta, o no se pudo comprobar. No se usa.',
  'That shell was not shown to you, or it is not where it was shown. Nothing was moved.':
    'Ese shell no se te mostró, o no está donde se mostró. No se movió nada.',
  'Where that shell is cannot be read, so nothing was moved.':
    'No se puede leer dónde está ese shell, así que no se movió nada.',
  'That Managed authority has not shown it serves that profile. Nothing was started.':
    'Esa autoridad Managed no mostró que atienda ese perfil. No se inició nada.',
  'The Managed authority is not the one that was asked first. Nothing was sent to this one; the first attempt is still unknown.':
    'La autoridad Managed no es la que se consultó primero. No se envió nada a esta; el primer intento sigue siendo desconocido.',
  'Not done.': 'No se hizo.',
  'Another change is still being sent.':
    'Otro cambio todavía se está enviando.',
  'this host keeps no screen': 'este host no guarda pantalla',
  'the screen helper is not running':
    'el auxiliar de pantalla no está corriendo',
  'the screen was too large to send':
    'la pantalla era demasiado grande para enviarla',
  'the screen helper did not answer in time':
    'el auxiliar de pantalla no respondió a tiempo',
  'the host did not answer': 'el host no respondió',
  'the screen could not be used': 'no se pudo usar la pantalla',
  'the screen could not be drawn here': 'no se pudo dibujar la pantalla acá',
  'the host or session changed while it was drawn':
    'el host o la sesión cambiaron mientras se dibujaba',
  'the screen could not be asked for': 'no se pudo pedir la pantalla',
  'The terminal host is not the one this session lived in.':
    'El host de terminales no es el que alojaba esta sesión.',
  'No terminal host answers.': 'No responde ningún host de terminales.',
  'The terminal host refused that.': 'El host de terminales rechazó eso.',
  'Not available for this session.': 'No disponible para esta sesión.',
  'The terminal could not be drawn.': 'No se pudo dibujar la terminal.',
  'Another window holds input.': 'Otra ventana tiene la entrada.',
  'Reading stopped.': 'Se dejó de leer.',
  'Input was not sent.': 'No se envió la entrada.',
  'Replayed from the output the host kept ({reason}). A full-screen program may look incomplete until it redraws.':
    'Reproducido desde la salida que guardó el host ({reason}). Un programa a pantalla completa puede verse incompleto hasta que vuelva a dibujarse.',
  'Release input': 'Soltar la entrada',
  'Another window has input': 'Otra ventana tiene la entrada',
  'Take input': 'Tomar la entrada',
  Detach: 'Desconectar',
  '{name} terminal': 'Terminal de {name}',
  'Earlier output is no longer held.': 'La salida anterior ya no se guarda.',
  'Read-only view. Input is exclusive; take it to type.':
    'Vista de solo lectura. La entrada es exclusiva; tomala para escribir.',
  'Read-only.': 'Solo lectura.',
  'Protocol {protocol} · stream': 'Protocolo {protocol} · stream',
  'Requested profile': 'Perfil pedido',
  'not recorded': 'no registrado',
  'Attached profile': 'Perfil adjunto',
  'not attached or unknown': 'no adjunto o desconocido',
  'The authority attached a different profile than the one requested. The request grants nothing; open the Managed screens to decide.':
    'La autoridad adjuntó un perfil distinto al pedido. El pedido no otorga nada; abrí las pantallas de Managed para decidir.',
  'A Managed session is opened and run in Sessions, Agents and Runtimes. Showing it here starts nothing and grants nothing.':
    'Una sesión de Managed se abre y corre en Sesiones, Agentes y Runtimes. Mostrarla acá no inicia nada y no otorga nada.',
  'Closing this window': 'Cerrar esta ventana',
  'Leaving a terminal (detach)': 'Salir de una terminal (desconectar)',
  'Locking the screen': 'Bloquear la pantalla',
  'Logging out, lingering on': 'Cerrar sesión, con lingering activado',
  'Logging out, lingering off': 'Cerrar sesión, con lingering desactivado',
  'Suspend and resume': 'Suspender y reanudar',
  'The host stopping or crashing': 'El host se detiene o se cae',
  'Reboot or power-off': 'Reinicio o apagado',
  'Keeps running': 'Sigue corriendo',
  Ends: 'Termina',
  'Not certain here': 'No está claro acá',
  'Lingering is on for your account.':
    'Lingering está activado para tu cuenta.',
  'Lingering is off for your account.':
    'Lingering está desactivado para tu cuenta.',
  'This system does not expose a lingering setting the Console can read.':
    'Este sistema no expone una opción de lingering que la Consola pueda leer.',
  'The setting could not be changed. Nothing was changed.':
    'No se pudo cambiar la opción. No se cambió nada.',
  'Terminals after the window closes':
    'Terminales después de cerrar la ventana',
  'What survives what could not be read from the host.':
    'No se pudo leer del host qué sobrevive a qué.',
  'Your terminals belong to a host process, not to this window. Closing the window does not stop them; this is what happens in the other cases on this machine.':
    'Tus terminales pertenecen a un proceso host, no a esta ventana. Cerrar la ventana no las detiene; esto es lo que pasa en los otros casos de esta máquina.',
  'What survives what': 'Qué sobrevive a qué',
  When: 'Cuándo',
  Why: 'Por qué',
  'Lingering keeps your user session manager running after you log out. It is {state}. The Console never turns it on by itself.':
    'Lingering mantiene el administrador de tu sesión de usuario corriendo después de cerrar sesión. Está {state}. La Consola nunca lo activa por sí sola.',
  'not something this system lets the Console read':
    'algo que este sistema no deja leer a la Consola',
  on: 'activado',
  off: 'desactivado',
  'Confirm lingering': 'Confirmar lingering',
  'This runs “loginctl enable-linger” for your account: your user services will start at boot and stay after you log out. You can turn it off here at any time.':
    'Esto ejecuta “loginctl enable-linger” para tu cuenta: tus servicios de usuario arrancarán al iniciar y seguirán después de cerrar sesión. Podés desactivarlo acá en cualquier momento.',
  'This runs “loginctl disable-linger” for your account: your user manager will stop when you log out, and anything running inside it with it.':
    'Esto ejecuta “loginctl disable-linger” para tu cuenta: el administrador de usuario se detendrá al cerrar sesión, y con él todo lo que corra dentro.',
  'Turn lingering on': 'Activar lingering',
  'Turn lingering off': 'Desactivar lingering',
  'Turn lingering off…': 'Desactivar lingering…',
  'Keep terminals after logout…':
    'Mantener las terminales después de cerrar sesión…',
  'The launch could not be opened.': 'No se pudo abrir el lanzamiento.',
  starting: 'iniciando',
  running: 'en curso',
  closing: 'cerrando',
  exited: 'terminó',
  closed: 'cerrada',
  failed: 'falló',
  'Terminal sessions': 'Sesiones de terminal',
  Launches: 'Lanzamientos',
  'New terminal': 'Nueva terminal',
  'Start agent…': 'Iniciar agente…',
  'Reading launches…': 'Leyendo lanzamientos…',
  'No launches. Add a file in {folder}.':
    'No hay lanzamientos. Agregá un archivo en {folder}.',
  'Full access': 'Acceso total',
  'Move {name} up': 'Mover {name} hacia arriba',
  'Move {name} down': 'Mover {name} hacia abajo',
  'Files in {folder}': 'Archivos en {folder}',
  Done: 'Listo',
  Order: 'Orden',
  'Every other key goes to the program in the terminal.':
    'Cualquier otra tecla va al programa de la terminal.',
  'No terminals open. Create a new terminal to continue.':
    'No hay terminales abiertas. Creá una nueva terminal para seguir.',
  'New terminal in a new tab': 'Nueva terminal en una pestaña nueva',
  'Split the pane side by side': 'Dividir el panel lado a lado',
  'Split the pane, one above the other':
    'Dividir el panel, una arriba de la otra',
  'Close the pane': 'Cerrar el panel',
  'Search the pane’s scrollback':
    'Buscar en el historial de desplazamiento del panel',
  'Move to the pane in that direction': 'Ir al panel en esa dirección',
  'Next and previous pane': 'Panel siguiente y anterior',
  'Next and previous tab': 'Pestaña siguiente y anterior',
  'Go to that tab': 'Ir a esa pestaña',
  'A command before the start did not succeed ({result}); the agent started anyway.':
    'Un comando previo al inicio no tuvo éxito ({result}); el agente arrancó igual.',
  'Started with {selection} applied.': 'Iniciado con {selection} aplicado.',
  'Started without applying {selection}; the selection is recorded.':
    'Iniciado sin aplicar {selection}; la selección queda registrada.',
  'The terminal started, but Arsenal could not record its selection.':
    'La terminal inició, pero Arsenal no pudo registrar su selección.',
  'Screen restored · {count} KiB of output was missed while it was kept':
    'Pantalla restaurada · se perdieron {count} KiB de salida mientras se guardaba',
  'The program printed faster than the screen could be kept, so part of its output never reached it. What is drawn may differ from the program until the program draws again.':
    'El programa imprimió más rápido de lo que se podía guardar la pantalla, así que parte de su salida nunca llegó. Lo dibujado puede diferir del programa hasta que el programa vuelva a dibujar.',
  'Screen rebuilt from the end of the output · {count} KiB before it is not included':
    'Pantalla reconstruida desde el final de la salida · {count} KiB anteriores no están incluidos',
  'The parser that keeps this screen was restarted and only the output the host still held was replayed into it. State set earlier (an alternate screen, a mode) may be missing until the program draws again.':
    'Se reinició el analizador que guarda esta pantalla y solo se reprodujo en él la salida que todavía tenía el host. El estado definido antes (una pantalla alternativa, un modo) puede faltar hasta que el programa vuelva a dibujar.',
  'Screen restored · {count} older lines not shown':
    'Pantalla restaurada · {count} líneas anteriores no se muestran',
  'The screen and its recent history were redrawn from the host. Older history was past what is kept.':
    'La pantalla y su historial reciente se volvieron a dibujar desde el host. El historial más viejo quedó fuera de lo que se guarda.',
  'Output replayed · no screen state was kept':
    'Salida reproducida · no se guardó estado de pantalla',
  'The host had no screen for this terminal ({reason}), so what it kept of the output was replayed and the program was asked to draw again.':
    'El host no tenía pantalla para esta terminal ({reason}), así que lo que guardó de la salida se reprodujo y se le pidió al programa que volviera a dibujar.',
  'Output was missed while the screen was kept':
    'Se perdió salida mientras se guardaba la pantalla',
  'The program printed faster than the screen could be kept. The terminal itself is complete; a window opened later may show a screen that differs until the program draws again.':
    'El programa imprimió más rápido de lo que se podía guardar la pantalla. La terminal en sí está completa; una ventana abierta después puede mostrar una pantalla distinta hasta que el programa vuelva a dibujar.',
  'Screen parser restarting': 'Reiniciando el analizador de pantalla',
  'The terminal itself is not affected. The screen state is being rebuilt from the output the host still holds.':
    'La terminal en sí no se ve afectada. El estado de pantalla se está reconstruyendo desde la salida que todavía tiene el host.',
  'Screen parser unavailable': 'Analizador de pantalla no disponible',
  'The terminal works. If this window is closed and opened again, the output the host kept is replayed instead of a saved screen.':
    'La terminal funciona. Si esta ventana se cierra y se vuelve a abrir, se reproduce la salida que guardó el host en lugar de una pantalla guardada.',
  'Started by the Console in your shell. Not mediated, contained or approved by the Console.':
    'Iniciada por la Consola en tu shell. La Consola no la media, no la contiene ni la aprueba.',
  'Starting agent…': 'Iniciando agente…',
  'Starting terminal…': 'Iniciando terminal…',
  'Agent exited': 'El agente terminó',
  'Shell exited': 'El shell terminó',
  'Closing terminal…': 'Cerrando terminal…',
  'Terminal closed': 'Terminal cerrada',
  'Terminal failed: {reason}': 'La terminal falló: {reason}',
  'Output truncated': 'Salida recortada',
  'Search this terminal': 'Buscar en esta terminal',
  'Search…': 'Buscar…',
  'No matches': 'Sin coincidencias',
  '{current} of {total}{suffix}': '{current} de {total}{suffix}',
  'Previous match': 'Coincidencia anterior',
  'Next match': 'Coincidencia siguiente',
  'Match case': 'Distinguir mayúsculas',
  'Close search': 'Cerrar búsqueda',
  'The terminal size was refused.': 'Se rechazó el tamaño de la terminal.',
  'The folder to start in does not exist or cannot be opened.':
    'La carpeta de inicio no existe o no se puede abrir.',
  'The shell could not be started.': 'No se pudo iniciar el shell.',
  'Input could not be written to the terminal.':
    'No se pudo escribir la entrada en la terminal.',
  'The terminal could not be resized.': 'No se pudo redimensionar la terminal.',
  'The terminal stayed busy and did not take the input.':
    'La terminal siguió ocupada y no tomó la entrada.',
  'The limit of open terminals was reached. Close one first.':
    'Se alcanzó el límite de terminales abiertas. Cerrá una primero.',
  'The terminal did not answer in time.': 'La terminal no respondió a tiempo.',
  'The terminal did not close in time.': 'La terminal no se cerró a tiempo.',
  'The terminal could not be closed.': 'No se pudo cerrar la terminal.',
  'The terminal could not be drawn in this window.':
    'No se pudo dibujar la terminal en esta ventana.',
  'The terminal host is not answering. Nothing was sent.':
    'El host de terminales no responde. No se envió nada.',
  'The terminal host did not confirm this request. It may or may not have taken effect, and it was not sent again. Check the kept terminals before starting another.':
    'El host de terminales no confirmó este pedido. Puede que haya surtido efecto o no, y no se volvió a enviar. Revisá las terminales guardadas antes de iniciar otra.',
  'The terminal stopped with an error.': 'La terminal se detuvo con un error.',
  'The host did not confirm the last input: it may or may not have reached the program. It was not sent again, and input still waiting was dropped. Check the screen before typing on.':
    'El host no confirmó la última entrada: puede que haya llegado al programa o no. No se volvió a enviar, y la entrada que seguía esperando se descartó. Revisá la pantalla antes de seguir escribiendo.',
  'That input was not sent: with what is already waiting it is more than a terminal holds (64 KiB). Nothing of it reached the program; what was waiting before it still goes.':
    'Esa entrada no se envió: con lo que ya está esperando, supera lo que cabe en una terminal (64 KiB). Nada de ella llegó al programa; lo que esperaba antes sigue su curso.',
  'Preset saved': 'Preset guardado',
  'Could not be saved. Nothing changed.': 'No se pudo guardar. No cambió nada.',
  Presets: 'Presets',
  'New preset': 'Nuevo preset',
  'Reading presets…': 'Leyendo presets…',
  'No presets for {provider} yet.': 'Todavía no hay presets para {provider}.',
  Duplicate: 'Duplicar',
  'Delete {name}?': '¿Borrar {name}?',
  'Deleting…': 'Borrando…',
  'Copy of {name}': 'Copia de {name}',
  Choose: 'Elegir',
  'Search capabilities': 'Buscar capacidades',
  Rules: 'Reglas',
  Family: 'Familia',
  'No capabilities match.': 'Ninguna capacidad coincide.',
  'Move to group…': 'Mover a grupo…',
  'Shows everything at once, of every type.':
    'Muestra todo junto, de todos los tipos.',
  'A hook is a script that runs by itself at a moment of the session, such as before a command or when it starts. It can stop a command or add context, without being asked.':
    'Un hook es un script que corre solo en un momento de la sesión, como antes de un comando o al arrancar. Puede frenar un comando o sumar contexto sin que se lo pidas.',
  'An MCP server connects the agent to an outside tool, such as Notion, a browser or library docs, and gives it new actions to use.':
    'Un servidor MCP conecta al agente con una herramienta de afuera, como Notion, un navegador o la documentación de una librería, y le da acciones nuevas para usar.',
  'A plugin is a package that brings several pieces together (skills, agents, hooks, MCP servers) and is switched on or off as a whole.':
    'Un plugin es un paquete que trae varias piezas juntas (skills, agentes, hooks, servidores MCP) y se prende o apaga entero.',
  'A command is a /name you type to run a saved prompt. Here only your own command files are listed; most /commands are skills.':
    'Un comando es un /nombre que escribís para correr un prompt guardado. Acá se listan solo tus archivos de comandos; la mayoría de los /comandos son skills.',
  MCP: 'MCP',
  Commands: 'Comandos',
  'Part of {name}': 'Parte de {name}',
  'No custom commands. Your /commands are skills here.':
    'No tenés comandos propios. Acá tus /comandos son skills.',
  'Shown, not selectable yet': 'Para ver, todavía no se eligen',
  'Hooks, MCP servers, plugins and commands will be chosen per preset in a later step. GSD Core agents go on and off with Keep GSD Core.':
    'Hooks, servidores MCP, plugins y comandos se van a elegir por preset en un paso siguiente. Los agentes de GSD Core se prenden y apagan con Mantener GSD Core.',
  'A rule is a file of permanent instructions, such as invariantes.md. The agent reads it when every session starts and follows it the whole time. It does not run anything.':
    'Una regla es un archivo de instrucciones permanentes, como invariantes.md. El agente lo lee al arrancar cada sesión y lo cumple todo el tiempo. No ejecuta nada.',
  'A skill is a how-to manual for one kind of task, such as designing a screen or finding a bug. The agent opens it only when the task you give it needs it.':
    'Una skill es un manual de cómo hacer un tipo de tarea, como diseñar una pantalla o encontrar un error. El agente la abre solo cuando la tarea que le das la necesita.',
  'An agent is a helper with a job of its own, such as reviewing code. The main agent hands it part of the work; it does it on its own and returns the result.':
    'Un agente es un ayudante con un trabajo propio, como revisar código. El agente principal le pasa una parte del trabajo; él la hace por su cuenta y devuelve el resultado.',
  'Only changes where it is listed on this screen, not what it does. Saved on this computer.':
    'Solo cambia dónde aparece en esta pantalla, no lo que hace. Se guarda en esta computadora.',
  'Back to automatic group': 'Volver al grupo automático',
  'No description in its file': 'Su archivo no trae descripción',
  Rule: 'Regla',
  Supported: 'Compatible',
  Unverified: 'Sin verificar',
  'Needs provider qualification': 'Necesita calificación del provider',
  'Missing dependency': 'Falta una dependencia',
  Conflicting: 'En conflicto',
  Unsupported: 'No compatible',
  'Models and rules': 'Modelos y reglas',
  'Rules the agent always follows, and the Codex model profiles (astra, luna, sol) that choose which model and effort a task runs with.':
    'Reglas que el agente cumple siempre, y los perfiles de modelo de Codex (astra, luna, sol) que eligen con qué modelo y esfuerzo corre una tarea.',
  'Checks for security problems: audits, threat reviews and guards against dangerous commands.':
    'Controles de seguridad: auditorías, revisión de amenazas y frenos ante comandos peligrosos.',
  'Finding why something fails: method to chase a bug down to its cause.':
    'Encontrar por qué algo falla: método para rastrear un error hasta su causa.',
  'Writing for the web and social media, and search ranking (SEO).':
    'Escribir para la web y redes, y posicionamiento en buscadores (SEO).',
  'Animation, 3D and video.': 'Animación, 3D y video.',
  'Designing screens: visual direction, components, accessibility and turning a mockup into code.':
    'Diseñar pantallas: dirección visual, componentes, accesibilidad y pasar un mockup a código.',
  'Learning and notes: tutors, quizzes, second brain and saved links.':
    'Estudio y notas: tutores, cuestionarios, segundo cerebro y links guardados.',
  'Making a decision: arguing against an idea before you commit to it.':
    'Tomar decisiones: atacar una idea antes de comprometerte con ella.',
  'Looking things up: documentation, the web and mapping a codebase.':
    'Investigar: documentación, la web y mapear un código.',
  'Checking work: code review, tests, verification and quality checks.':
    'Revisar trabajo: revisión de código, tests, verificación y controles de calidad.',
  'Writing and converting documents: docs, reports, prompts and PDFs.':
    'Escribir y convertir documentos: docs, informes, prompts y PDF.',
  'Planning the work: specs, roadmaps, plans and the method that orders them.':
    'Planificar el trabajo: specs, roadmaps, planes y el método que los ordena.',
  'Writing the code: executing plans, fixing findings and shipping.':
    'Escribir el código: ejecutar planes, aplicar arreglos y publicar.',
  'This computer and the tools themselves: setup, health checks and upgrades.':
    'Esta computadora y las propias herramientas: configuración, chequeos de salud y actualizaciones.',
  'What no rule placed. Use Move to group… to put it where it belongs.':
    'Lo que ninguna regla ubicó. Usá Mover a grupo… para ponerlo donde va.',
  Debugging: 'Depuración',
  'Content and SEO': 'Contenido y SEO',
  'Animation and video': 'Animación y video',
  'Design and interface': 'Diseño e interfaz',
  'Study and knowledge': 'Estudio y conocimiento',
  Decisions: 'Decisiones',
  Research: 'Investigación',
  'Review and testing': 'Revisión y pruebas',
  'Docs and writing': 'Documentación y escritura',
  'Planning and method': 'Planificación y método',
  Building: 'Construcción',
  'System and setup': 'Sistema y entorno',
  'How it is put together': 'Cómo está armado',
  'Name the preset': 'Poné un nombre al preset',
  'Nothing chosen yet. Tick a group or a capability.':
    'Todavía no elegiste nada. Marcá un grupo o una capacidad.',
  'Checking…': 'Verificando…',
  'Could not be checked.': 'No se pudo verificar.',
  '{on} on · {off} off': '{on} activas · {off} apagadas',
  'Keep {name} ({count} skills)': 'Mantener {name} ({count} skills)',
  '{count} conflict': '{count} conflicto',
  '{count} conflicts': '{count} conflictos',
  'Same name': 'Mismo nombre',
  'Same content': 'Mismo contenido',
  'Two rules for one provider': 'Dos reglas para un mismo provider',
  'Two methodologies': 'Dos metodologías',
  'Installed in account and project': 'Instalada en la cuenta y en el proyecto',
  'How it starts': 'Cómo arranca',
  '{agent} starts with {on} capabilities from this preset. The other {off} installed skills are switched off in that terminal.':
    '{agent} arranca con {on} capacidades de este preset. Las otras {off} skills instaladas quedan apagadas en esa terminal.',
  'Save preset': 'Guardar preset',
  'Discard changes': 'Descartar cambios',
  'Keep editing': 'Seguir editando',
  'Unsaved changes': 'Cambios sin guardar',
  'Save as launch…': 'Guardar como lanzamiento…',
  'Save as launch': 'Guardar como lanzamiento',
  'Launch name': 'Nombre del lanzamiento',
  'No profile — as configured': 'Sin perfil — como está configurado',
  'None — all installed skills': 'Ninguno — todas las skills instaladas',
  'A launch with that name exists.': 'Ya hay un lanzamiento con ese nombre.',
  Replace: 'Reemplazar',
  'Save launch': 'Guardar lanzamiento',
  'The launch could not be saved.': 'No se pudo guardar el lanzamiento.',
  'Launch saved as “{name}”. Find it in Launch ▾.':
    'Lanzamiento guardado como “{name}”. Está en Launch ▾.',
  'No profile named “{name}” for this agent. Create it in Profiles, or change the launch file.':
    'No hay un perfil llamado “{name}” para este agente. Créalo en Perfiles o cambia el archivo del lanzamiento.',
  'No preset named “{name}” for this agent. Create it in Arsenal, or change the launch file.':
    'No hay un preset llamado “{name}” para este agente. Créalo en Arsenal o cambia el archivo del lanzamiento.',
  'More than one preset is named “{name}” for this agent. Delete the extra ones in Arsenal, then start again.':
    'Hay más de un preset llamado “{name}” para este agente. Borra los sobrantes en Arsenal y vuelve a iniciar.',
  'Saved launches': 'Lanzamientos guardados',
  'No saved launches yet.': 'Todavía no hay lanzamientos guardados.',
  'The saved launches did not load.':
    'Los lanzamientos guardados no se cargaron.',
  'The launch could not be deleted.': 'No se pudo borrar el lanzamiento.',
  'Starting…': 'Iniciando…',
  'Terminal commands · {folder}': 'Comandos de terminal · {folder}',
  'Preset {name}': 'Preset {name}',
  Preset: 'Preset',
  'Edit in Arsenal': 'Editar en Arsenal',
  'No presets for {agent}.': 'No hay presets para {agent}.',
  // ---- m3 ----
  Available: 'Disponible',
  Blocked: 'Bloqueado',
  'final figures from the provider': 'cifras finales del proveedor',
  'the provider reported an error': 'el proveedor informó un error',
  'interrupted: streaming figures only':
    'interrumpido: solo cifras de streaming',
  'the provider reported nothing': 'el proveedor no informó nada',
  '5-hour window': 'Ventana de 5 horas',
  '7-day window': 'Ventana de 7 días',
  '7-day window (Opus)': 'Ventana de 7 días (Opus)',
  '7-day window (Sonnet)': 'Ventana de 7 días (Sonnet)',
  Overage: 'Excedente',
  'Waiting for your decision': 'Esperando tu decisión',
  'Accepted — running': 'Aceptada — en ejecución',
  'Declined — nothing ran': 'Rechazada — no se ejecutó nada',
  'Cancelled — nothing ran': 'Cancelada — no se ejecutó nada',
  'Could not be done — nothing changed': 'No se pudo hacer — no cambió nada',
  'Unknown whether it happened — it will not be repeated':
    'No se sabe si pasó — no se va a repetir',
  'Refused before asking you': 'Rechazada antes de consultarte',
  'A fixture runtime is already running.':
    'Ya hay un runtime simulado corriendo.',
  'There is no running fixture runtime.':
    'No hay ningún runtime simulado corriendo.',
  'The runtime is not ready for a prompt.':
    'El runtime todavía no está listo para un mensaje.',
  'The prompt is empty or too long.':
    'El mensaje está vacío o es demasiado largo.',
  'No turn is running.': 'No hay ningún turno en curso.',
  'That action is not waiting for a decision.':
    'Esa acción no está esperando una decisión.',
  'That is not a decision.': 'Eso no es una decisión.',
  'The worker refused the decision; it is not retried.':
    'El worker rechazó la decisión; no se reintenta.',
  'The runtime refused the turn.': 'El runtime rechazó el turno.',
  'The runtime could not be started.': 'No se pudo iniciar el runtime.',
  'That profile does not exist.': 'Ese perfil no existe.',
  'The decision this needs has not been accepted.':
    'La decisión que hace falta no se aceptó.',
  'The window could not reach the service.':
    'La ventana no pudo llegar al servicio.',
  'The decision on Claude Managed security and authentication has not been accepted.':
    'No se aceptó la decisión sobre seguridad y autenticación de Claude Managed.',
  'The authority’s store only accepts Codex runtimes; holding a Claude runtime needs a store change the owner has not approved.':
    'El almacén de la autoridad solo acepta runtimes de Codex; tener un runtime de Claude necesita un cambio de almacén que el dueño no aprobó.',
  'No authorized run against the real provider has been recorded for this profile.':
    'No hay registro de una corrida autorizada contra el proveedor real para este perfil.',
  'How a provider key would reach a Claude runtime has not been decided; keys stay sealed and none is delivered.':
    'Todavía no se decidió cómo llegaría la clave del proveedor a un runtime de Claude; las claves siguen selladas y no se entrega ninguna.',
  'No reviewer other than the author has inspected this runtime and its network confinement.':
    'Ningún revisor distinto del autor revisó este runtime ni su aislamiento de red.',
  'Choosing a model or reasoning effort for a Managed turn needs a decision the owner has not accepted.':
    'Elegir un modelo o un esfuerzo de razonamiento para un turno Managed necesita una decisión que el dueño no aceptó.',
  'The privacy and retention policy for a usage history has not been accepted.':
    'No se aceptó la política de privacidad y retención del historial de uso.',
  'No Claude Agent SDK installation was named for this Console.':
    'No se indicó ninguna instalación del Claude Agent SDK para esta Consola.',
  'The protocol-2 authority is not connected.':
    'La autoridad de protocolo 2 no está conectada.',
  'This platform has no accepted evidence.':
    'Esta plataforma no tiene evidencia aceptada.',
  'The installation is being checked.': 'Se está verificando la instalación.',
  'The SDK version is not the one this build was qualified against.':
    'La versión del SDK no es la que se calificó para esta build.',
  'The bundled Claude Code version is not the qualified one.':
    'La versión de Claude Code incluida no es la calificada.',
  'The bundled Claude Code executable is not the pinned one.':
    'El ejecutable de Claude Code incluido no es el fijado.',
  'A file of the SDK installation does not match what pip recorded.':
    'Un archivo de la instalación del SDK no coincide con lo que registró pip.',
  'The SDK installation holds a file no package vouches for.':
    'La instalación del SDK tiene un archivo que ningún paquete respalda.',
  'An administrator’s Claude Code policy exists on this machine; it can add hooks and tools no option turns off.':
    'Hay una política de Claude Code del administrador en esta máquina; puede agregar hooks y herramientas que ninguna opción apaga.',
  'The credential file is not private, plain and non-empty.':
    'El archivo de credenciales no es privado, plano ni vacío.',
  'A path of the installation is not safe to use.':
    'Una ruta de la instalación no es segura para usar.',
  'The named folder is not an isolated Python virtual environment.':
    'La carpeta indicada no es un entorno virtual de Python aislado.',
  'Keep a local usage history': 'Guardar un historial de uso local',
  'Why it cannot be turned on': 'Por qué no se puede activar',
  Off: 'Desactivado',
  Recording: 'Guardando',
  'Partial: a storage problem stopped it':
    'Parcial: un problema de almacenamiento lo detuvo',
  'It is off by default and nothing is stored while it is off. If you turn it on it keeps provider-reported counters and limits only: no prompts, no answers, no reasoning, no commands, no paths and no account names. At most {records} records or {days} days, whichever comes first, and what it removes is counted. It is a separate file; it never touches the audit trail or the vault.':
    'Está desactivado por defecto y, mientras esté desactivado, no se guarda nada. Si lo activás, guarda solo contadores y límites que informa el proveedor: nada de mensajes, respuestas, razonamiento, comandos, rutas ni nombres de cuenta. Como máximo {records} registros o {days} días, lo que pase antes, y lo que se quita se cuenta. Es un archivo aparte; nunca toca el registro de auditoría ni la bóveda.',
  Export: 'Exportar',
  'Delete the history': 'Borrar el historial',
  'Provider profiles': 'Perfiles del proveedor',
  'No build verified on this machine.':
    'No hay build verificado en esta máquina.',
  'Why it is not available': 'Por qué no está disponible',
  Evidence: 'Evidencia',
  'Fixtures:': 'Simulado:',
  'Real provider:': 'Proveedor real:',
  'Start it from Runtimes; this view does not start anything.':
    'Iniciala desde Runtimes; esta vista no inicia nada.',
  passed: 'aprobado',
  'not run': 'sin correr',
  'not applicable': 'no aplica',
  'Managed profiles': 'Perfiles Managed',
  'Claude Agent SDK {sdk} (Claude Code {cli}) verified':
    'Claude Agent SDK {sdk} (Claude Code {cli}) verificado',
  'checking the Claude installation…': 'revisando la instalación de Claude…',
  'Claude installation refused: {reason}':
    'instalación de Claude rechazada: {reason}',
  'no Claude installation named': 'no se indicó ninguna instalación de Claude',
  'Reading the providers…': 'Leyendo los proveedores…',
  'What is shown was read earlier and may have changed; no action is offered from it.':
    'Lo que se muestra se leyó antes y puede haber cambiado; desde acá no se ofrece ninguna acción.',
  'The provider state could not be read.':
    'No se pudo leer el estado del proveedor.',
  'Claude runtime (fixture)': 'Runtime de Claude (simulado)',
  'A profile can start only when it is available. “Blocked” names the decision or evidence that is missing; fixtures never stand in for a run against the real provider.':
    'Un perfil solo puede iniciarse cuando está disponible. “Bloqueado” indica la decisión o la evidencia que falta; los simulados nunca reemplazan una corrida contra el proveedor real.',
  'Model and effort': 'Modelo y esfuerzo',
  'Usage and limits': 'Uso y límites',
  'Usage history': 'Historial de uso',
  'No Claude runtime is running, so there are no model facts to show. A listed model is offered, not entitled, and a request is not proof of use: when a runtime runs, this view keeps what is offered, configured, requested and reported in use apart.':
    'No hay ningún runtime de Claude corriendo, así que no hay datos del modelo para mostrar. Un modelo listado se ofrece, no está habilitado, y un pedido no prueba que se use: cuando un runtime corre, esta vista mantiene separados lo ofrecido, lo configurado, lo pedido y lo informado en uso.',
  Offered: 'Ofrecidos',
  Configured: 'Configurado',
  Requested: 'Pedido',
  'The provider’s own list · revision {revision} · {freshness} · taken {time}':
    'Lista propia del proveedor · revisión {revision} · {freshness} · tomada {time}',
  fresh: 'fresca',
  stale: 'vieja',
  incomplete: 'incompleta',
  'Models offered': 'Modelos ofrecidos',
  'Efforts it lists: {efforts}': 'Esfuerzos que lista: {efforts}',
  'Offered is not entitled: nothing here says this account may use a model.':
    'Lo ofrecido no es habilitación: nada de acá dice que esta cuenta pueda usar un modelo.',
  'Not listed yet.': 'Todavía no hay lista.',
  'Effort: {effort}': 'Esfuerzo: {effort}',
  'the provider default': 'el predeterminado del proveedor',
  'Nothing was pinned when the runtime started: the provider’s default.':
    'No se fijó nada al arrancar el runtime: el predeterminado del proveedor.',
  'For turn {turn} · generation {generation}':
    'Para el turno {turn} · generación {generation}',
  'No model was requested for the latest turn.':
    'No se pidió ningún modelo para el último turno.',
  'The provider has not named a model yet.':
    'El proveedor todavía no nombró ningún modelo.',
  'Model for the next turn': 'Modelo para el próximo turno',
  'The provider’s default': 'El predeterminado del proveedor',
  'Reasoning effort': 'Esfuerzo de razonamiento',
  'Fixed when the runtime starts': 'Fijo al arrancar el runtime',
  'Why model selection is off':
    'Por qué la selección de modelo está desactivada',
  'Claude’s SDK can change the model for a turn, but reads the effort once, when its runtime starts. A request that cannot be honored is refused; it is never replaced by another model.':
    'El SDK de Claude puede cambiar el modelo de un turno, pero lee el esfuerzo una sola vez, al arrancar su runtime. Un pedido que no se puede cumplir se rechaza; nunca se reemplaza por otro modelo.',
  'No model was requested for this turn.':
    'No se pidió ningún modelo para este turno.',
  'The provider has not named the model that answered.':
    'El proveedor no nombró el modelo que respondió.',
  'The provider named the model that was requested.':
    'El proveedor nombró el modelo que se pidió.',
  'Requested {requested}; the provider named {reported}.':
    'Se pidió {requested}; el proveedor nombró {reported}.',
  'No effort was requested.': 'No se pidió ningún esfuerzo.',
  'Requested {value}. Nothing shows the provider received it.':
    'Se pidió {value}. Nada muestra que el proveedor lo haya recibido.',
  '{value} was in the request the provider received. That is not proof it took effect.':
    '{value} estaba en el pedido que recibió el proveedor. Eso no prueba que haya tenido efecto.',
  'The provider reported {value}.': 'El proveedor informó {value}.',
  Input: 'Entrada',
  Output: 'Salida',
  'Cache write': 'Escritura de caché',
  'Cache read': 'Lectura de caché',
  Reasoning: 'Razonamiento',
  allowed: 'permitido',
  'allowed warning': 'permitido con advertencia',
  rejected: 'rechazado',
  'not reported': 'sin dato',
  'in {count}': 'entrada {count}',
  'out {count}': 'salida {count}',
  'cache write {count}': 'escritura de caché {count}',
  'cache read {count}': 'lectura de caché {count}',
  'reasoning {count}': 'razonamiento {count}',
  'at least {sum} ({count} turn did not report)':
    'al menos {sum} ({count} turno no informó)',
  'at least {sum} ({count} turns did not report)':
    'al menos {sum} ({count} turnos no informaron)',
  'No usage reported yet. A figure appears only when the provider reports it; one it did not report is shown as “not reported”, never as zero.':
    'Todavía no se informó uso. Una cifra aparece solo cuando el proveedor la informa; una que no informó se muestra como “sin dato”, nunca como cero.',
  'Running totals': 'Totales acumulados',
  'Running totals over {count} turn, as Claude reported them. Nothing is added across providers or sessions.':
    'Totales acumulados de {count} turno, tal como los informó Claude. No se suma nada entre proveedores ni sesiones.',
  'Running totals over {count} turns, as Claude reported them. Nothing is added across providers or sessions.':
    'Totales acumulados de {count} turnos, tal como los informó Claude. No se suma nada entre proveedores ni sesiones.',
  'Usage per turn': 'Uso por turno',
  'Answered by {models} (named by the provider)':
    'Respondió {models} (nombrado por el proveedor)',
  'Limits the provider reported': 'Límites que informó el proveedor',
  'Utilization not reported': 'Uso sin dato',
  '{percent} % used': '{percent} % usado',
  'reset time not reported': 'hora de reinicio sin dato',
  'resets {time}': 'se reinicia {time}',
  'An allowance, not a price. Reported {time}.':
    'Es un cupo, no un precio. Informado {time}.',
  'Estimate computed by the SDK': 'Estimación calculada por el SDK',
  'The Agent SDK adds a price estimate of its own to each result. It is not billing and not your subscription’s allowance, and this Console holds no price table to check it.':
    'El Agent SDK agrega una estimación de precio propia a cada resultado. No es facturación ni el cupo de tu suscripción, y esta Consola no tiene una tabla de precios para verificarla.',
  '{count} provider request checked · tools offered: {tools}':
    '{count} pedido al proveedor revisado · herramientas ofrecidas: {tools}',
  '{count} provider requests checked · tools offered: {tools}':
    '{count} pedidos al proveedor revisados · herramientas ofrecidas: {tools}',
  Refused: 'Rechazado',
  completed: 'completado',
  interrupted: 'interrumpido',
  '{label}. It drives the real Claude Agent SDK against a mock provider on this machine. Nothing here is leased, fenced or recorded by the authority, and no real provider is contacted.':
    '{label}. Maneja el Claude Agent SDK real contra un proveedor simulado en esta máquina. Nada de acá tiene concesión, fencing ni lo registra la autoridad, y no se contacta ningún proveedor real.',
  'Start Claude Inference': 'Iniciar Claude Inference',
  'Start Claude Local tools': 'Iniciar Claude Local tools',
  'Stop the runtime': 'Detener el runtime',
  'The runtime was refused and stopped before it could be used':
    'El runtime fue rechazado y se detuvo antes de poder usarse',
  'The runtime failed and was stopped': 'El runtime falló y se detuvo',
  ': {detail}': ': {detail}',
  'Send a prompt to the runtime.': 'Mandá un mensaje al runtime.',
  'Start a runtime to talk to it.': 'Iniciá un runtime para hablar con él.',
  'File change proposed by Claude': 'Cambio de archivo propuesto por Claude',
  'Command proposed by Claude': 'Comando propuesto por Claude',
  'Runs in': 'Corre en',
  ', with no network.': ', sin red.',
  'This action is not one this window can show completely, so it cannot be accepted.':
    'Esta ventana no puede mostrar esta acción completa, así que no se puede aceptar.',
  'The fingerprint the decision is bound to':
    'La huella a la que queda atada la decisión',
  'fingerprint {fingerprint}…': 'huella {fingerprint}…',
  'Nobody can say whether this happened. It will not be run again; check the workspace before asking for anything similar.':
    'Nadie puede decir si esto pasó. No se va a volver a ejecutar; revisá el workspace antes de pedir algo parecido.',
  Prompt: 'Mensaje',
  'Ask Claude something': 'Preguntale algo a Claude',
  'Start a runtime first': 'Iniciá un runtime primero',
  Send: 'Enviar',
  'Cancel the turn': 'Cancelar el turno',
  'Turn {state}': 'Turno {state}',
  // ---- native ----
  Restarting: 'Reiniciando',
  Active: 'Activa',
  'Ended by a signal': 'Terminada por una señal',
  'Exited (0)': 'Terminó (0)',
  'Did not start': 'No arrancó',
  Lost: 'Perdida',
  'The start was accepted and is being prepared.':
    'El inicio fue aceptado y se está preparando.',
  'The agent’s process is alive in its terminal.':
    'El proceso del agente está vivo en su terminal.',
  'The process ended by itself.': 'El proceso terminó solo.',
  'You stopped it.': 'La detuviste vos.',
  'The process never ran.': 'El proceso nunca corrió.',
  'The Console was restarted and did not see how it ended. Its terminal went with the window, so it is not running.':
    'La consola se reinició y no vio cómo terminó. Su terminal se fue con la ventana, así que no está corriendo.',
  'The Console could not record this start, so a process may exist. Check the sessions below. Nothing is started again automatically, and OpenCode and Hermes wait until the store is confirmed.':
    'La consola no pudo registrar este inicio, así que puede existir un proceso. Revisá las sesiones de abajo. No se vuelve a iniciar nada solo, y OpenCode y Hermes esperan hasta que se confirme el almacenamiento.',
  'The process was stopped, but the Console could not record that. Nothing was started again, and OpenCode and Hermes stay blocked until the store is confirmed.':
    'El proceso se detuvo, pero la consola no pudo registrarlo. No se volvió a iniciar nada, y OpenCode y Hermes siguen bloqueados hasta que se confirme el almacenamiento.',
  'Ask before acting': 'Preguntar antes de actuar',
  'Read only (plan)': 'Solo lectura (plan)',
  'Accept edits (Claude Code only)': 'Aceptar ediciones (solo Claude Code)',
  'Full access: no prompts, no sandbox (bypass)':
    'Acceso total: sin preguntas, sin sandbox (bypass)',
  'Full access: the agent asks nothing and runs every command and edit without a sandbox, with your account’s rights.':
    'Acceso total: el agente no pregunta nada y ejecuta cada comando y cada edición sin sandbox, con los permisos de tu cuenta.',
  'Off for this session (Claude Code only)':
    'Desactivados en esta sesión (solo Claude Code)',
  'To change the model in this session, type /model in it. Claude Code also saves your choice as its default.':
    'Para cambiar el modelo en esta sesión, escribí /model en ella. Claude Code también guarda tu elección como predeterminada.',
  'How to change the model of a running Codex session is not verified, so none is suggested.':
    'No está verificado cómo cambiar el modelo de una sesión de Codex en curso, así que no se sugiere ninguno.',
  'How to change the model in a running session is not verified, so none is suggested.':
    'No está verificado cómo cambiar el modelo en una sesión en curso, así que no se sugiere ninguno.',
  'That agent is not installed on this computer.':
    'Ese agente no está instalado en esta computadora.',
  'That folder cannot be used. Choose a folder with the folder button.':
    'Esa carpeta no se puede usar. Elegí una carpeta con el botón de carpeta.',
  'This profile cannot be applied to that agent.':
    'Este perfil no se puede aplicar a ese agente.',
  'That profile no longer exists.': 'Ese perfil ya no existe.',
  'Review the current profile preview before starting with it.':
    'Revisá la vista previa actual del perfil antes de iniciar con él.',
  'The profile changed after its preview. Review the new preview and start again.':
    'El perfil cambió después de su vista previa. Revisá la nueva vista previa y volvé a iniciar.',
  'This exact provider build is not verified for profile overrides, so the override was not launched. Choose “No profile — as configured” or use a qualified build.':
    'Esta versión exacta del proveedor no está verificada para sobrescribir perfiles, así que la sobrescritura no se lanzó. Elegí “Sin perfil — según la configuración” o usá una versión calificada.',
  'Skill exclusion is not qualified for this exact provider build, so nothing was started with it. Start without applying the selection, or use a qualified build.':
    'La exclusión de skills no está calificada para esta versión exacta del proveedor, así que no se inició nada con ella. Iniciá sin aplicar la selección, o usá una versión calificada.',
  'The Arsenal plan changed since it was shown: the library, the installed skills or the provider build moved. Review the new plan and start again.':
    'El plan de Arsenal cambió desde que se mostró: cambiaron la biblioteca, las skills instaladas o la versión del proveedor. Revisá el plan nuevo y volvé a iniciar.',
  'The Arsenal plan has items that cannot be carried or names a skill that is not installed, so nothing was started with it.':
    'El plan de Arsenal tiene elementos que no se pueden llevar o nombra una skill que no está instalada, así que no se inició nada con él.',
  'The Arsenal library or the installed skills could not be read, so nothing was started with the selection.':
    'No se pudo leer la biblioteca de Arsenal ni las skills instaladas, así que no se inició nada con la selección.',
  'That Arsenal selection no longer exists.':
    'Esa selección de Arsenal ya no existe.',
  'Arsenal does not read this project folder, so its selection cannot be applied here.':
    'Arsenal no lee esta carpeta de proyecto, así que su selección no se puede aplicar acá.',
  'Hermes Native does not have a verified interactive handoff argument. It was not started with this brief.':
    'Hermes Native no tiene un argumento de traspaso interactivo verificado. No se inició con este brief.',
  'The brief must start with a “# ” heading, hold no NUL and be at most 16 KiB.':
    'El brief tiene que empezar con un encabezado “# ”, no tener NUL y pesar 16 KiB como máximo.',
  'The terminal limit (16) is reached. Close one to start another.':
    'Se alcanzó el límite de terminales (16). Cerrá una para abrir otra.',
  'The agent could not be started.': 'No se pudo iniciar el agente.',
  'A start is already in progress.': 'Ya hay un inicio en curso.',
  'That session is not known any more.': 'Esa sesión ya no está registrada.',
  'That session is not running.': 'Esa sesión no está corriendo.',
  'A running session cannot be removed. Stop it first.':
    'No se puede quitar una sesión que corre. Detenela primero.',
  'The session could not be stopped.': 'No se pudo detener la sesión.',
  'Session storage is busy. Readiness could not be checked; it will be retried.':
    'El almacenamiento de sesiones está ocupado. No se pudo verificar la disponibilidad; se volverá a intentar.',
  'The Console has no private folder to keep native sessions in, so it cannot start agents.':
    'La consola no tiene una carpeta privada donde guardar las sesiones nativas, así que no puede iniciar agentes.',
  'The request was not valid.': 'La solicitud no era válida.',
  'The terminal host is not answering. Nothing was sent; nothing changed.':
    'El host de terminales no responde. No se envió nada; no cambió nada.',
  'The terminal host did not confirm this request: it may or may not have taken effect, and it was not sent again. Check the sessions listed here before repeating it, so nothing runs twice.':
    'El host de terminales no confirmó esta solicitud: puede que haya surtido efecto o no, y no se volvió a enviar. Revisá las sesiones listadas acá antes de repetirla, para que nada corra dos veces.',
  'The terminal host is older than this window and cannot keep these profile options. Update it before using effort or preview-bound profiles; existing sessions are unchanged.':
    'El host de terminales es más viejo que esta ventana y no puede mantener estas opciones de perfil. Actualizalo antes de usar esfuerzo o perfiles atados a una vista previa; las sesiones existentes no cambian.',
  'The file could not be written.': 'No se pudo escribir el archivo.',
  'Choose at least one agent, each once.':
    'Elegí al menos un agente, cada uno una sola vez.',
  'The model may hold letters, digits and . _ : - / only, and may not start with a dash.':
    'El modelo solo puede tener letras, dígitos y . _ : - /, y no puede empezar con guion.',
  'The effort may hold letters, digits and . _ - only (32 at most), and may not start with a dash.':
    'El esfuerzo solo puede tener letras, dígitos y . _ - (32 como máximo), y no puede empezar con guion.',
  'A skill name may hold letters, digits and . _ : - only (64 at most).':
    'Un nombre de skill solo puede tener letras, dígitos y . _ : - (64 como máximo).',
  'The notes are too long or hold control characters.':
    'Las notas son demasiado largas o tienen caracteres de control.',
  'The text looks like it holds a secret. A profile never holds one.':
    'El texto parece tener un secreto. Un perfil nunca guarda uno.',
  'The profile it extends does not exist.': 'El perfil que hereda no existe.',
  'A profile cannot extend itself, directly or through others.':
    'Un perfil no puede heredar de sí mismo, ni directa ni indirectamente.',
  'Profiles may extend up to eight levels.':
    'Los perfiles pueden heredar hasta ocho niveles.',
  'A template is read only. Make a copy to change it.':
    'Una plantilla es de solo lectura. Hacé una copia para cambiarla.',
  'There are too many profiles (100).': 'Hay demasiados perfiles (100).',
  'That version does not exist.': 'Esa versión no existe.',
  'That file is not a profile this app can read.':
    'Ese archivo no es un perfil que esta app pueda leer.',
  'The profiles could not be read or written.':
    'No se pudieron leer ni guardar los perfiles.',
  'Another profile extends this one. Change that one first.':
    'Otro perfil hereda de este. Cambiá ese primero.',
  'Session profiles': 'Perfiles de sesión',
  'A profile says what one session should start with. It is applied when the session starts and changes no global file: your Claude Code and Codex settings stay as they are. OpenCode and Hermes Native retain their own controls; unsupported profile fields are refused. A profile holds names and choices, never a secret.':
    'Un perfil dice con qué debe arrancar una sesión. Se aplica al iniciar la sesión y no cambia ningún archivo global: tus ajustes de Claude Code y Codex quedan como están. OpenCode y Hermes Native conservan sus propios controles; los campos de perfil que no soportan se rechazan. Un perfil guarda nombres y opciones, nunca un secreto.',
  'New profile': 'Nuevo perfil',
  'Import…': 'Importar…',
  'Version {n}': 'Versión {n}',
  Template: 'Plantilla',
  Profile: 'Perfil',
  '· refers to something not present': '· apunta a algo que no está',
  'Choose a profile or make a new one': 'Elegí un perfil o creá uno nuevo',
  'Templates are read only. Make a copy to change one.':
    'Las plantillas son de solo lectura. Hacé una copia para cambiar una.',
  'Written for': 'Pensado para',
  Extends: 'Hereda de',
  Nothing: 'Nada',
  Model: 'Modelo',
  'the agent’s own default': 'el predeterminado del agente',
  'Names are suggestions: Claude Code’s documented levels, its model aliases (':
    'Los nombres son sugerencias: los niveles documentados de Claude Code, sus alias de modelo (',
  it: 'el agente',
  'decides what they mean), and what the managed Codex runtime lists':
    'decide qué significan), y lo que lista el runtime Codex Managed',
  'right now': 'ahora mismo',
  '(not running, so none are offered)':
    '(no está corriendo, así que no se ofrece ninguno)',
  '. Wyss Console cannot check a name: the agent says so in its session if it does not know it. Claude Code takes only its documented levels; Codex takes the word as a configuration override for that session only.':
    '. Wyss Console no puede verificar un nombre: el agente avisa en su sesión si no lo conoce. Claude Code acepta solo sus niveles documentados; Codex toma la palabra como una sobrescritura de configuración solo para esa sesión.',
  Permissions: 'Permisos',
  'As configured': 'Según la configuración',
  'Skills switched off (Claude Code), one per line':
    'Skills desactivadas (Claude Code), una por línea',
  Notes: 'Notas',
  'Inherited values:': 'Valores heredados:',
  '{field} from “{from}”;': '{field} de “{from}”;',
  Create: 'Crear',
  'Save as a new version': 'Guardar como versión nueva',
  'Make a copy': 'Hacer una copia',
  Versions: 'Versiones',
  current: 'actual',
  'Bring back': 'Volver a esta versión',
  'Profile exported.': 'Perfil exportado.',
  'Profile imported. It grants nothing: read what it asks for before using it.':
    'Perfil importado. No otorga nada: leé lo que pide antes de usarlo.',
  'Hand off to another agent': 'Traspasar el trabajo a otro agente',
  'Hand off {agent}’s work': 'Traspasar el trabajo de {agent}',
  'This starts a': 'Esto inicia una',
  new: 'nueva',
  'session of the agent you choose, in the same folder, with the text below as its first message. It is not a transfer: the new agent has only this text and the files, not {agent}’s context, and nothing is moved or ended. Read it and change it before starting. Secrets found in it were removed.':
    'sesión del agente que elegís, en la misma carpeta, con el texto de abajo como primer mensaje. No es un traspaso: el agente nuevo tiene solo este texto y los archivos, no el contexto de {agent}, y no se mueve ni se termina nada. Leelo y cambialo antes de iniciar. Los secretos que había en él se quitaron.',
  'Hermes Native remains available as a separate interactive provider. It is omitted here because its pinned CLI does not expose a verified reviewed first-message interface without entering an effectful one-shot mode.':
    'Hermes Native sigue disponible como proveedor interactivo aparte. Acá se omite porque su CLI fijada no expone una interfaz de primer mensaje verificada y revisada sin entrar en un modo de una sola ejecución con efectos.',
  'Preparing the brief…': 'Preparando el brief…',
  'The screen of that session is no longer held, so the brief has the repository state only.':
    'La pantalla de esa sesión ya no se conserva, así que el brief tiene solo el estado del repositorio.',
  'Brief (Markdown, starts with “# ”)': 'Brief (Markdown, empieza con “# ”)',
  'Longer than 16 KiB: shorten it.': 'Más de 16 KiB: acortalo.',
  'Could not verify this profile preview. Refresh Native before starting.':
    'No se pudo verificar la vista previa de este perfil. Actualizá Native antes de iniciar.',
  'What the profile does': 'Qué hace el perfil',
  'Start {agent} with this brief': 'Iniciar {agent} con este brief',
  'Save to a file…': 'Guardar en un archivo…',
  'Delegated agents': 'Agentes delegados',
  'The terminal host did not list delegated agents.':
    'El host de terminales no pudo listar los agentes delegados.',
  'None yet. A Claude Code or Codex session started with delegation on can give a task to the other one.':
    'Todavía no hay ninguno. Una sesión de Claude Code o Codex iniciada con delegación activada puede darle una tarea a la otra.',
  'From {provider} session {id}': 'Desde la sesión de {provider} {id}',
  '{count} agent': '{count} agente',
  '{count} agents': '{count} agentes',
  'Timed out': 'Agotado',
  'configuration verified': 'configuración verificada',
  'configuration did not match': 'la configuración no coincide',
  'configuration not checked yet': 'configuración sin verificar todavía',
  'configuration not verifiable': 'la configuración no se puede verificar',
  done: 'terminado(s)',
  'timed out': 'agotado(s)',
  cancelled: 'cancelado(s)',
  other: 'otro(s)',
  '{n} completed step(s)': '{n} paso(s) completado(s)',
  'Observed:': 'Observado:',
  'effort not reported': 'esfuerzo no informado',
  'Preset: {name}': 'Preset: {name}',
  'Selection: {name}': 'Selección: {name}',
  'none of your own': 'ninguna propia',
  '{n} installed skill(s) switched off':
    '{n} skill(s) instalada(s) desactivada(s)',
  Files: 'Archivos',
  in: 'en',
  'Stop this agent? Its unfinished work is lost.':
    '¿Detener este agente? Su trabajo sin terminar se pierde.',
  'Stop agent': 'Detener agente',
  'Stopping…': 'Deteniendo…',
  Result: 'Resultado',
  // ---- orchestration ----
  'Pipelines and handoff': 'Pipelines y traspaso',
  'Structured handoff': 'Traspaso estructurado',
  'Managed Codex · F8 authority': 'Managed Codex · autoridad F8',
  'Human entered': 'Ingresado por humano',
  'Provider reported': 'Reportado por el proveedor',
  'Provider output · untrusted': 'Salida del proveedor · no confiable',
  'OL3 supervised · finite DAG · Managed concurrency pinned at 1':
    'OL3 supervisado · DAG finito · concurrencia Managed fijada en 1',
  Coordination: 'Coordinación',
  'Plan and runs': 'Plan y ejecuciones',
  'Create a reviewable plan': 'Crear un plan revisable',
  'Prompts stay in volatile memory until the owner reviews and confirms the digest.':
    'Los prompts permanecen en memoria volátil hasta que el dueño revisa y confirma el digest.',
  'Choose a folder': 'Elige una carpeta',
  'Choose…': 'Elegir…',
  'Project ID (label, not path)': 'Project ID (etiqueta, no ruta)',
  Goal: 'Objetivo',
  'Total budget in minutes': 'Presupuesto total en minutos',
  'The budget must be a whole number between 1 and 60 minutes.':
    'El presupuesto debe ser un número entero entre 1 y 60 minutos.',
  'Steps · maximum 16': 'Pasos · máximo 16',
  'Add step': 'Añadir paso',
  'Step {number}': 'Paso {number}',
  Remove: 'Quitar',
  Provider: 'Proveedor',
  'Managed Claude · adapter blocked': 'Managed Claude · adapter bloqueado',
  'Native Codex · blocked by trust boundary':
    'Native Codex · bloqueado por frontera',
  'Native Claude · blocked by trust boundary':
    'Native Claude · bloqueado por frontera',
  'Hermes planner · blocked': 'Hermes planner · bloqueado',
  'Managed session': 'Sesión Managed',
  'Select a ready session': 'Selecciona una sesión lista',
  'profile not reported': 'perfil no reportado',
  'binding workspace': 'workspace del binding',
  Effect: 'Efecto',
  'Managed inference': 'Managed inference',
  'Managed local tools · F8 approval per action':
    'Managed local tools · aprobación F8 por acción',
  'BLOCKED_TRUST_BOUNDARY · no Native process will be launched from OL3.':
    'BLOCKED_TRUST_BOUNDARY · no se lanzará un proceso Native desde OL3.',
  'BLOCKED_PROVIDER · the planner has no execution authority.':
    'BLOCKED_PROVIDER · el planner no tiene autoridad de ejecución.',
  Dependencies: 'Dependencias',
  '(required)': '(obligatorio)',
  '(optional)': '(opcional)',
  'Required step': 'Paso obligatorio',
  'Exact step input': 'Entrada exacta del paso',
  "Describe this step's work. The text grants no authority or approvals.":
    'Describe el trabajo de este paso. El texto no concede autoridad ni aprobaciones.',
  'M6 keeps one Managed turn running globally at a time. Native full-account, Claude adapter and Hermes planner stay visible but blocked.':
    'M6 fija un turno Managed global a la vez. Native full-account, Claude adapter y Hermes planner permanecen visibles pero bloqueados.',
  'Review the full plan': 'Revisar el plan completo',
  'Reviewed plan': 'Plan revisado',
  'Explicit review · digest': 'Revisión explícita · digest',
  'Review goal, workspace, DAG order, provider, effects, prompts, budget and expiry. Confirming allows only the exact plan shown above.':
    'Revisa objetivo, workspace, orden DAG, proveedor, efectos, prompts, presupuesto y vencimiento. Confirmar permite solo el plan exacto anterior.',
  'See the full plan and exact inputs': 'Ver plan completo y entradas exactas',
  'Confirm and send to the coordinator': 'Confirmar y enviar al coordinador',
  'Prepare structured handoff v1': 'Preparar handoff estructurado v1',
  'Source content is selected explicitly; conversation, credentials, permissions and approvals are never inherited.':
    'El contenido del origen se selecciona explícitamente; nunca se heredan conversación, credenciales, permisos ni aprobaciones.',
  Source: 'Origen',
  'Source plane': 'Plano de origen',
  'Managed source session': 'Sesión Managed de origen',
  'Select a session to observe generation and workspace':
    'Selecciona sesión para observar generación y workspace',
  generation: 'generation',
  'generation observed {generation}': 'generation observada {generation}',
  'Native source session': 'Sesión Native de origen',
  'Select an active Native session': 'Selecciona sesión Native activa',
  restarts: 'reinicios',
  'Source workspace': 'Workspace de origen',
  'Taken from the observed session': 'Se toma de la sesión observada',
  'Source revision': 'Revisión de origen',
  'Session ID': 'Session',
  'Target workspace': 'Workspace de destino',
  'Choose the receiving folder': 'Elige la carpeta receptora',
  'Target revision': 'Revisión de destino',
  'Receiving provider': 'Proveedor receptor',
  'Managed Codex · admitted by F8': 'Managed Codex · admitido por F8',
  'Managed Claude · blocked': 'Managed Claude · bloqueado',
  'Native Codex · blocked, manual copy only':
    'Native Codex · bloqueado, solo copia manual',
  'Native Claude · blocked, manual copy only':
    'Native Claude · bloqueado, solo copia manual',
  'Each handoff requires a new Managed receiver, with no earlier turns or context. The receiver inherits no permissions.':
    'Cada handoff requiere un receptor Managed nuevo, sin turnos ni contexto previo. El receptor no hereda permisos.',
  'Prepare new Managed receiver through F8':
    'Preparar receptor Managed nuevo mediante F8',
  'Configure receiver in Runtimes': 'Configurar receptor en Runtimes',
  'Managed local tools with F8 approval':
    'Managed local tools con aprobación F8',
  'BLOCKED_PROVIDER · no Managed Claude adapter admitted by the F8 authority in this M6.':
    'BLOCKED_PROVIDER · no hay adaptador Managed Claude admitido por la autoridad F8 de este M6.',
  'Native profile (reference)': 'Perfil Native (referencia)',
  'BLOCKED_TRUST_BOUNDARY · no Native target will be started automatically. The existing manual flow remains available.':
    'BLOCKED_TRUST_BOUNDARY · no se iniciará automáticamente un destino Native. El flujo manual existente sigue disponible.',
  'BLOCKED_PROVIDER · Hermes receives no authority to execute.':
    'BLOCKED_PROVIDER · Hermes no recibe autoridad para ejecutar.',
  'Selected content': 'Contenido seleccionado',
  'Goal confirmed by the owner': 'Objetivo confirmado por el dueño',
  Provenance: 'Procedencia',
  'Chosen excerpts (one line per item)':
    'Extractos elegidos (una línea por elemento)',
  'File references as relative labels (one per line)':
    'Referencias de archivo como etiquetas relativas (una por línea)',
  'Open questions (one per line)': 'Preguntas abiertas (una por línea)',
  'Seal and review exact bytes': 'Sellar y revisar bytes exactos',
  Target: 'Destino',
  target: 'destino',
  workspace: 'workspace',
  'Review and delivery': 'Revisión y entrega',
  'Every quoted text is untrusted input. The digest is confirmed against source, target, revisions and selection.':
    'Todo texto citado es entrada no confiable. El digest se confirma contra origen, destino, revisiones y selección.',
  'M5/F8 is inactive. Managed handoff cannot start until the approved authority is active.':
    'M5/F8 está inactivo. Handoff Managed no puede iniciar hasta que la autoridad aprobada esté activa.',
  'Confirmed digest': 'Digest confirmado',
  Handoff: 'Handoff',
  'Session target': 'Destino de sesión',
  'proposal without session': 'propuesta sin sesión',
  'Exact first message, delimited as untrusted context':
    'Primer mensaje exacto, delimitado como contexto no confiable',
  'Review the full envelope': 'Revisar sobre completo',
  Copied: 'Copiado',
  'Copy reviewed message': 'Copiar mensaje revisado',
  'BLOCKED_TRUST_BOUNDARY · the digest does not authorize a Native launch; use the manual flow and its local confirmation.':
    'BLOCKED_TRUST_BOUNDARY · el digest no autoriza el lanzamiento Native; usa el flujo manual y su confirmación local.',
  'BLOCKED_PROVIDER · the target adapter is not admitted for sending.':
    'BLOCKED_PROVIDER · el adaptador del destino no está admitido para envío.',
  'Confirm digest and deliver to the new Managed':
    'Confirmar digest y entregar al Managed nuevo',
  'Handoff content is volatile. Complete the selection and workspace to generate the review.':
    'El contenido del handoff es volátil. Completa selección y workspace para generar la revisión.',
  Execution: 'Ejecución',
  'Delivery does not mean the recipient finished correctly.':
    'La entrega no implica que el destinatario haya terminado correctamente.',
  'Local runs': 'Ejecuciones locales',
  'Durable metadata only. No prompts or provider output in the local log.':
    'Solo metadatos durables. Sin prompts ni salida de proveedor en el registro local.',
  'Reading local log…': 'Leyendo registro local…',
  'No runs recorded.': 'No hay ejecuciones registradas.',
  digest: 'digest',
  '{provider} · {effect} · attempts {count}':
    '{provider} · {effect} · intentos {count}',
  'The turn ended. Inspect the workspace before marking the step completed.':
    'El turno terminó. Inspecciona el workspace antes de marcar el paso completado.',
  'Uncertain result or coordinator restarted. The turn is not resent. Confirm evidence before closing this step.':
    'Resultado incierto o coordinador reiniciado. No se reenvía el turno. Confirma evidencia antes de cerrar este paso.',
  'Choose and observe workspace': 'Elegir y observar workspace',
  'Confirm output after reviewing the current revision':
    'Confirmar salida tras revisar la revisión actual',
  'Observed revision': 'Revisión observada',
  'Observed workspace': 'Workspace observado',
  'I confirm it completed · review current revision':
    'Confirmo que completó · revisar revisión actual',
  'I confirm it had no effect · close as failed':
    'Confirmo que no tuvo efecto · cerrar como fallido',
  'I confirm cancellation': 'Confirmo cancelación',
  Pause: 'Pausar',
  'Stop/cancel': 'Detener/cancelar',
  'The local coordinator is not available.':
    'El coordinador local no está disponible.',
  'The request does not match the M6 contract.':
    'La solicitud no coincide con el contrato M6.',
  'The plan is not valid or exceeds the finite limits.':
    'El plan no es válido o supera los límites finitos.',
  'The handoff is incomplete, expired or changed.':
    'El handoff está incompleto, vencido o cambió.',
  'The objective exceeds 2,048 bytes; shorten the text before sealing it.':
    'El objetivo supera 2.048 bytes; reduce el texto antes de sellarlo.',
  'The generated message exceeds 8 KiB; shorten the excerpts or questions before sealing it.':
    'El mensaje generado supera 8 KiB; reduce los extractos o preguntas antes de sellarlo.',
  'The content changed after the review; review it again.':
    'El contenido cambió después de la revisión; vuelve a revisarlo.',
  'Blocked: the Native trust boundary is not proven.':
    'Bloqueado: la frontera de confianza de Native no está demostrada.',
  'The provider or host is not allowed for this handoff.':
    'El proveedor o el host no está admitido para este handoff.',
  'F8 confirmed the receiver has earlier work or is not prepared as a new session.':
    'F8 confirmó que el receptor tiene trabajo previo o no está preparado como sesión nueva.',
  'The F8 evidence cannot prove the receiver is new and has no history.':
    'No se pudo probar desde la evidencia F8 que el receptor esté nuevo y sin historial.',
  'The current identity or generation of the source Managed session could not be proven.':
    'No se pudo probar la identidad o generación actual de la sesión Managed de origen.',
  'The Managed authority denied the start.':
    'La autoridad Managed denegó el inicio.',
  'The Managed resource is busy; no other turn was started.':
    'El recurso Managed está ocupado; no se inició otro turno.',
  'The workspace changed or does not match the approved revision.':
    'El workspace cambió o no coincide con la revisión aprobada.',
  'The local journal could not save the state.':
    'El registro local no pudo guardar el estado.',
  'The Managed authority did not respond; the result may need review.':
    'La autoridad Managed no respondió; el resultado puede requerir revisión.',
  'That run was not found.': 'No se encontró esa ejecución.',
  'The action is not available in the current state.':
    'La acción no está disponible para el estado actual.',
  'The review or the run expired.': 'La revisión o la ejecución venció.',
  'The effect is uncertain and stays stopped for human review.':
    'El efecto es incierto y queda detenido para revisión humana.',
  'Operation M6 failed.': 'Falló la operación M6.',
  'The M6 operation could not be completed.':
    'No se pudo completar la operación M6.',
  'Could not read Managed sessions from F8.':
    'No se pudieron leer sesiones Managed desde F8.',
  'The Managed authority is inactive; only the observed Native sessions are shown.':
    'La autoridad Managed está inactiva; se muestran solo las sesiones Native observadas.',
  'The budget must be a whole number from 1 to 60 minutes.':
    'El presupuesto debe ser un número entero entre 1 y 60 minutos.',
  'Complete the workspace, objective, steps, dependencies and sessions before reviewing.':
    'Completa el workspace, objetivo, pasos, dependencias y sesiones antes de revisar.',
  'The Native session is no longer active; refresh and select a running session.':
    'La sesión Native ya no está activa; actualiza y selecciona una sesión en ejecución.',
  'Select the workspace and confirm the current revision before declaring an effect completed.':
    'Selecciona el workspace y confirma la revisión actual antes de declarar un efecto completado.',
  'First prepare a new Managed session for this handoff.':
    'Primero prepara una sesión nueva Managed para este handoff.',
  'Select a target workspace and enable F8 Managed before preparing the receiver.':
    'Selecciona workspace de destino y habilita F8 Managed antes de preparar el receptor.',
  'New Managed session prepared. Authorize its binding, runtime and lease from Runtimes.':
    'Sesión Managed nueva preparada. Autoriza su binding, runtime y lease desde Runtimes.',
  'New receiver {id}': 'Receptor nuevo {id}',
  'F8 did not confirm the preparation. No other session is created with a different intent; retry the same request.':
    'F8 no confirmó la preparación. No se crea otra sesión con un intent diferente; vuelve a intentar el mismo pedido.',
  'Could not prepare the Managed receiver through F8.':
    'No se pudo preparar el receptor Managed mediante F8.',
  'A new Managed session, active F8 or a target workspace is missing.':
    'Falta una sesión nueva Managed, F8 activo o workspace destino.',
  'The receiver was not created for this handoff. Prepare a new Managed session.':
    'El receptor no fue creado para este handoff. Prepara una sesión Managed nueva.',
  '{id} · generation {generation} · {profile} · lease {lease}':
    '{id} · generation {generation} · {profile} · lease {lease}',
  'unknown profile': 'perfil desconocido',
  absent: 'ausente',
  'New receiver, without history, attached to the workspace and enabled by the F8 turn_start lease.':
    'Receptor nuevo, sin historial, adjunto al workspace y habilitado por la lease F8 turn_start.',
  'Receiver not admitted yet. In Runtimes, configure the requested profile, bind the target workspace and grant a turn_start lease; the handoff will not be sent until everything is confirmed.':
    'Receptor aún no admitido. En Runtimes, configura el perfil solicitado, enlaza el workspace destino y concede una lease turn_start; no se enviará el handoff hasta confirmar todo.',
  'The receiver could not be observed through F8; it is not assumed to be prepared.':
    'No se pudo observar el receptor desde F8; no se asume que esté preparado.',
  'Prepare a new Managed receiver for this handoff first.':
    'Prepara primero un receptor Managed nuevo para este handoff.',
  'Complete source, target, session, objective and review both workspace revisions.':
    'Completa origen, destino, sesión, objetivo y revisa ambas revisiones de workspace.',
  'BLOCKED: the Native target requires an accepted trust boundary. The text can be copied for the existing manual flow.':
    'BLOCKED: el destino Native requiere una frontera de confianza aceptada. El texto se puede copiar para el flujo manual existente.',
  'The clipboard is not available; select and copy the reviewed text.':
    'El portapapeles no está disponible; selecciona y copia el texto revisado.',
  // ---- final ----
  'Failed ({code})': 'Falló ({code})',
  'none (the agent’s own choice)': 'ninguno (la elección del propio agente)',
  'the profile no longer exists': 'el perfil ya no existe',
  'the profile has changed since this session started':
    'el perfil cambió desde que empezó esta sesión',
  'not read yet (open Configuration)': 'todavía sin leer (abre Configuración)',
  'not set in its files': 'sin fijar en sus archivos',
  'not observed by this provider configuration view':
    'no lo observa esta vista de configuración del proveedor',
  'not reported by the agent': 'el agente no lo informa',
  'The launch preview failed.': 'La vista previa del lanzamiento falló.',
  // ---- workspace ----
  '{name} · not applied': '{name} · sin aplicar',
  'Update {agent}': 'Actualizar {agent}',
  'Started with {name} applied.': 'Iniciado con {name} aplicado.',
  'Started without applying {name}; the selection is recorded.':
    'Iniciado sin aplicar {name}; la selección queda registrada.',
  'ended {time}': 'terminó {time}',
  profile: 'perfil',
  agent: 'agente',
  'this agent': 'este agente',
  'Home · {path}': 'Inicio · {path}',
  'Start {agent}': 'Iniciar {agent}',
  'Agents started from the Console': 'Agentes iniciados desde la Console',
  'Hermes Native is a full-account provider session. Its approval prompts and hooks are not a Wyss security boundary. Hermes planner admission is separately blocked pending tested OS isolation. The installed version is not probed because Hermes version output performs an update check.':
    'Hermes Native es una sesión de proveedor con toda la cuenta. Sus avisos de aprobación y sus hooks no son una frontera de seguridad de Wyss. La admisión del planificador de Hermes está bloqueada aparte hasta probar un aislamiento del sistema. La versión instalada no se sondea, porque la salida de versión de Hermes hace una comprobación de actualización.',
  'Looking for your agents…': 'Buscando tus agentes…',
  'Agents cannot be started here': 'Los agentes no se pueden iniciar aquí',
  'The desktop did not answer. Nothing was started.':
    'El escritorio no respondió. No se inició nada.',
  'OpenCode and Hermes wait until the Console confirms its private M9 session store again.':
    'OpenCode y Hermes esperan a que la Console vuelva a confirmar su almacén privado de sesiones M9.',
  'Check again': 'Revisar de nuevo',
  'OpenCode and Hermes are blocked: the Console could not confirm its private M9 session store, because a read or a write failed. Claude Code and Codex stay available where the Console allows them.':
    'OpenCode y Hermes están bloqueados: la Console no pudo confirmar su almacén privado de sesiones M9, porque falló una lectura o una escritura. Claude Code y Codex siguen disponibles donde la Console los permite.',
  'This terminal host predates M9. OpenCode and Hermes Native require a matching M9 host; existing Native sessions remain available.':
    'Este host de terminal es anterior a M9. OpenCode y Hermes Native requieren un host M9 compatible; las sesiones Native existentes siguen disponibles.',
  'Start an agent': 'Iniciar un agente',
  installed: 'instalado',
  'Project folder': 'Carpeta del proyecto',
  'Could not verify this profile preview. Start stays disabled until a fresh preview succeeds.':
    'No se pudo verificar la vista previa de este perfil. Iniciar sigue deshabilitado hasta que una vista previa nueva funcione.',
  'Refreshing the profile preview…': 'Actualizando la vista previa del perfil…',
  'The selected profile changed. Checking a fresh preview before Start.':
    'El perfil elegido cambió. Se verifica una vista previa nueva antes de Iniciar.',
  'Retry preview': 'Reintentar vista previa',
  'Use for this project': 'Usar en este proyecto',
  'This profile cannot be applied to {agent}, so it will not start.':
    'Este perfil no se puede aplicar a {agent}, así que no se iniciará.',
  Delegation: 'Delegación',
  'Let it hand tasks to Claude Code and Codex through Wyss (2 agents at a time, 20 minutes each, writes only in the workspace).':
    'Permitir que pase tareas a Claude Code y Codex a través de Wyss (de a 2 agentes a la vez, 20 minutos cada uno, escribe solo en el workspace).',
  Change: 'Cambiar',
  'Checked {date}.': 'Verificado el {date}.',
  'Interface: {interface}. Format: {format}. Scope: {scope}.':
    'Interfaz: {interface}. Formato: {format}. Alcance: {scope}.',
  'Reload: {reload}.': 'Recarga: {reload}.',
  'Source: {source}.': 'Fuente: {source}.',
  'Limit: {limit}.': 'Límite: {limit}.',
  'Runs “{command}” in a new terminal':
    'Ejecuta “{command}” en una terminal nueva',
  'Capability evidence and limits': 'Evidencia y límites de capacidades',
  'Starts through your shell ({family}): same environment as typing the command yourself.':
    'Inicia a través de tu shell ({family}): mismo entorno que si escribieras el comando vos mismo.',
  'Starts directly with the Console’s own environment.':
    'Inicia directamente con el entorno propio de la Console.',
  "Native sessions: the provider's own tools, configuration and privileges stay active. The Console does not mediate, contain or approve their effects.":
    'Sesiones Native: las herramientas, la configuración y los privilegios del proveedor siguen activos. La Console no media, contiene ni aprueba sus efectos.',
  'Native plane': 'Plano Native',
  'Start the lost session again': 'Iniciar de nuevo la sesión perdida',
  'Start {count} lost sessions again':
    'Iniciar de nuevo {count} sesiones perdidas',
  'Remove finished': 'Quitar terminadas',
  'No agent sessions yet': 'Todavía no hay sesiones de agentes',
  'Start one above. It opens in the Console as a terminal tab.':
    'Inicia una arriba. Se abre en la Console como una pestaña de terminal.',
  'Two sessions run here and can write the same files. Nothing coordinates them.':
    'Dos sesiones corren aquí y pueden escribir los mismos archivos. Nada las coordina.',
  'Sessions in {folder}': 'Sesiones en {folder}',
  'Profile {name} (v{version})': 'Perfil {name} (v{version})',
  'No profile': 'Sin perfil',
  'up {time}': 'activa {time}',
  'started {date}': 'iniciada {date}',
  'started directly': 'iniciada directamente',
  'restart {count}': 'reinicio {count}',
  'process {pid}': 'proceso {pid}',
  '{count} processes': '{count} procesos',
  'Model: asked for {requested} · set in its files {configured} · in use: {inUse} · Effort: asked for {effort} · set in its files {configuredEffort}':
    'Modelo: pedido {requested} · fijado en sus archivos {configured} · en uso: {inUse} · Esfuerzo: pedido {effort} · fijado en sus archivos {configuredEffort}',
  Open: 'Abrir',
  Stop: 'Detener',
  Restart: 'Reiniciar',
  'Show terminal': 'Mostrar terminal',
  'Start again': 'Iniciar de nuevo',
  'Hand off…': 'Transferir…',
  'Native sessions': 'Sesiones Native',
  // ---- arsenal sync ----
  Synchronization: 'Sincronización',
  Waiting: 'En espera',
  'Conflicts kept both versions': 'Conflictos: se conservaron ambas versiones',
  'Deleted on another computer': 'Borrados en otra computadora',
  'Synchronization details': 'Detalles de la sincronización',
  'This computer': 'Esta computadora',
  '{host} · {count} change(s) published':
    '{host} · {count} cambio(s) publicado(s)',
  'Shared folder': 'Carpeta compartida',
  '(Syncthing carries it)': '(Syncthing lo transporta)',
  'Last check': 'Última comprobación',
  'not yet': 'todavía no',
  'applied {applied} of {published} change(s) from here':
    'aplicó {applied} de {published} cambio(s) de esta computadora',
  'last heard {time}': 'última señal {time}',
  'Check now': 'Comprobar ahora',
  // ---- sweep ----
  'Arsenal for this terminal': 'Arsenal para esta terminal',
  'Arsenal selection': 'Selección de Arsenal',
  'Choose an agent first.': 'Elegí un agente primero.',
  'Arsenal selections are for Claude Code and Codex. This agent starts as configured.':
    'Las selecciones de Arsenal son para Claude Code y Codex. Este agente arranca como está configurado.',
  'None — skills as installed': 'Ninguna: skills como están instaladas',
  Reload: 'Recargar',
  'No saved selection for this provider. Make one in Arsenal › Selections.':
    'No hay una selección guardada para este proveedor. Creá una en Arsenal › Selecciones.',
  'Building the terminal plan…': 'Armando el plan de la terminal…',
  'Plan ready': 'Plan listo',
  'Plan partial': 'Plan parcial',
  'Plan blocked': 'Plan bloqueado',
  'unidentified build': 'build sin identificar',
  'no project': 'sin proyecto',
  '{count} prepared': '{count} preparada(s)',
  '{count} rejected': '{count} rechazada(s)',
  '{count} not carried': '{count} no incluida(s)',
  '{count} installed skill(s) stay':
    '{count} skill(s) instalada(s) se mantienen',
  '{count} already off in your settings':
    '{count} ya desactivada(s) en tu configuración',
  '{count} would be switched off': '{count} se desactivarían',
  'Items ({count})': 'Ítems ({count})',
  'Missing capability': 'Capacidad faltante',
  'revision {rev}': 'revisión {rev}',
  pinned: 'fijada',
  'Switched off ({count})': 'Desactivadas ({count})',
  'Nothing.': 'Nada.',
  'Not changed': 'Sin cambios',
  'Session flag it would add (with --no-daemon)':
    'Flag de sesión que agregaría (con --no-daemon)',
  'Session settings it would write': 'Configuración de sesión que escribiría',
  'Apply to this terminal (restart required to change it later)':
    'Aplicar a esta terminal (hace falta reiniciar para cambiarlo después)',
  'Checking the launch preview…': 'Revisando la vista previa del arranque…',
  'The launch preview refuses this combination:':
    'La vista previa del arranque rechaza esta combinación:',
  'NOT APPLIED: this terminal will start with every installed skill.':
    'NO APLICADO: esta terminal arrancará con todas las skills instaladas.',
  'Start without applying it, and record the selection with the terminal':
    'Arrancar sin aplicarlo, y registrar la selección con la terminal',
  'Can be applied': 'Se puede aplicar',
  'Not qualified for this build': 'Build no calificada',
  'Build not identified': 'Build no identificada',
  'Installed skills not fully read': 'Skills instaladas no leídas por completo',
  prepared: 'preparada',
  'not carried': 'no incluida',
  missing: 'faltante',
  'Same name twice': 'Mismo nombre dos veces',
  'Same content twice': 'Mismo contenido dos veces',
  'Several rules': 'Varias reglas',
  'Two methodologies kept': 'Dos metodologías conservadas',
  'The session ended.': 'La sesión terminó.',
  'The terminal host refused the input.':
    'El host de la terminal rechazó la entrada.',
  'Sync error': 'Error de sincronización',
  'The last run failed: {error}.': 'La última corrida falló: {error}.',
  Receiving: 'Recibiendo',
  '{changes} waiting for files still on their way.':
    '{changes} en espera de archivos que todavía vienen en camino.',
  '{count} change': '{count} cambio',
  '{count} changes': '{count} cambios',
  '{count} items': '{count} elementos',
  '{count} item': '{count} elemento',
  '{conflicts} kept both versions':
    '{conflicts}: se conservaron ambas versiones',
  '{items} refused': '{items} rechazado(s)',
  '{items} deleted on another computer, a copy kept here':
    '{items} borrado(s) en otra computadora, con una copia guardada acá',
  '{parts}. See the details.': '{parts}. Mirá los detalles.',
  '{first}; {rest}': '{first}; {rest}',
  'This computer only': 'Solo esta computadora',
  'No other computer has written to the shared folder yet.':
    'Ninguna otra computadora escribió todavía en la carpeta compartida.',
  Published: 'Publicado',
  '{hosts} has not applied {changes} from here yet.':
    '{hosts} todavía no aplicó {changes} de esta computadora.',
  '{hosts} has not applied {changes} from here yet (last heard {time}).':
    '{hosts} todavía no aplicó {changes} de esta computadora (última señal {time}).',
  'In sync': 'Sincronizado',
  '{hosts} applied everything published here; checked {when}.':
    '{hosts} aplicó todo lo publicado acá; comprobado {when}.',
  '{hosts} applied everything published here.':
    '{hosts} aplicó todo lo publicado acá.',
  'just now': 'recién',
  '{minutes} min ago': 'hace {minutes} min',
  '{hours} h ago': 'hace {hours} h',
  '{days} d ago': 'hace {days} d',
  // ---- featc ----
  Library: 'Biblioteca',
  Imported: 'Importada',
  Discover: 'Descubrir',
  Composites: 'Composites',
  Account: 'Cuenta',
  'Not imported': 'No importada',
  'Source changed': 'El origen cambió',
  'Read only': 'Solo lectura',
  Tool: 'Herramienta',
  File: 'Archivo',
  'declared, not checked offline': 'declarada, sin verificar sin conexión',
  present: 'presente',
  unknown: 'desconocida',
  'A hook runs code. Arsenal lists it and never copies or runs it.':
    'Un hook ejecuta código. Arsenal lo lista y nunca lo copia ni lo ejecuta.',
  'An MCP definition can hold credentials. Arsenal lists it and never copies it.':
    'Una definición MCP puede tener credenciales. Arsenal la lista y nunca la copia.',
  'A plugin brings code and hooks. Arsenal lists it; importing plugins is not available yet.':
    'Un plugin trae código y hooks. Arsenal lo lista; importar plugins todavía no está disponible.',
  'GSD Core payload': 'Contenido de GSD Core',
  'Claude Code plugin': 'Plugin de Claude Code',
  'Codex plugin': 'Plugin de Codex',
  'Claude user skills': 'Skills de usuario de Claude',
  'Codex user skills': 'Skills de usuario de Codex',
  'Codex home skills': 'Skills de la carpeta de Codex',
  'Codex system skills': 'Skills del sistema de Codex',
  'Claude project skills': 'Skills de proyecto de Claude',
  'Codex project skills': 'Skills de proyecto de Codex',
  'Claude user agents': 'Agentes de usuario de Claude',
  'Codex user agents': 'Agentes de usuario de Codex',
  'Claude project agents': 'Agentes de proyecto de Claude',
  'Codex project agents': 'Agentes de proyecto de Codex',
  'Claude user rules': 'Reglas de usuario de Claude',
  'Codex user rules': 'Reglas de usuario de Codex',
  'Claude project rules': 'Reglas de proyecto de Claude',
  'Codex project rules': 'Reglas de proyecto de Codex',
  'Credential file, not copied': 'Archivo de credenciales, no se copia',
  'Hidden, not copied': 'Oculto, no se copia',
  'Link, not followed': 'Enlace, no se sigue',
  'Binary, not copied': 'Binario, no se copia',
  'May hold a secret': 'Puede tener un secreto',
  'Special file, not copied': 'Archivo especial, no se copia',
  'Nested too deep, not copied': 'Anidado demasiado profundo, no se copia',
  'Name is not UTF-8, not copied': 'El nombre no es UTF-8, no se copia',
  read: 'leída',
  'not present': 'no está',
  'could not be read': 'no se pudo leer',
  'Importing…': 'Importando…',
  'Already imported': 'Ya importada',
  'Import as new revision': 'Importar como revisión nueva',
  'Installed does not mean imported. A selection reaches a terminal only when you start one with it from Workspace.':
    'Instalada no quiere decir importada. Un preset llega a una terminal solo cuando la iniciás con él desde Workspace.',
  'Arsenal sections': 'Secciones de Arsenal',
  'Reading the Arsenal library…': 'Leyendo la biblioteca de Arsenal…',
  'The Arsenal library could not be opened. Refresh after the Wyss data folder is writable.':
    'No se pudo abrir la biblioteca de Arsenal. Actualizá cuando la carpeta de datos de Wyss se pueda escribir.',
  'The library did not load completely. Try Refresh.':
    'La biblioteca no se cargó completa. Probá Actualizar.',
  'Library sections': 'Secciones de la biblioteca',
  Search: 'Buscar',
  'Name, description or origin': 'Nombre, descripción u origen',
  'Nothing is imported yet. Open Discover to import a capability you already have.':
    'Todavía no hay nada importado. Abrí Descubrir para importar una capacidad que ya tenés.',
  'No capabilities match these filters.':
    'Ninguna capacidad coincide con estos filtros.',
  '{count} imported': '{count} importadas',
  '{shown} of {total} imported': '{shown} de {total} importadas',
  'Imported capabilities': 'Capacidades importadas',
  'Revision {revision}': 'Revisión {revision}',
  'Capability details': 'Detalle de la capacidad',
  'Revision {current} of {total}': 'Revisión {current} de {total}',
  '{files} files · {bytes} B': '{files} archivos · {bytes} B',
  Revisions: 'Revisiones',
  Current: 'Actual',
  'Restoring…': 'Restaurando…',
  'Restore revision {revision}': 'Restaurar la revisión {revision}',
  'Show files of revision {revision}':
    'Ver los archivos de la revisión {revision}',
  'Imported from': 'Importada desde',
  Content: 'Contenido',
  'Use in a session': 'Uso en una sesión',
  'Only through a terminal plan: Workspace shows, per terminal, whether this revision is installed and whether it can be applied.':
    'Solo a través del plan de una terminal: Workspace muestra, por terminal, si esta revisión está instalada y si se puede aplicar.',
  'Its files declare and point at nothing Arsenal can check.':
    'Sus archivos no declaran nada que Arsenal pueda verificar.',
  'Files in revision {revision}': 'Archivos de la revisión {revision}',
  'Account and project': 'Cuenta y proyecto',
  'Scan again': 'Escanear de nuevo',
  'Importing {done} of {total}…': 'Importando {done} de {total}…',
  'Import {count} eligible': 'Importar {count} elegibles',
  'Bulk import progress': 'Progreso de la importación',
  'Reading installed capabilities…': 'Leyendo las capacidades instaladas…',
  'No project selected. Project capabilities are not listed; select a project in Configuration.':
    'No hay proyecto elegido. Las capacidades de proyecto no se listan; elegí un proyecto en Configuración.',
  'Capability folders': 'Carpetas de capacidades',
  'The folders have more entries than the scan shows. Some are not listed.':
    'Las carpetas tienen más entradas de las que muestra el escaneo. Algunas no se listan.',
  '{count} composite collection(s) installed.':
    '{count} colección(es) compuesta(s) instalada(s).',
  'Open Composites': 'Abrir Composites',
  'No capabilities found in your Claude, Codex or project folders.':
    'No se encontraron capacidades en tus carpetas de Claude, Codex o de proyecto.',
  'Discovered capabilities': 'Capacidades encontradas',
  'Duplicate content': 'Contenido duplicado',
  'Name collision': 'Nombre repetido',
  'Capability to import': 'Capacidad para importar',
  'Read from': 'Leída desde',
  'Listed only. Arsenal does not copy it.':
    'Solo listada. Arsenal no la copia.',
  'Contains scripts. Arsenal copies them and never runs them.':
    'Tiene scripts. Arsenal los copia y nunca los ejecuta.',
  'Same content as {names}.': 'Mismo contenido que {names}.',
  'Another capability here has the same name. Both can be imported; they stay separate.':
    'Otra capacidad de acá tiene el mismo nombre. Se pueden importar las dos; quedan separadas.',
  'Files that will be copied': 'Archivos que se van a copiar',
  'Files in this capability': 'Archivos de esta capacidad',
  'Not copied': 'No se copia',
  'Review before importing': 'Revisar antes de importar',
  'These files use a credential word. They are copied; check that none holds a real value.':
    'Estos archivos usan una palabra de credencial. Se copian; revisá que ninguno tenga un valor real.',
  'Mentions a credential word': 'Menciona una palabra de credencial',
  'This is a link into {composite} {version}. It is read only: GSD Core installs it, so Arsenal does not copy it.':
    'Es un enlace a {composite} {version}. Es solo lectura: lo instala GSD Core, así que Arsenal no lo copia.',
  'A composite is installed by its own tool: GSD Core by its installer, a plugin by Claude Code or Codex. Arsenal describes it and never copies it.':
    'Un composite lo instala su propia herramienta: GSD Core con su instalador, un plugin con Claude Code o Codex. Arsenal lo describe y nunca lo copia.',
  'Reading installed composites…': 'Leyendo los composites instalados…',
  'No composite collection is installed in your Claude or Codex folders.':
    'No hay ninguna colección composite instalada en tus carpetas de Claude o Codex.',
  'Enabled in your settings': 'Habilitado en tu configuración',
  'Disabled in your settings': 'Deshabilitado en tu configuración',
  Methodology: 'Metodología',
  'Not importable': 'No importable',
  Members: 'Miembros',
  '{skills} skills · {agents} agents': '{skills} skills · {agents} agentes',
  '{count} commands': '{count} comandos',
  '{count} artifacts in its manifest': '{count} artefactos en su manifiesto',
  'Runs code': 'Ejecuta código',
  'Yes: it registers hooks.': 'Sí: registra hooks.',
  'Its hooks and engine belong to the GSD Core install.':
    'Sus hooks y su motor pertenecen a la instalación de GSD Core.',
  'No hooks declared.': 'No declara hooks.',
  'It declares MCP servers.': 'Declara servidores MCP.',
  Manifest: 'Manifiesto',
  Limits: 'Límites',
  'Skills ({count})': 'Skills ({count})',
  'Agents ({count})': 'Agentes ({count})',
  "What each terminal started with, recorded right after its start from the host's own session record. Applied means the host launched it with that exact exclusion; whether the provider then used each skill is not observed.":
    'Con qué arrancó cada terminal, anotado justo después de su inicio desde el registro de sesión del propio host. Aplicado quiere decir que el host la lanzó con esa exclusión exacta; no se observa si el provider después usó cada skill.',
  'Terminal configurations': 'Configuraciones de terminal',
  APPLIED: 'APLICADO',
  'NOT APPLIED': 'NO APLICADO',
  'Use not observed': 'Uso no observado',
  '{prepared} prepared · {excluded} installed skills switched off · {kept} kept':
    '{prepared} preparadas · {excluded} skills instaladas apagadas · {kept} conservadas',
  '{prepared} prepared · {excluded} installed skills would have been switched off · {kept} kept':
    '{prepared} preparadas · {excluded} skills instaladas que se habrían apagado · {kept} conservadas',
  'No terminal has started with a selection yet. Choose one in Workspace when starting Claude Code or Codex.':
    'Ninguna terminal arrancó todavía con un preset. Elegí uno en Workspace al iniciar Claude Code o Codex.',
  'The Arsenal library could not be opened. Check that the Wyss data folder is writable, then Refresh.':
    'No se pudo abrir la biblioteca de Arsenal. Revisá que la carpeta de datos de Wyss se pueda escribir y después actualizá.',
  'The Arsenal library is damaged, so nothing was changed. Keep the file and report it.':
    'La biblioteca de Arsenal está dañada, así que no se cambió nada. Guardá el archivo y avisalo.',
  'Another change is in progress. Try again in a moment.':
    'Hay otro cambio en curso. Probá de nuevo en un momento.',
  'This library was written by a newer version of Wyss Console.':
    'Esta biblioteca la escribió una versión más nueva de Wyss Console.',
  'Your home folder is not known, so skills cannot be read. Refresh to try again.':
    'No se conoce tu carpeta personal, así que no se pueden leer las skills. Actualizá para probar de nuevo.',
  'That item is no longer in the library or in your skill folders. Refresh and try again.':
    'Ese elemento ya no está en la biblioteca ni en tus carpetas de skills. Actualizá y probá de nuevo.',
  'The request was not valid. Check the fields and try again.':
    'El pedido no era válido. Revisá los campos y probá de nuevo.',
  'This kind is listed only. Arsenal does not copy it.':
    'Este tipo solo se lista. Arsenal no lo copia.',
  'This file is too large to show here.':
    'Este archivo es demasiado grande para mostrarlo acá.',
  'This file is not text, so it cannot be shown here.':
    'Este archivo no es texto, así que no se puede mostrar acá.',
  'Arsenal returned something unexpected. Refresh and try again.':
    'Arsenal devolvió algo inesperado. Actualizá y probá de nuevo.',
  'A file holds something that looks like a secret. Remove it from the skill to import it.':
    'Un archivo tiene algo que parece un secreto. Quitalo de la skill para importarla.',
  'This folder has no SKILL.md, so it is not a skill.':
    'Esta carpeta no tiene SKILL.md, así que no es una skill.',
  'This skill is a symbolic link. Arsenal does not follow links out of a skill folder.':
    'Esta skill es un enlace simbólico. Arsenal no sigue enlaces que salgan de una carpeta de skill.',
  'This skill has more files than the import limit.':
    'Esta skill tiene más archivos que el límite de importación.',
  'This skill is larger than the import limit.':
    'Esta skill supera el límite de importación.',
  'One file in this skill is larger than the import limit.':
    'Un archivo de esta skill supera el límite de importación.',
  'This skill nests folders deeper than the import limit.':
    'Esta skill anida carpetas más profundo que el límite de importación.',
  'A file of this skill could not be read.':
    'No se pudo leer un archivo de esta skill.',
  'This skill changed while it was being read. Scan again.':
    'Esta skill cambió mientras se leía. Escaneá de nuevo.',
  'Codex installs and replaces this skill itself. Arsenal lists it and does not copy it.':
    'Codex instala y reemplaza esta skill por sí mismo. Arsenal la lista y no la copia.',
  'This project is in a folder another account can write. Arsenal lists it as read only and does not copy it.':
    'Este proyecto está en una carpeta que otra cuenta puede escribir. Arsenal lo lista como solo lectura y no lo copia.',
  'This skill cannot be imported.': 'Esta skill no se puede importar.',
};
