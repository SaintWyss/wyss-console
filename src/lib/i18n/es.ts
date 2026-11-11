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
};
