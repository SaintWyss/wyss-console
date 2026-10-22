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

**Runtime de producción ensamblado, aún sin activar (cierre independiente):**

- Sin `--codex`, la autoridad conserva `NoRuntime` y rechaza inicios.
- `--codex RUTA_ABSOLUTA --codex-home DIRECTORIO` selecciona explícitamente el spawner nativo
  local-tools. El ejecutable sellado debe responder exactamente `codex-cli 0.159.0`; no hay fallback.
- Una credencial sólo puede ligarse si el operador añadió `--allow-credential`. Se verifica la
  capacidad realmente adquirida, además del chequeo inicial del directorio. **No usar ese flag
  con credenciales reales sin autorización de Santiago para la aceptación autenticada y su cuota.**
- Preparación/sellado/probe fuera del hilo del writer. Cancelación retiene slot y staging hasta
  finalizar la preparación y demostrar reap; readiness exige el handshake y las attestations existentes.
- Este spawner admite sólo `codex_local_tools_v1` con workspace. La ruta de inferencia existente
  sigue probada por sus fixtures, pero no se selecciona mediante este spawner de producción.

**Pendiente:** la aceptación gráfica actual y las condiciones NOT_PROVEN del
[informe de cierre](f8-closeout-2026-10-21.md), además de las autorizaciones siguientes.
No se creó servicio systemd ni lanzador activo de protocolo 2. La base instalada V7 sigue siendo
la fuente de protocolo 1; ninguna de estas opciones la migra automáticamente.

## Precondiciones (todas)

- ADR 0051 aceptado por Santiago.
- Ventana de migración y de eventual rollback autorizada por Santiago, con la interrupción de
  sesiones vivas que implique.
- Aceptación autenticada del proveedor (Codex real) autorizada y hecha: **NOT_PROVEN** hoy.
- Compatibilidad de refresco de credenciales de Codex con varios procesos: **NOT_PROVEN**.
- Confirmación W1 implementada: aceptación y leases ejecutables exigen diálogo GTK leído desde
  Rust. Lectura fallida o fence/lease diferente impide mostrar Allow. Falta completar su validación
  gráfica actual en el escritorio real; el riesgo de inyección X11 por el mismo uid sigue declarado.
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
4. No iniciar un proveedor real hasta autorizar y completar su aceptación aislada autenticada,
   incluida la contención local-tools en el host. El spawner CLI actual no ofrece Inference;
   no sustituir el perfil ni debilitar el aislamiento para sortear ese rechazo.
5. Criterio de abandono: cualquier `unavailable` sostenido, deriva de recuentos o fallo de
   auditoría → detener y ejecutar el rollback.

## Qué hizo y qué no hizo la entrega de instalación (2026-10-21)

Instaló el paquete (`--keep-host`, sin interrumpir la GUI, el host ni la autoridad abiertos), respaldó
el estado y ensayó la migración V7→V8 sobre **una copia** del respaldo. No migró ni abrió la base viva,
no reinició ninguna autoridad ni host, no tocó credenciales ni ajustes, no lanzó Codex y no creó
ningún servicio ni lanzador de protocolo 2. Las pruebas de ventana usaron una base V8 temporal, un display
anidado propio y un runtime de fixture.
