# F8: instrucciones de migración, respaldo, rollback y activación (NO AUTORIZADAS)

Este documento prepara una decisión del dueño. **Nada de lo que sigue fue ejecutado ni está
autorizado.** El entorno instalado sigue en protocolo 1 y SQLite V7; ADR 0051 sigue _Proposed_.
Fuente completa no es producción lista.

## Qué falta además de la autorización

El binario instalado selecciona `LocalSelection::InstalledProtocol1` y no existe ningún camino
en el código de producción que construya un `PreparedApplication` instalado ni un `Connector` de
ventana (`run_with_f8` y el ejemplo `f8_local_gui` son sólo de desarrollo, tras `f8-dev`).
Activar exige, por lo tanto, **un cambio de código separado y revisado** que:

1. abra la base instalada migrada con `migration::prepare` y entregue el `PreparedApplication`
   a `run_selected(PreparedProtocol2)` en el proceso `wyss-control`;
2. cree el listener local del socket de protocolo 2 con permisos privados;
3. construya en la ventana un `SocketConnector` hacia ese socket y llame a `run_with_f8`;
4. habilite la capacidad `f8` (overlay `tauri.f8.conf.json`) sólo en esa compilación.

Ese cambio debe pasar su propia revisión independiente antes de cualquier instalación. La
capacidad `main` instalada hoy sólo concede `allow-f8-state` (informa `active:false`).

## Precondiciones (todas)

- ADR 0051 aceptado por Santiago.
- Ventana de migración y de eventual rollback autorizada por Santiago, con la interrupción de
  sesiones vivas que implique.
- Aceptación autenticada del proveedor (Codex real) autorizada y hecha: **NOT_PROVEN** hoy.
- Compatibilidad de refresco de credenciales de Codex con varios procesos: **NOT_PROVEN**.
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

## Qué hizo y qué no hizo esta entrega sobre el entorno instalado

La configuración base del repositorio (`tauri.conf.json`) sigue listando sólo la capacidad `main`
y el código de producción selecciona protocolo 1. Todas las pruebas y las capturas de la
ventana usaron bases temporales aisladas, un display anidado propio y un fixture sin
credenciales. No se reinició ninguna ventana, autoridad ni host de terminal instalados, no se
abrió ni migró la base instalada, no se tocaron credenciales ni ajustes, y no se lanzó Codex
autenticado.
