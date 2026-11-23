# Arquitectura

| Proceso | Responsabilidad |
| --- | --- |
| `wyss-console` | GTK4, entrada y dibujo, pestañas, paneles y layout. Invoca los servicios locales de Workspace, Personalización y Ajustes. |
| `wyss-host` | Retiene PTY, salida y sesiones al cerrar una ventana. |
| `wyss-screen` | Procesa la salida bajo aislamiento. El host puede reiniciarlo y reconstruir la pantalla con el búfer disponible. |

La UI se conecta a `$XDG_RUNTIME_DIR/wyss-console/host.sock`. Si el socket está ausente o quedó obsoleto, arranca el `wyss-host` que está junto a su ejecutable. Una instancia activa con el mismo protocolo se reutiliza. El directorio del socket es privado, el socket queda con modo 0600 y el servidor valida el UID del cliente. Cada conexión transporta una solicitud y una respuesta JSON con prefijo de longitud de cuatro bytes big-endian.

- `host`, `terminal` y `screen` gestionan el IPC, el PTY y la reconstrucción de pantalla. `native` lanza las sesiones de Claude Code y Codex que abre "+ Launch" y guarda su registro.
- `launch` prepara los lanzamientos de Workspace. El selector de directorios recientes usa el registro de sesiones del host.
- `agentconfig`, `arsenal`, `distributors`, `credentials` y `settings` implementan inventario, edición, almacenamiento y preferencias.
- `src/bin/wyss-console-native` contiene la interfaz, las traducciones generadas y el enlace al motor de terminal.

El instalador mantiene un directorio y una entrada de menú propios.

Los presets de Claude Code activan mediante `skillOverrides` las skills elegidas que la
configuración efectiva tenía desactivadas. El mismo overlay desactiva las skills instaladas
que quedaron afuera. Codex conserva el aviso para las skills elegidas que ya estaban desactivadas.

Al abrir o reiniciar una sesión con preset, si hay `CLAUDE.md` o `AGENTS.md` en la carpeta de
trabajo, Wyss muestra sus secciones y los imports directos de Claude que resuelven a un archivo
regular dentro de la carpeta. Los imports se presentan como texto independiente. La selección
local conserva por preset y proyecto solo identificadores, hashes y casillas. Las secciones
nuevas aparecen desmarcadas y las editadas llevan un aviso. El inicio excluye el descubrimiento
de instrucciones del proyecto y entrega el texto revisado después del rol del preset, con un
límite conjunto de 64 KiB. Claude Code recibe el texto por un archivo privado de la sesión y
Codex por las instrucciones de su carpeta de lanzamiento.

La sugerencia se pide desde ese diálogo. Claude Code usa un modelo explícito de `ANTHROPIC_MODEL`,
la clave de API del entorno y una consulta en modo bare, sin herramientas ni persistencia,
en un proceso con escritura de archivos restringida. Al aceptar una sugerencia, el inicio fija
ese mismo modelo. Codex no ofrece sugerencias porque su CLI 0.170.0 carece de un modo sin herramientas.

## Uso y cola por sesión

La franja de uso compara la sesión enfocada y la última registrada del otro proveedor. Al enfocar
una terminal sin agente, toma la última de cada proveedor. Los nombres salen del registro de Wyss
(proveedor, carpeta e ID corto). La intensidad del contexto se indica con texto y color desde
50 % y 75 %. Sin campos válidos la franja muestra «sin dato».

| CLI | Cuotas de 5 horas y 7 días | Contexto |
| --- | --- | --- |
| Claude Code 2.1.330 | El JSON documentado de `statusLine` entrega `rate_limits.five_hour` y `rate_limits.seven_day`, con `used_percentage` y `resets_at`. Wyss lee el reloj local al recibir el JSON. Claude entrega estas cuotas después de una respuesta en los planes compatibles. | El mismo JSON entrega `context_window.used_percentage`, correspondiente a la última respuesta de la API. |
| Codex 0.170.0 | El evento local `token_count` entrega `rate_limits` y la hora UTC del registro. Se acepta `limit_id: codex` y se distingue por duración (300 o 10080 minutos), con `used_percent` y `resets_at`. Puede venir sin límites. | `info.last_token_usage.total_tokens / info.model_context_window` mide la última ventana reportada. Se excluyen los recálculos sintéticos con cero tokens de entrada y se omite el porcentaje cuando el conteo falta o está fuera de rango. |

