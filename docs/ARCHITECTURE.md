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
