# Pruebas de ventanas

Usá Xvfb, xauth, xdotool, xclip, ImageMagick y dbus-run-session. La variante IBus también requiere ibus-daemon y setxkbmap. Desde la raíz del repo, después de compilar:

```sh
R=$(ui-tests/nest-up.sh)
B="${CARGO_TARGET_DIR:-$PWD/target}/release/wyss-console"
ui-tests/hosttest.sh "$R" "$B" "$R/capturas"
ui-tests/ctrlc.sh "$R" "$B" "$R/ctrlc.bin"
ui-tests/kbtest.sh "$R" "$B" "$R/teclado.bin" simple
ui-tests/kbspace.sh "$R" "$B" "$R/espacios.bin" ibus
ui-tests/uitest.sh "$R" "$B" "$R/capturas"
ui-tests/rstest.sh "$R" "$B" "$R/capturas"
ui-tests/seltest.sh "$R" "$B" "$R/capturas"
ui-tests/splittest.sh "$R" "$B" "$R/capturas"
ui-tests/nest-down.sh "$R"
```

Los scripts crean un HOME y un conjunto de directorios XDG por prueba. El caso de reconexión inicia el host hermano de la UI, verifica que la terminal sobrevive al cierre de la ventana y consulta el socket de esa prueba al reabrir. Los logs y capturas quedan en el directorio que imprime `nest-up.sh`.