- El overlay de Claude agrega el callback de uso solo si los ajustes efectivos del usuario y los
  del proyecto carecen de `statusLine`. Un archivo ambiguo, ilegible o con enlaces en la ruta final
  cuenta como ocupado. Los ajustes administrados conservan la precedencia de Claude. Se agrega
  el enlace al callback exclusivamente al overlay del lanzamiento. El UUID de Claude se fija al
  crear ese proceso y se valida en cada callback. Una sesión abierta con otro ID, una versión
  distinta o una configuración que impida el callback puede quedar sin lectura.
  [Status line y esquema](https://code.claude.com/docs/en/statusline),
  [precedencia de settings](https://code.claude.com/docs/en/settings#settings-precedence).
- Wyss vincula el rollout de Codex al proceso del PTY por su descriptor abierto en modo append,
  su archivo dentro de la ruta `sessions/YYYY/MM/DD` y el ID del encabezado `session_meta`.
  Si el proceso usa otro escritor, no expone el descriptor, tiene varios candidatos o no guarda el
  rollout, la lectura de esa sesión queda sin dato. El host lee por desplazamiento en bloques de
  hasta 128 KiB y retiene hasta 256 KiB de una línea incompleta. Ignora registros ajenos, inválidos
  o más grandes. [Esquema de Codex 0.170.0](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/protocol/src/protocol.rs),
  [escritor de rollout](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/rollout/src/recorder.rs).
- El host conserva hasta cuatro muestras de cuota en el archivo privado `native/usage.json` bajo
  el directorio de estado de Wyss. El contexto queda en memoria por sesión. Una muestra de cuota
  queda desactualizada después de 15 minutos o cuando vence su ventana. Repetir los mismos valores
  de cuota no renueva su antigüedad. La UI consulta al host fuera del hilo GTK.
- `native/prompt-queues.json` guarda los textos y su orden por ID nativo, con escritura atómica y
  modo 0600. El archivo admite hasta 2 MiB, 200 sesiones, 32 elementos por sesión y 2048 bytes por
  elemento. El armado pertenece a la ventana y al reiniciar empieza pausado. Los prompts de
  sesiones detenidas con elementos se pueden revisar en «Colas guardadas». Un fallo de escritura
  deja el primer elemento retenido para revisión.
- En Claude, el overlay añade observadores a los hooks existentes. Wyss registra un ID de turno
  local con `UserPromptSubmit`, observa `Stop` y exige después `Notification: idle_prompt` para el
  mismo ID. La notificación llega alrededor de un minuto después de terminar y puede faltar. Una
  solicitud de permiso, pregunta, interrupción, señal vencida, ausencia de modo de pegado o entrada
  manual pausa la cola. El operador arma durante el turno con el campo de entrada vacío. El host
  valida de nuevo sesión, turno, revisión de entrada y el modo de pegado en el parser de terminal.
  Escribe por el PTY un frame de pegado sin secuencias de control internas y un único Enter.
  [Eventos de Claude Code](https://code.claude.com/docs/en/hooks#notification).
- En Codex se pueden editar y guardar las colas. El rollout registra `task_complete` del motor,
  pero las solicitudes de aprobación y de entrada no son eventos durables. El fin de turno no
  acredita que la interfaz esté lista para recibir un pegado. El envío automático queda pausado.
  [Política de persistencia de Codex 0.170.0](https://github.com/openai/codex/blob/rust-v0.170.0/codex-rs/rollout/src/policy.rs).
