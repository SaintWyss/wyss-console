# Wyss Console

Aplicación GTK4 para Linux. Consola abre terminales en pestañas y paneles. Workspace organiza proyectos y lanzamientos. Personalización reúne Distribuidores, Arsenal y Credenciales. Ajustes guarda las preferencias de la aplicación.

## Pantallas

| Pantalla | Qué hace |
| --- | --- |
| Consola | Terminales en pestañas y paneles divididos, con búsqueda en el historial. El host las mantiene vivas al cerrar la ventana y la Consola se vuelve a enganchar al abrirla. |
| Workspace | Lista los lanzamientos de Sistema (comandos por distribución, como Actualizar sistema) y de Agentes. El editor tiene General, Instrucciones, Skills, MCP, Hooks y Resumen, y guarda en `~/.config/wyss-console/launch/*.toml`. Si un lanzamiento que ejecuta algo (comandos, `before` o acceso total) cambió fuera del editor, al abrirlo muestra la carpeta y los comandos exactos y pide aprobación. Claude Global y Codex Global (`claude-global.toml`, `codex-global.toml`) son fijos: Workspace los muestra y los abre, pero no los edita ni los borra. |
| Personalización → Distribuidores | Las carpetas donde cada proveedor lee sus instrucciones y skills. |
| Personalización → Arsenal | La biblioteca de capacidades: escanear, importar, presets, hooks y configuración por proveedor. Se sincroniza con tus otras PC por una carpeta de Syncthing; lo que llega de otra PC queda en **Recibido de otra PC** hasta que lo aceptás, con sus archivos y los scripts marcados. |
| Personalización → Credenciales | Claves por proveedor. La clave se pide en un diálogo propio y nunca se muestra; la lista enseña nombre y huella. Borrar pide escribir el nombre. |
| Ajustes | Apariencia, idioma, pantalla de inicio, terminal (fuente, historial, cursor), qué pasa con las terminales al cerrar la ventana, Acerca de y Restablecer. |

En los lanzamientos de Codex, la pestaña **MCP** permite elegir qué servidores del `config.toml` se usan al arrancar. **Skills → Entorno → Cuenta de Codex** toma las skills de `$CODEX_HOME/skills` o `~/.codex/skills`. Workspace guarda ambos ajustes en el preset. **Ediciones** usa `workspace-write` con aprobación `on-request`.

El host consulta la ayuda del ejecutable instalado de Codex o Claude Code. Si falta una opción del lanzamiento, la Consola avisa qué se omitió. **Acceso total** aparece cuando la ayuda anuncia su parámetro y conserva las comprobaciones de aprobación.

## Atajos de la Consola

| Atajo | Acción |
| --- | --- |
| Ctrl+Shift+T | Terminal nueva en una pestaña nueva |
| Ctrl+Shift+D | Dividir el panel en dos, lado a lado |
| Ctrl+Shift+E | Dividir el panel en dos, uno arriba del otro |
| Ctrl+Shift+W | Cerrar el panel |
| Ctrl+Shift+F | Buscar en el historial del panel |
| Ctrl+Shift+flechas | Ir al panel en esa dirección |
| Ctrl+Shift+. y , | Panel siguiente y anterior |
| Ctrl+Shift+AvPág, RePág | Pestaña siguiente y anterior |
| Ctrl+Shift+1 a 9 | Ir a esa pestaña |
| Ctrl+C | Copiar la selección (nunca interrumpe) |
| Ctrl+Shift+C | Interrumpir el programa (el Ctrl+C de una terminal común) |
| Ctrl+V, Ctrl+Shift+V | Pegar. Si el programa no usa pegado seguro (DEC 2004) y el texto trae un salto de línea, pide confirmación. |

La misma lista aparece en la leyenda de la Consola (`src/bin/wyss-console-native/keymap.rs`).

## Línea de comandos

Hay una sola ventana. Si ya está abierta, `wyss-console …` abre una pestaña en ella y la trae al frente; si no, la abre y restaura las terminales vivas del host.

| Opción | Qué hace |
|---|---|
| `-e PROGRAMA [ARGS…]` o `-- PROGRAMA [ARGS…]` | Corre el programa en una pestaña nueva; la pestaña se cierra cuando el programa termina. Un único argumento con espacios se toma como línea de shell. |
| `--workdir DIR`, `--working-directory DIR`, `-w DIR` | Carpeta de la pestaña nueva. Sin esta opción se usa la carpeta desde donde se llamó. |
| `--no-terminal` | Abre la ventana en Workspace sin crear una pestaña si no hay nada que restaurar. Al entrar a Consola se crea la primera. |
| `N` (número) | Pestañas a abrir cuando no hay nada que restaurar. |

## Requisitos

- Rust 1.92 o posterior, Cargo, compilador C y `pkg-config`.
- GTK4 4.14 o posterior y headers de desarrollo.
- `libghostty-vt` de Ghostty 1.4, con headers compatibles con `abi/check-abi.sh` y la biblioteca compartida accesible al cargador.
- Python 3 para el generador de traducciones. Credenciales requiere `secret-tool` y un servicio Secret Service disponible en la sesión.

## Compilar y ejecutar

```sh
cargo build --release --offline --locked
target/release/wyss-console
```

La compilación offline requiere que las dependencias del lockfile estén en la caché de Cargo. Si usás `CARGO_TARGET_DIR`, buscá los binarios en su subdirectorio `release/`. La UI usa el socket del host del usuario y puede iniciar el ejecutable que la acompaña.

## Instalar y probar

```sh
./install.sh install
./install.sh status
```

La instalación es por usuario. El comando del menú se llama **Wyss Console** y el lanzador de terminal es `~/.local/bin/wyss-console-native`. El instalador copia los tres ejecutables y usa la biblioteca Ghostty provista por el sistema. `rollback` intercambia las dos versiones instaladas. `uninstall` retira los archivos administrados y deja el host activo hasta que termine su ciclo normal.

Consultá [Pruebas](docs/TESTING.md) para correr la suite con un entorno aislado.
