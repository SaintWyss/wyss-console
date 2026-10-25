# Integración M1–M5: evidencia y límites

Candidata de integración: 5eb3f69a45472403c35c6b2b6588047ad5aaa4e4.
Base remota observada: def69fe35ddfe8b696b65a12d2f2babe296c8247.
CI exacta: https://github.com/SaintWyss/wyss-console/actions/runs/40146626160

## Historia

M2 977cb8f, M3 d52705c, integración 6c944dd, M4 0a8fb60, M5 42c73aa y correcciones ac04742 son ancestros de la candidata. M1 había llegado por squash: los árboles completos 6fd3b27/4829d6c, 817bc95/e3af6ff y fddbb42/def69fe son respectivamente idénticos. El merge 5eb3f69 conserva sus tres ascendencias originales, sin cambiar el árbol ac7bf6be43eec5d8c1f76332f6f888c109454b90 de 51dc23e. Hay 97 commits alcanzables desde la candidata y no desde la base main.

No se modificó ningún otro worktree. `wyss-console` contiene `?? src-tauri/src/contracts/b1.rs`; esa rama local y su remoto deben conservarse mientras ese trabajo no tenga destino. PR #13 conserva 48 commits exclusivos; el prototipo B1 conserva cuatro.

## Cambios de esta integración

- CI de handoff habilita f8-fixture: antes ejecutaba cero tests.
- CI ejecuta suite all-features y exige WYSS_B1_REQUIRE_RUN=1, sin convertir falta de plataforma en PASS.
- CI verifica ausencia de marcadores fixture en los cuatro binarios por defecto.
- Fixture Python de ownership usa un read bloqueante de la propia shell como señuelo; matar una shell que espera sleep dejaba el sleep huérfano en Ubuntu. Reproducción descartable con shell retenida y pidfd; limpieza del hijo propio mediante su descriptor. Los 16 tests pasan tras la corrección.
- Runbook actualizado y antigua calificación de paquete marcada como histórica.
- No se cambió código Rust/productivo, dependencias, gates ni servicios.

## Causas de CI investigadas

| Evidencia | Clase | Causa y tratamiento |
| --- | --- | --- |
| 40142178968 | Formato | rustfmt exigía expandir assertions GUI; corrección f90d6e5 ya presente. |
| 40140425307 | Plataforma no calificada | Siete pruebas no podían iniciar systemd privado; siguen explícitamente NOT_PROVEN. No se ejecutaron contra el bus del dueño. |
| 40138571762 | Fixture no determinista | Reutilización del inode de socket en test que simula otro host en el mismo proceso; a3d4e82 retiene el socket retirado. |
| 40135972053 | Herramienta/ruta | /usr/bin/bwrap viejo no aceptaba --overlay-src; CI compila versión fijada, verifica hash y la instala sólo en runner descartable. |
| 40144222131 | Hueco del harness de CI | Corrida verde con cero tests en credential_handoff_process; corregido pasando la feature requerida. |

## Revisión enfocada de seguridad

Lectura del delta 48396c4..candidata: V9/provider, schema/replay, writers/allocator/engine/operations, store/contratos, agente y fuente de credenciales, vault, lifecycle/GUI, packaging y cambios de Clippy. Retención exacta respecto de 48396c4 de claude/exec.rs, screen/sandbox.rs y screen/helper.rs. No nuevos bloqueantes críticos/altos confirmados para integrar fuente. No es revisión independiente de producción ni prueba de ausencia de defectos.

V9 valida esquema exacto, vocabulario, identidad cerrada por proveedor y frontera histórica; el replay y recuperación conservan rechazo V8 de datos Claude. Migración offline trabaja sobre copia, outputs nuevos y backup. Restaurar V8 descarta cualquier trabajo V9 posterior al backup: requiere decisión explícita, no sólo cambiar un binario.

Credenciales: gates de producción preceden vault/parent-auth; canal por pipes heredados, capability aleatoria, HMAC vinculado a runtime/generación/perfil/record, uso único por fuente y deadlines. Broker de producto mantiene origen Anthropic fijo y namespace. El E2E sintético no califica consentimiento humano, expiración ni carrera del padre. El wiring real aún no tiene caller operativo. La memoria y el mismo UID no reciben garantía absoluta de no divulgación.

GUI: observe_only y retry inerte; no bootstrap implícito. La aceptación gráfica final, incluidos efectos y descendientes de --gui-only, sigue NOT_PROVEN. Capacidad de producción uno. B1 feature-only. Seccomp/x32 conservados; runtime se califica en las suites de proceso.

Approved-write conserva el intervalo último chequeo/rename frente al mismo UID. No se afirma CAS libre de carreras. Gates Claude real, B1 real, capacidades mayores, lingering y ADR/OD pendientes no se abrieron.

`npm audit --omit=dev`: cero vulnerabilidades. Auditoría completa: dos paquetes de desarrollo con severidad alta, Vite (vía launch-editor, alto afecta Windows) y undici vía jsdom. No forman parte del producto empaquetado; siguen deuda del tooling, sin upgrade automático en esta integración.

## Antes de instalar

Procedimiento exacto: acceptance-pending.md junto a este archivo. Paquete nuevo y hashes del SHA final, VM descartable, systemd privado, GUI/M4/M5, consentimiento sintético positivo/negativo, revisión independiente, decisiones del owner y rollback. No usar hashes de 48396c4 ni el script Secret Service retirado como evidencia actual. Producción NO-GO.

## Evidencia de frontend final

