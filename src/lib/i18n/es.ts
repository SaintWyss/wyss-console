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

  // ApprovalCard.svelte
  Approval: 'Aprobación',
  session: 'sesión',
  turn: 'turno',
  Command: 'Comando',
  Directory: 'Carpeta',
  'Preview truncated: {shown} of {total} bytes shown.':
    'Vista previa cortada: se muestran {shown} de {total} bytes.',
  'Reading…': 'Leyendo…',
  'Show the rest': 'Mostrar el resto',
  'The rest of this change could not be read.':
    'No se pudo leer el resto de este cambio.',
  '{shown} of {total} files shown.': 'Se muestran {shown} de {total} archivos.',
  'Show more files': 'Mostrar más archivos',
  'The authority no longer holds what this approval was about.':
    'La autoridad ya no tiene lo que describe esta aprobación.',
  'Waits until': 'Espera hasta',
  'The authority checks the action again before anything runs.':
    'La autoridad vuelve a verificar la acción antes de que se ejecute algo.',
  Dismiss: 'Descartar',
  Approve: 'Aprobar',
  Deny: 'Denegar',
  'Read the whole change to approve it.':
    'Leé el cambio completo para aprobarlo.',

  // ApprovalsView.svelte
  'Connecting to the authority': 'Conectando con la autoridad',
  'Authority unavailable': 'Autoridad no disponible',
  Reading: 'Leyendo',
  '{count} waiting for a decision': '{count} esperando una decisión',
  Refresh: 'Actualizar',
  'Connecting to the authority…': 'Conectando con la autoridad…',
  'The authority is unavailable': 'La autoridad no está disponible',
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

  // present.ts
  Pending: 'Pendiente',
  Approved: 'Aprobada',
  Denied: 'Denegada',
  Cancelled: 'Cancelada',
  Expired: 'Vencida',
  'No longer applies': 'Ya no aplica',
  'Outcome unknown': 'Resultado desconocido',
  'File change': 'Cambio de archivo',
  'Nobody decided it in time.': 'Nadie la decidió a tiempo.',
  'The execution lease of its turn expired.':
    'Venció la concesión de ejecución de su turno.',
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
    'Se aprobó, y la autoridad no pudo confirmar qué hizo Codex con eso.',
  'Codex asked for something that cannot be approved.':
    'Codex pidió algo que no se puede aprobar.',
  'Codex acted outside what the session allows.':
    'Codex actuó fuera de lo que permite la sesión.',
  Add: 'Agregar',
  Delete: 'Borrar',
  Modify: 'Modificar',
  Move: 'Mover',
  'Codex wants to run a command': 'Codex quiere ejecutar un comando',
  'Codex wants to change files': 'Codex quiere cambiar archivos',
  'Codex wants to change a file': 'Codex quiere cambiar un archivo',
  'Codex wants to change {count} files':
    'Codex quiere cambiar {count} archivos',
  'Session {id}': 'Sesión {id}',
  'Asking the authority to approve it…':
    'Pidiendo a la autoridad que la apruebe…',
  'Asking the authority to deny it…':
    'Pidiendo a la autoridad que la deniegue…',
  'It was not approved.': 'No se aprobó.',
  'It was not denied.': 'No se denegó.',
  'The console sent a request it should not have. Nothing changed.':
    'La consola envió un pedido que no debía enviar. No cambió nada.',
  'The authority no longer has this approval.':
    'La autoridad ya no tiene esta aprobación.',
  'The approval changed after it was shown. It has been read again.':
    'La aprobación cambió después de mostrarse. Se volvió a leer.',
  'The approval had already ended.': 'La aprobación ya había terminado.',
  'The authority denied it: the lease its turn was admitted under does not allow this action.':
    'La autoridad la denegó: la concesión con la que se admitió su turno no permite esta acción.',
  'Codex is no longer there to answer.': 'Codex ya no está para responder.',
  'The authority is busy with another operation. Try again in a moment.':
    'La autoridad está ocupada con otra operación. Probá de nuevo en un momento.',
  'The authority could not confirm what Codex did with the answer. Nothing is sent again by itself.':
    'La autoridad no pudo confirmar qué hizo Codex con la respuesta. No se vuelve a enviar nada por sí solo.',
  'That request was already used for another decision.':
    'Ese pedido ya se usó para otra decisión.',
  'This console and the authority speak different versions.':
    'Esta consola y la autoridad hablan versiones distintas.',
  'The authority did not answer, so it is not known whether the decision was taken. Retrying repeats the same decision: it cannot decide twice.':
    'La autoridad no respondió, así que no se sabe si la decisión se tomó. Reintentar repite la misma decisión: no puede decidir dos veces.',
  'Something unexpected happened. Nothing is assumed to have changed.':
    'Pasó algo inesperado. No se supone que haya cambiado nada.',
  'Approvals could not be read: the authority did not answer.':
    'No se pudieron leer las aprobaciones: la autoridad no respondió.',
  'Approvals could not be read. The authority is busy; try again in a moment.':
    'No se pudieron leer las aprobaciones. La autoridad está ocupada; probá de nuevo en un momento.',
  'Approvals could not be read. Nothing is shown rather than something unverified.':
    'No se pudieron leer las aprobaciones. No se muestra nada antes que algo sin verificar.',

  // LaunchPanel.svelte
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

  // MigrationPanel.svelte
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

  // ProjectRail.svelte
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
  'Save name': 'Guardar nombre',
  Rename: 'Renombrar',
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

  // ProjectsGate.svelte
  'Projects and workspaces': 'Proyectos y workspaces',
  'On. Projects and workspaces are shown in their own view. Turning them off goes back to the older windows; nothing running is stopped and the project data is kept.':
    'Activado. Los proyectos y workspaces se muestran en su propia vista. Desactivarlos vuelve a las ventanas anteriores; no se detiene nada que esté corriendo y se conservan los datos de los proyectos.',
  'Off. When turned on, a project is a folder with its own workspaces and sessions. Your sessions and the older windows are not changed, and nothing is started or stopped. Turning it on is confirmed in a window of its own.':
    'Desactivado. Al activarlo, un proyecto es una carpeta con sus propios workspaces y sesiones. Tus sesiones y las ventanas anteriores no cambian, y no se inicia ni detiene nada. Activarlo se confirma en una ventana propia.',
  'Turn off…': 'Desactivar…',
  'Turn on…': 'Activar…',

  // ProjectsView.svelte
  Agent: 'Agente',
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

  // WorkspaceCanvas.svelte
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

  // controller.ts
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

  // SessionPane.svelte
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
  Session: 'Sesión',
  'Requested profile': 'Perfil pedido',
  'not recorded': 'no registrado',
  'Attached profile': 'Perfil adjunto',
  'not attached or unknown': 'no adjunto o desconocido',
  'The authority attached a different profile than the one requested. The request grants nothing; open the Managed screens to decide.':
    'La autoridad adjuntó un perfil distinto al pedido. El pedido no otorga nada; abrí las pantallas de Managed para decidir.',
  'A Managed session is opened and run in Sessions, Agents and Runtimes. Showing it here starts nothing and grants nothing.':
    'Una sesión de Managed se abre y corre en Sesiones, Agentes y Runtimes. Mostrarla acá no inicia nada y no otorga nada.',

  // LifecycleCard.svelte
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

  // TerminalView.svelte
  'The launch could not be opened.': 'No se pudo abrir el lanzamiento.',
  starting: 'iniciando',
  running: 'en ejecución',
  closing: 'cerrando',
  exited: 'terminó',
  closed: 'cerrada',
  failed: 'falló',
  'Terminal sessions': 'Sesiones de terminal',
  'Close {name}': 'Cerrar {name}',
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

  // keymap.ts
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

  // launch.ts
  'A command before the start did not succeed ({result}); the agent started anyway.':
    'Un comando previo al inicio no tuvo éxito ({result}); el agente arrancó igual.',
  'Started with {selection} applied.': 'Iniciado con {selection} aplicado.',
  'Started without applying {selection}; the selection is recorded.':
    'Iniciado sin aplicar {selection}; la selección queda registrada.',
  'The terminal started, but Arsenal could not record its selection.':
    'La terminal inició, pero Arsenal no pudo registrar su selección.',

  // parser-status.ts
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

  // TerminalPanel.svelte
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

  // failure-words.ts
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
  // Arsenal: presets and group corrections
  'Preset saved': 'Preset guardado',
  'Could not be saved. Nothing changed.': 'No se pudo guardar. No cambió nada.',
  // Arsenal: the Presets screen
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
  All: 'Todas',
  Skills: 'Skills',
  Rules: 'Reglas',
  Family: 'Familia',
  'No capabilities match.': 'Ninguna capacidad coincide.',
  'Move to group…': 'Mover a grupo…',
  'Back to automatic group': 'Volver al grupo automático',
  'No description in its file': 'Su archivo no trae descripción',
  Skill: 'Skill',
  Rule: 'Regla',
  Hook: 'Hook',
  'MCP server': 'Servidor MCP',
  Plugin: 'Plugin',
  Supported: 'Compatible',
  Unverified: 'Sin verificar',
  'Needs provider qualification': 'Necesita calificación del provider',
  'Missing dependency': 'Falta una dependencia',
  Conflicting: 'En conflicto',
  Unsupported: 'No compatible',
  'Models and rules': 'Modelos y reglas',
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
  Other: 'Otros',
  'How it is put together': 'Cómo está armado',
  Name: 'Nombre',
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
  // ---- launch ----
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
};
