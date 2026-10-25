# M1–M5: integración final y cierre seguro

**Estado: INTEGRATED / PRODUCTION NO-GO.** Detención explícita solicitada por Santiago. Corte de estado: **2026-10-25 02:17:42 UTC / 2026-10-24 23:17:42 Córdoba**. No se continúa esperando CI ni se ejecutan más pruebas después del corte. El workflow remoto que ya estaba iniciado puede terminar autónomamente; su resultado posterior no está certificado por este informe.

## 1. Main y estado Git

- Repositorio: SaintWyss/wyss-console.
- Main remoto y checkout de inspección: **`0e9a5341a9df4e6169436a1d52320ce8548c031c`**.
- [PR #27 fusionado](https://github.com/SaintWyss/wyss-console/pull/27) mediante merge normal, con guarda `expected_head_sha=51b3fbeb9c42ed3216b7b1fe27ad77263899a48b`.
- Padres comprobados del merge: `def69fe35ddfe8b696b65a12d2f2babe296c8247` y `51b3fbeb9c42ed3216b7b1fe27ad77263899a48b`.
- Árbol: `1bd3d238d89854db76a0ceb2bdc0d60a6a07b213`, idéntico al candidato y al merge sintético validado.
- Main no avanzó inesperadamente después del merge, según la última consulta remota. Checkout limpio en `/home/saintwyss/Github_repos/wyss-console-final-integration`, rama `main`.
- Este informe se guarda en una rama documental **local**, sin modificar ni avanzar main. No se publica ni se fusiona el informe durante la detención.

## 2. GitHub Actions: evidencia exacta

| Estado | Revisión y evidencia |
| --- | --- |
| **PASS completo** | [40150262201](https://github.com/SaintWyss/wyss-console/actions/runs/40150262201), candidato exacto `51b3fbe` antes del merge. Ambos jobs SUCCESS. |
| **EN CURSO al corte** | [40152262031](https://github.com/SaintWyss/wyss-console/actions/runs/40152262031), commit real de main `0e9a534`. Frontend SUCCESS; Rust en la última suite all-features. Formato, Clippy, V9, handoff, B1 temprano, suite por defecto y exclusión de fixtures ya habían pasado. **No se afirma PASS global de esta corrida.** |

Resultados de la corrida completa del candidato:

- Frontend: formato, ESLint sin warnings, Svelte/TypeScript, **1.056 tests en 66 archivos**, build de producción y repo-scan de 1.576 archivos; 16 tests Python de ownership y 77 de bridge/broker.
- Rustfmt y Clippy `--all-targets --all-features -- -D warnings`: PASS.
- Suite por defecto: **2.153 PASS, 0 FAIL, 57 ignored**, 23 harnesses.
- Suite all-features: **2.195 PASS, 0 FAIL, 57 ignored**, 28 harnesses.
- B1 process: **22/22**, con `WYSS_B1_REQUIRE_RUN=1`; sin NOT_RUN. Ejecutado temprano y nuevamente en la suite completa.
- V9 productivo focalizado: **3/3**. Handoff multiproceso con `f8-fixture`: **1/1**.
- Cuatro binarios por defecto (`wyss-console`, `wyss-control`, `wyss-screen`, `wyss-credential-agent`): sin marcadores fixture.
- Los ignored incluyen pruebas manuales, proveedores/configuración/credenciales reales, mediciones y siete pruebas de systemd privado. **Ignored no equivale a PASS.**

[Resumen por suites](evidence/m1-m5-integration-safe-stop-2026-10-24/validation-summary.json).

## 3. Historia M1–M5 preservada

Verificados como ancestros de main: M2 `977cb8f`, M3 `d52705c`, integración `6c944dd`, M4 `0a8fb60`, M5 `42c73aa`, correcciones `ac04742`, checkpoint `f90d6e5` y candidato final `51b3fbe`.

M1 había llegado por squash. Se comprobaron iguales los árboles completos de cada original y su squash:

| Original | Incorporado previamente en main |
| --- | --- |
| `6fd3b27` | `4829d6c` |
| `817bc95` | `e3af6ff` |
| `fddbb42` | `def69fe` |

El merge de ascendencia `5eb3f69` conserva los tres historiales originales sin cambiar un byte del árbol de código. Hay 100 commits del candidato no alcanzables desde la base previa, más el merge final. No se rebaseó, squasheó ni reescribió main. [Inventario exacto](evidence/m1-m5-integration-safe-stop-2026-10-24/integrated-commits.txt).

## 4. Fallos investigados y correcciones

| Problema reproducido/observado | Clasificación y corrección |
| --- | --- |
| Credential handoff verde con **cero tests** | Defecto de configuración CI: se agregó la feature requerida `f8-fixture`; ahora se ejecuta y pasa el E2E. |
| All-features no formaba parte de la ejecución | Cobertura incompleta: se incorporó suite completa y B1 obligatorio, sin eliminar suites previas. |
| Linker SIGBUS y `No space left on device`, runner con 29 MB | Capacidad del runner: sólo se omite debuginfo de perfiles dev/test; assertions y overflow checks permanecen. La corrida final tenía 8,5 GB libres antes de all-features. |
| B1 decía que no había namespaces, pero fallaba `/usr/bin/true` por ENOENT | Defecto del detector: `/lib64 -> usr/lib` era propio de CachyOS, incompatible con Ubuntu. Se reprodujo con filesystem Ubuntu mínimo descartable: alias viejo exit1, correcto exit0. El test obtiene ahora los aliases del host como ya hacía el plan B1. **Plan y límites del candidato sin cambios.** |
| Fixture de ownership dejaba un `sleep` huérfano en Ubuntu | La shell señuelo esperaba un hijo que sobrevivía al cleanup. Ahora se bloquea en builtin `read` con pipe y conserva el mismo ejecutable shell. 16 tests PASS y cleanup CI sin aquel huérfano. |
| Documentación de rollout decía que V9 no existía y GUI hacía bootstrap | Runbook reconciliado; informe del paquete `48396c4` marcado histórico. Sus hashes no califican el árbol actual. |

También se examinaron logs históricos: Bubblewrap viejo o instalado fuera de `/usr/bin/bwrap`, restricciones de namespaces del runner, reutilización del inode de socket en un test y formato Rust. Las correcciones previas están preservadas. No se rerunearon fallos sin examinar su causa.

## 5. Seguridad y calificación V9/credenciales

- **V9 sintético: PASS** en identidad `codex/codex` y `claude/claude_code`, persistencia y replay mixto, corrupción de atribución, interrupciones, backup, rechazo por validador V8 y recuperación de un crash real de proceso. La preparación explícita genera copias nuevas; no descubre ni sustituye un store instalado.
- Restaurar el backup V8 no conserva trabajo V9 posterior. Cualquier pérdida o reconciliación requiere decisión explícita del dueño; cambiar un binario no es rollback de datos.
- **Handoff/TLS sintéticos: PASS**: MAC, binding de runtime/generación/perfil/record, uso único, secreto sellado, broker propio, rechazo de token/certificado incorrecto y TLS local verificado. No se utilizaron claves reales.
- **NOT_PROVEN antes de producción**: consentimiento nativo final positivo/negativo, autenticación/carrera del padre, expiración real, revocación durante diálogo y activación operativa completa. El wiring real de autoridad/broker está pendiente y los gates siguen cerrados.
- GUI usa `AuthorityBootstrap::observe_only`: ausencia y Retry no instalan ni inician autoridad. Esa lógica y su regresión pasan; la ejecución gráfica final de `--gui-only`, sus efectos y descendientes siguen NOT_PROVEN.
- Approved-write conserva el intervalo último chequeo/rename frente a escrituras del mismo UID. No se afirma CAS libre de carreras ni autenticidad humana absoluta frente a procesos hostiles de esa cuenta.
- Seccomp/x32 y los archivos de executor/helper revisados respecto de `48396c4` se retienen; suites de proceso y sandbox pasan. La capacidad productiva sigue siendo uno. B1 sigue excluido del producto por defecto.
- No se confirmó un nuevo blocker crítico/alto para integrar fuente en el alcance revisado. Esto no equivale a calificación productiva ni a ausencia demostrada de defectos.
- Revisión **independiente estática enfocada del harness/CI**: PASS, incluidos detector B1 y paso temprano. No cubre todo el producto ni reemplaza la revisión independiente final antes de activar producción.
- `npm audit --omit=dev`: cero vulnerabilidades. Auditoría completa: dos paquetes de desarrollo con avisos altos (Vite/launch-editor, alto aplicable a Windows; undici vía jsdom). Son deuda del tooling, sin actualización automática ni inclusión en el producto empaquetado. Fuentes: [Vite](https://github.com/advisories/GHSA-587h-7wmj-h5pm), [undici](https://github.com/advisories/GHSA-xjj6-f33q-929c).

## 6. PRs y ramas

- #22–#24: GitHub los marcó incorporados automáticamente a través de #27.
- #25–#26: cerrados como supersedidos con explicación y prueba de ascendencia. Ninguno se fusionó por separado.
- Único PR abierto: [#13](https://github.com/SaintWyss/wyss-console/pull/13), con trabajo exclusivo.
- **17 ramas remotas eliminadas** mediante transacción atómica y lease SHA por rama, todas con su historia completa en main. [Lista exacta y motivos de preservación](evidence/m1-m5-integration-safe-stop-2026-10-24/cleanup-plan.json), [recibo Git](evidence/m1-m5-integration-safe-stop-2026-10-24/remote-cleanup-receipt.txt).
- **6 ramas locales libres eliminadas**. Se conservaron todas las ramas de worktrees ocupados; ningún árbol ajeno se reseteó, limpió, rebaseó o sobrescribió. [Detalle local](evidence/m1-m5-integration-safe-stop-2026-10-24/local-cleanup.json).

Remotos restantes al corte:

| Rama | Motivo |
| --- | --- |
| `main` | Fuente autoritativa `0e9a534`. |
| `feat/native-e2e-fixture-harness` | 48 commits exclusivos; PR #13 abierto. |
| `prototype/b1-synthetic-contract-2026-10-14` | 4 commits exclusivos; referencia pendiente. |
| `feat/b1-pure-contracts-2026-10-15` | Worktree con `src-tauri/src/contracts/b1.rs` sin seguimiento, preservado sin leer ni modificar su contenido. |

La nueva rama documental local de este informe es una excepción explícita de cierre seguro solicitada por Santiago; no se publica ni altera el estado remoto anterior.

## 7. Funcionalidad y límites pendientes

| Milestone/área | Implementado en fuente | Gated o sin calificación final |
| --- | --- | --- |
| M1 | Proyectos, workspaces, store, identidad/ownership, UI y restauración. | Activación y mediciones del paquete final requieren aceptación correspondiente. |
| M2 | Paridad Native y configuración. | Aceptación Native gráfica del artefacto final. |
| M3 | Adaptador Claude, broker, modelos/uso/historial y fixtures offline. | Claude Managed real, wiring operativo, política de métricas, selección gated y evidencia real. |
| M4 | Persistencia de pantalla, helper aislado, reattach/recovery y fallback. | Repetir GUI y presupuestos sobre el paquete final. |
| M5 | Límites, ownership, accounting/capacidad y candidato B1 de fixtures. | B1 real, principal protegido/OD02 y capacidad por encima de uno. |
| V9/credenciales/GUI | Código V9, preparación offline, agente protegido y observación GUI. | Migración live, consentimiento/activación real y calificación gráfica/productiva. |

ADR0053–0058, OD02/OD-M2-01 y decisiones del dueño no se aprobaron por el merge. No se habilitó lingering, B1 real, mayor concurrencia ni admisión Claude real.

## 8. Pasos exactos restantes

1. Consultar el resultado final de [40152262031](https://github.com/SaintWyss/wyss-console/actions/runs/40152262031). Al corte seguía en all-features; no reintentar un fallo sin leer sus logs. No se ejecuta este paso durante la detención.
2. Construir un artefacto nuevo del main final, con dependencias bloqueadas: `npm run desktop:package`. Verificar `source_revision`, inventario/digests y `npm run check:package-fixtures`. No usar el paquete ni los hashes históricos de `48396c4`.
3. Calificar en VM Linux/KDE descartable, sin HOME ni credenciales del dueño: GUI-only y reconexión sin bootstrap, ausencia de mutaciones de servicios, M4 recovery/fallback, procesos propios y capacidad M5 uno.
4. En esa VM, ejecutar las siete pruebas de lifecycle con manager privado operativo: `cargo test --locked --manifest-path src-tauri/Cargo.toml --test lifecycle_process -- --ignored --test-threads=1`. Si no puede iniciarse ese manager, conservar NOT_PROVEN y corregir el harness; no apuntarlo al bus del dueño.
5. Calificar diálogo nativo sintético positivo/negativo y restantes propiedades de credenciales. El procedimiento detallado y límites están en [aceptación pendiente](evidence/m1-m5-integration-safe-stop-2026-10-24/acceptance-pending.md). El script histórico `isolated-secret-service.sh` está retirado y no acredita un keyring privado disponible.
6. Completar revisión independiente final del producto y resolver las decisiones aplicables antes de abrir cualquier gate. La revisión independiente del harness no sustituye ese trabajo.
7. Ensayar instalación/rollback únicamente en la VM. Obtener autorización explícita de Santiago antes de instalar localmente, migrar stores, reiniciar servicios, admitir credenciales/proveedores reales o cambiar capacidad.

**No se instaló ni actualizó la aplicación de producción, no se reiniciaron autoridades/hosts/servicios, no se tocaron bases o sesiones productivas, no hubo migración live ni inferencia paga.** La integración de fuente está terminada; la comprobación automática post-merge quedó en curso por la detención. Producción sigue NO-GO.
