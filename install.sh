#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
prefix="$HOME/.local/opt/wyss-console-native"
previous="$prefix.previous"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
desktop="$apps/wyss-console-native.desktop"
bindir="$HOME/.local/bin"
link="$bindir/wyss-console-native"
build="${CARGO_TARGET_DIR:-$repo/target}/release"
bins=(wyss-console wyss-host wyss-screen)

die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }
owned() { [[ ! -L "$1" && -f "$1/.wyss-native-install" ]]; }
check_slot() {
    if [[ -e "$1" || -L "$1" ]]; then
        owned "$1" || die "ruta ocupada por otra instalación: $1"
    fi
}

case "${1:-status}" in
    install)
        check_slot "$prefix"
        check_slot "$previous"
        [[ ! -e "$desktop" ]] || grep -qx 'X-Wyss-Native=true' "$desktop" ||
            die "entrada de escritorio ocupada: $desktop"
        if [[ -e "$link" || -L "$link" ]]; then
            [[ $(readlink "$link") == "$prefix/bin/wyss-console" ]] ||
                die "lanzador ocupado: $link"
        fi
        for bin in "${bins[@]}"; do
            [[ -x "$build/$bin" ]] || die "falta $build/$bin"
        done
        libraries=$(ldd "$build/wyss-console")
        grep -q 'libghostty-vt.*=>' <<< "$libraries" || die "falta el enlace con libghostty-vt"
        if grep -q 'not found' <<< "$libraries"; then
            die "faltan bibliotecas de ejecución. Instalá GTK4 y libghostty-vt compatibles"
        fi

        mkdir -p -- "$(dirname -- "$prefix")" "$apps" "$bindir"
        stage=$(mktemp -d "$prefix.new.XXXXXX")
        trap 'rm -rf -- "$stage"' EXIT
        mkdir -- "$stage/bin"
        for bin in "${bins[@]}"; do
            install -m 755 -- "$build/$bin" "$stage/bin/$bin"
        done
        (cd -- "$stage/bin" && sha256sum "${bins[@]}") > "$stage/SHA256"
        touch -- "$stage/.wyss-native-install"
        if owned "$prefix"; then
            if owned "$previous"; then rm -rf -- "$previous"; fi
            mv -- "$prefix" "$previous"
        fi
        if ! mv -- "$stage" "$prefix"; then
            if owned "$previous"; then mv -- "$previous" "$prefix"; fi
            die "no se pudo activar la instalación"
        fi

        # Desktop Entry Exec escaping plus escaping literal field codes.
        exec_path="$prefix/bin/wyss-console"
        exec_path=${exec_path//\\/\\\\}
        exec_path=${exec_path//\"/\\\"}
        exec_path=${exec_path//\$/\\\$}
        exec_path=${exec_path//\`/\\\`}
        exec_path=${exec_path//%/%%}
        exec_path=${exec_path//\\/\\\\}
        cat > "$desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Wyss Console
GenericName=Terminal
Comment=Terminal y espacio de trabajo GTK4
Exec="$exec_path"
Icon=utilities-terminal
Terminal=false
Categories=System;TerminalEmulator;
Keywords=wyss;consola;console;terminal;nativa;native;
StartupWMClass=dev.wyss.Console
X-Wyss-Native=true
DESKTOP
        ln -sfn -- "$prefix/bin/wyss-console" "$link"
        printf 'Instalado en %s\n' "$prefix"
        ;;
    rollback)
        check_slot "$prefix"
        check_slot "$previous"
        owned "$prefix" && owned "$previous" || die "se necesitan dos instalaciones"
        stage=$(mktemp -d "$prefix.rollback.XXXXXX")
        mv -- "$prefix" "$stage/current"
        if ! mv -- "$previous" "$prefix"; then
            mv -- "$stage/current" "$prefix"
            rmdir -- "$stage"
            die "no se pudo restaurar la versión anterior"
        fi
        mv -- "$stage/current" "$previous"
        rmdir -- "$stage"
        printf 'Versión anterior restaurada en %s\n' "$prefix"
        ;;
    uninstall)
        check_slot "$prefix"
        check_slot "$previous"
        if [[ -f "$desktop" ]] && grep -qx 'X-Wyss-Native=true' "$desktop"; then
            rm -- "$desktop"
        fi
        if [[ -L "$link" && $(readlink "$link") == "$prefix/bin/wyss-console" ]]; then
            rm -- "$link"
        fi
        for slot in "$prefix" "$previous"; do
            if owned "$slot"; then rm -rf -- "$slot"; fi
        done
        printf 'Instalación de Wyss Console eliminada\n'
        ;;
    status)
        if owned "$prefix"; then
            printf 'Instalado en %s\n' "$prefix"
            (cd -- "$prefix/bin" && sha256sum --check ../SHA256)
            if owned "$previous"; then printf 'Hay una versión anterior en %s\n' "$previous"; fi
        else
            printf 'Sin instalación administrada en %s\n' "$prefix"
        fi
        ;;
    *) die "uso: $0 install|rollback|uninstall|status" ;;
esac
