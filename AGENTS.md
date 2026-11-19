# Runbook

Este crate Linux compila `wyss-console`, `wyss-host` y `wyss-screen`. El host solo atiende las operaciones que usa esta app. La aplicación lee las preferencias, los lanzamientos, el registro de sesiones y el layout guardados en los formatos admitidos.

- Compilar con `cargo build --release --offline --locked`.
- Ejecutar `tools/test-isolated.sh`, `cargo clippy --offline --locked --all-targets -- -D warnings`, `python3 tools/gen-i18n.py --check` y `abi/check-abi.sh`.
- Editar `i18n/es.json` y regenerar la tabla Rust con `python3 tools/gen-i18n.py`.
- No ejecutar pruebas contra el socket real, el display del usuario, su HOME, `~/.claude`, `~/.codex`, `~/.config/wyss-console` ni su llavero. Usar las rutas aisladas de [docs/TESTING.md](docs/TESTING.md). Guardar archivos temporales en `$HOME/.cache/wyss-build`.
- Usar datos sintéticos en el corpus de terminal y en los fixtures. Antes de publicar, revisar también archivos binarios de prueba y metadatos de capturas.
