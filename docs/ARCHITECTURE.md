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
