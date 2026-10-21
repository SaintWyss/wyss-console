# F8: instrucciones de migración, respaldo, rollback y activación (NO AUTORIZADAS)

Este documento prepara una decisión del dueño. **Nada de lo que sigue fue ejecutado ni está
autorizado.** El entorno instalado sigue en protocolo 1 y SQLite V7; ADR 0051 sigue _Proposed_.
Fuente completa no es producción lista.

## Qué está hecho y qué falta además de la autorización

**Hecho (2026-10-21, ver [instalación](f8-installation-2026-10-21.md)):** el código de producción ya
tiene el cableado de protocolo 2 y está instalado, inerte:

- `wyss-control --protocol 2 --runtime-root D --state-root D [--create | --migrate-copy COPIA]` sirve
  una base V8 propia (`authority.sqlite3`) en un socket privado; sin argumentos sigue siendo protocolo 1.
- `wyss-console --f8-authority <runtime-root>` es la única forma de activar la interfaz de la ventana
  (conector de socket con verificación de versión y de esquema, sin TOCTOU de directorio).
- Las capacidades `main` + `f8` están listadas en `tauri.conf.json`; sin el flag todo responde `inactive`.
- W1: aceptar una aprobación y conceder/renovar un lease que ejecuta exigen **Permitir** en una ventana GTK.

**Falta, además de la autorización:**

1. **Un runtime real.** La autoridad de producción usa `NoRuntime`: prepara sesiones y leases pero no inicia
   ningún runtime. Ensamblar el `NativeLocalToolsPlan` de producción (hoy sólo existe en pruebas) y un
   `Spawner` real es un cambio de código separado, con revisión independiente, y lanza Codex autenticado
   (no autorizado).
2. Un servicio `systemd --user` para la autoridad de protocolo 2 y un lanzador que pase `--f8-authority`:
   no se crearon (crearlos es parte de la activación).
3. Que el protocolo 1 deje de escribir esa base: protocolo 1 y 2 usan archivos distintos, pero la
   migración significa que la V7 deja de ser la fuente (ver Migración y Rollback).

## Precondiciones (todas)

- ADR 0051 aceptado por Santiago.
- Ventana de migración y de eventual rollback autorizada por Santiago, con la interrupción de
  sesiones vivas que implique.
- Aceptación autenticada del proveedor (Codex real) autorizada y hecha: **NOT_PROVEN** hoy.
- Compatibilidad de refresco de credenciales de Codex con varios procesos: **NOT_PROVEN**.
- Decisión del dueño sobre el hallazgo W1 de la revisión (una ventana comprometida tiene la autoridad
  de aprobación, igual que en protocolo 1): aceptarlo o exigir antes confirmación nativa en Rust /
  token de lectura de un solo uso para `lease_grant` de comandos/archivos y `approval_accept`.
- `max_active_runtimes = 1`. Subirlo requiere evidencia de recursos y decisión del dueño.

## Respaldo (antes de migrar)

1. Detener la autoridad instalada por su ciclo de vida propio (no matar el host de terminal:
   `install.sh rollback` lo detiene y no es un rollback desatendido de la autoridad).
2. Probar escritor exclusivo (ningún otro proceso con la base abierta).
3. Respaldo con la API de respaldo consistente de SQLite (`VACUUM INTO` o `.backup`) a un
   archivo con permisos 0600 en un directorio 0700; nunca copiar sólo el `.db` vivo.
4. Verificar: `PRAGMA integrity_check`, `PRAGMA foreign_key_check`, versión de esquema 7,
   suma de verificación SHA-256 en un manifiesto junto con la versión del binario instalado.
5. Conservar el binario instalado anterior (`wyss-console.previous`).

## Migración V7 → V8

- Sólo sobre una copia o con escritor exclusivo; `migration::prepare` valida el esquema exacto,
  todos los registros y las claves foráneas antes de confirmar, y preserva stream, secuencia,
  eventos, recibos e identidad histórica (el vocabulario sube a 2 sin reescribir eventos).
- Ensayar primero sobre el respaldo (`VACUUM INTO` de la copia) y comparar recuentos de filas
  por tabla antes y después.
- Límites de por vida por base: 128 filas de clave de runtime y 4096 de asignación de lanzamiento;
  agotarlos devuelve `capacity_exceeded` hasta una decisión de retención revisada.

## Rollback

No existe downgrade SQL. Rollback = (1) detener la autoridad nueva por su ciclo propio,
(2) escritor exclusivo, (3) restaurar el snapshot completo verificado, (4) reinstalar el binario
anterior, (5) reconocer por escrito que se pierden las escrituras posteriores al snapshot.
Debe estar autorizado por Santiago, con tiempo de inactividad.

## Activación (sólo tras todo lo anterior)

1. Instalar la compilación con el cambio de código revisado y la capacidad `f8`.
2. Arrancar la autoridad con protocolo 2 y confirmar `hello` v2 y `authority_instance_id`.
3. Abrir "Runtimes" en la ventana: debe mostrar "Connected · protocol 2".
4. Primer runtime sólo en perfil `Inference` y sesión de prueba; sin Local Tools hasta aceptar
   el aislamiento en el host real.
5. Criterio de abandono: cualquier `unavailable` sostenido, deriva de recuentos o fallo de
   auditoría → detener y ejecutar el rollback.

## Qué hizo y qué no hizo la entrega de instalación (2026-10-21)

Instaló el paquete (`--keep-host`, sin interrumpir la GUI, el host ni la autoridad abiertos), respaldó
el estado y ensayó la migración V7→V8 sobre **una copia** del respaldo. No migró ni abrió la base viva,
no reinició ninguna autoridad ni host, no tocó credenciales ni ajustes, no lanzó Codex y no creó
ningún servicio ni lanzador de protocolo 2. Las pruebas de ventana usaron una base V8 temporal, un display
anidado propio y un runtime de fixture.