Job https://github.com/SaintWyss/wyss-console/actions/runs/40146626160/job/121029306459: PASS, 66 archivos Vitest/1.056 tests, Svelte sin errores ni warnings, lint/formato, 16 tests Python ownership, 77 bridge/broker, build y repo-scan (1.576 archivos). El cleanup del runner no registró un proceso sleep huérfano tras corregir el fixture. `npm run check` y `scan:repo` también pasaron localmente; ningún binario GUI fue ejecutado en esta sesión.

## Reconciliación del último fallo

40146626160: V9 3/3, handoff 1/1 y suite por defecto PASS, incluida exclusión de fixtures. All-features no llegó a ejecutar tests: linker SIGBUS seguido de `No space left on device`, runner con 29 MB. Clasificación: capacidad del runner, no defecto confirmado de la aplicación. a68e786 omite sólo debuginfo de perfiles dev/test; assertions, overflow checks y la suite íntegra permanecen. Evidencia de Cargo: https://doc.rust-lang.org/cargo/reference/profiles.html#debug . Nueva CI exacta: https://github.com/SaintWyss/wyss-console/actions/runs/40148252494 .

## B1: detector de capacidad y revisión separada

40148252494: default, V9 y handoff PASS; all-feature lib 1450 y bin 535 PASS. B1: 7 PASS/15 FAIL porque el detector bare creaba /lib64->usr/lib, alias propio de CachyOS, mientras Ubuntu necesita usr/lib64. El error era ENOENT del cargador, no falta de namespaces. El candidato ya obtiene los aliases de read_link del host (plan.rs sin cambios).

Reproducción local descartable con /usr mínimo de layout Ubuntu, mismo true/libc/loader y mismos namespaces: alias viejo exit1/ENOENT; alias correcto exit0. 9b603f4 aplica al detector de tests el mismo procedimiento portable que el plan. No cambia candidato ni assertions; REQUIRE_RUN sigue obligatorio. 51b3fbe ejecuta además B1 antes de la suite larga para detectar temprano cualquier fallo real; suite all-features completa retenida.

Revisor independiente focused_harness_review: PASS estático sobre el alcance f90d6e5..9b603f4 (workflow, ownership fixture, detector B1), sin omisión de tests/assertions ni ampliación del candidato. No ejecutó tests, no calificó producto ni otras modificaciones. Confirmación del último paso temprano pendiente de recibir; CI final sigue siendo obligatoria.

Confirmación independiente 9b603f4..51b3fbe: PASS estático, sólo agrega paso B1 temprano con todas las features y REQUIRE_RUN=1; suite íntegra, candidato y assertions sin cambios. CI final: https://github.com/SaintWyss/wyss-console/actions/runs/40150262201 sobre 51b3fbeb9c42ed3216b7b1fe27ad77263899a48b.

# Resultado de integración

INTEGRATED / PRODUCTION NO-GO. Main 0e9a5341a9df4e6169436a1d52320ce8548c031c. PR #27 fusionado mediante merge normal con expected_head_sha 51b3fbeb9c42ed3216b7b1fe27ad77263899a48b. Padres exactos: def69fe + 51b3fbe. Árbol 1bd3d238d89854db76a0ceb2bdc0d60a6a07b213, idéntico al candidato y al merge sintético validado. Todos los heads M1–M5 y correcciones verificados como ancestros. Checkout actual: main, limpio, en este worktree final-integration.

CI exacta del candidato 40150262201 PASS: frontend/contratos y Rust. Default 1435 lib +535 bin y proceso; all-features 1450 lib +535 bin y proceso. B1 22/22 sin NOT_RUN, V9 3/3, handoff 1/1. Cuatro binarios por defecto sin marcadores fixture. Antes de all-features había 8,5 GB libres. Detalle sumado por suites en validation-summary.json. Ignored no equivalen a PASS.

PRs #22–#24 absorbidos automáticamente por #27; #25–#26 cerrados por redundancia con explicación y verificación de cada SHA. Único PR abierto: #13.

17 ramas remotas eliminadas mediante una transacción atómica con lease de cada SHA; ver cleanup-plan.json y remote-cleanup-receipt.txt. 6 ramas locales libres eliminadas; ramas y archivos de todo worktree ocupado preservados (local-cleanup.json). Remotos restantes: main, feat/native-e2e-fixture-harness (48 commits exclusivos), prototype/b1-synthetic-contract-2026-10-14 (4), feat/b1-pure-contracts-2026-10-15 (trabajo local sin commit).

Implementado: M1 proyectos/workspaces, M2 paridad Native/configuración, M3 adaptador Claude/broker y UI offline con gates, M4 persistencia/recovery de terminal y parser aislado, M5 límites/ownership/capacidad y fixtures B1; más V9 y agente de handoff protegido. La existencia de código no habilita capacidades productivas.

Gated/pendiente: Claude Managed real y wiring operativo de autoridad/broker, consentimiento/expiración/parent-auth finales, revisión independiente de producto, ADR0053–0058/OD02, métricas/selección de modelo gated, B1 real y concurrencia mayor que uno. GUI final, lifecycle privado, empaquetado/digests/reproducibilidad/rollback del artefacto actual: NOT_PROVEN; procedimiento en acceptance-pending.md. Límite TOCTOU frente al mismo UID conservado y declarado. No se instalaron binarios, migraron stores live, reiniciaron servicios ni usaron credenciales o APIs pagas de proveedores.

CI automática del commit real de main 40152262031: en curso; se comprobará antes de cerrar esta ejecución. https://github.com/SaintWyss/wyss-console/actions/runs/40152262031
