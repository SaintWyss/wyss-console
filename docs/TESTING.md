# Pruebas aisladas

Desde la raíz del crate, prepará un HOME y rutas XDG descartables en disco. El siguiente bloque ejecuta las pruebas de release, incluidas las de UI que usan una bóveda en memoria.

```sh
tools/test-isolated.sh
```

Las pruebas de proceso requieren sockets Unix. El auxiliar de pantalla también tiene casos que dependen de las funciones de aislamiento del kernel. Los tests de custodia y de Ajustes también comprueban la propiedad de los directorios ancestros y algunos escriben ACL de prueba. Si el contenedor bloquea estas operaciones o remapea UID del sistema, registrá los casos omitidos y repetilos en una sesión Linux con esos recursos.

Los tests de integración `screen_corpus` y `screen_helper_process` piden la feature `test-fixtures`, que `tools/test-isolated.sh` activa.

```sh
cargo clippy --offline --locked --all-targets -- -D warnings
python3 tools/gen-i18n.py --check
abi/check-abi.sh
```

Las pruebas de teclado, selección, resize y reconexión usan Xvfb, un display dedicado y un bus de sesión aislado. Los pasos y sus dependencias están en [ui-tests](../ui-tests/README.md). Para un cambio de ABI, ejecutá el chequeo de headers junto con las pruebas Rust de Ghostty.

La prueba `v3_upgrade_keeps_safe_offers_and_existing_decisions` verifica la migración de Arsenal de v3 a v4 y la vuelta a v3 mediante [arsenal-v4-to-v3.sql](../tools/arsenal-v4-to-v3.sql). La vuelta conserva las decisiones y las ofertas únicas por objeto. Sync reconstruye las demás ofertas desde el diario.
