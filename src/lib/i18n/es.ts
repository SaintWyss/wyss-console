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
};
