# Arsenal H — home por lanzamiento: matriz de compatibilidad

Investigación del 2026-11-13. Decide el mecanismo de [Arsenal H](../contracts/arsenal-UI-SPEC.md#decisión-del-2026-11-13-home-por-lanzamiento).
Versiones medidas: **Claude Code 2.1.323**, **codex-cli 0.167.0**, en la .18. Ninguna prueba tocó Wyss Console, `~/.claude/settings.json`, `~/.codex/config.toml` ni las credenciales (mtime y tamaño verificados antes y después). La evidencia cruda (stream-json de cada corrida) quedó fuera del repo porque incluye rutas y contexto de sesión.

## Objetivo

Que cada terminal cargue solo lo que el preset eligió: skills, reglas, CLAUDE.md/AGENTS.md, MCP, hooks y plugins. Lo que no se elige no consume contexto ni provoca reintentos. Una capacidad desmarcada **no puede figurar como aislada si el provider todavía la carga**.

## Mecanismo

Cada lanzamiento tiene una carpeta propia (`CLAUDE_CONFIG_DIR` para Claude; `CODEX_HOME` para Codex, con el `HOME` real). La Console regenera la composición en cada lanzamiento (settings, links, config). El estado de sesión vive en la misma carpeta y **persiste**: no se borra al cerrar, porque sin él no se puede retomar.

## Claude Code 2.1.323

| Fuente | ¿Se aísla? | Cómo |
|---|---|---|
| Login | ✅ | symlink a `~/.claude/.credentials.json` |
| CLAUDE.md / `rules/` globales | ✅ | no se ponen en el home. Sobre el home global, `claudeMdExcludes` (vía `--settings`) saca ~18,6k tokens |
| Skills del usuario | ✅ | link de directorio solo a la elegida |
| Skills incluidas (17) y `anthropic-skills:*` de la cuenta (13) | ✅ | `skillOverrides: {"<nombre>": "off"}` |
| Plugins sincronizados de la cuenta (design, engineering) y sus MCP | ✅ | **se descargan solos al home desde la 2ª corrida**; `enabledPlugins: {"design@synced": false, "engineering@synced": false}` |
| MCP | ✅ | `--strict-mcp-config --mcp-config <solo los elegidos>` |
| Hooks | ✅ | el `settings.json` del home trae solo los elegidos |
| Memoria automática | ✅ | `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` (si no, se crea dentro del home) |
| Plugins builtin `cc-plugin-*` | ❌ siempre cargan | peso bajo |
| CLAUDE.md, skills y hooks **del repo** | ❌ siempre cargan (correcto) | mostrarlos como "del repo" |
| Retomar sesión | ✅ desde el mismo home | otro home: `No conversation found` |
| Dos terminales a la vez | ✅ | homes distintos no se pisan |

| Medición | Home global | Home mínimo |
|---|---|---|
| Contexto inicial ("OK") | 63.042 tok · $0,435 · 12,1 s | **12.596 tok · $0,017** · 6,4 s |
| Tarea de 3 comandos Bash | 127.686 tok · $0,450 · 16,9 s | **26.365 tok · $0,036 · 6,9 s** |
| Tarea `git clean -f` (2 comandos) | 191.473 tok · $0,468 · 38,0 s | **38.704 tok · $0,035 · 9,3 s** |

Cada número sale de una sola corrida. El ahorro sigue la misma dirección en las tres tareas, pero no es una medición estadística.

### Hooks: qué costaron

En la tarea de 3 comandos, los hooks globales sumaron 5 eventos SessionStart, 5 reescrituras de comando de RTK (`rtk hook claude`), 2 inyecciones del gate de routing ("Routing gate ALLOW…", unos 600 caracteres cada una), 3 PostToolUse y 1 Stop: **11.528 caracteres de hooks en el transcript**. No hubo rechazos: el gate de routing deja pasar los comandos de riesgo bajo e inyecta texto.

**`r3-gate` no está activo.** `~/.claude/hooks/r3-gate.py` existe, pero no figura en `~/.claude/settings.json`; existe además un respaldo `settings.json.bak-r3-off-20261010`. Por eso, en la prueba con hooks globales, `git clean -f` corrió sin pedir consentimiento. `invariantes.md` lo describe como activo. **El dueño confirmó el 2026-11-13 que queda apagado a propósito.** Consecuencia para Arsenal: hoy un preset mínimo no saca ninguna protección destructiva, porque el global tampoco la tiene.

## Codex 0.167.0

| Fuente | ¿Se aísla? | Cómo |
|---|---|---|
| Login | ✅ | symlink a `~/.codex/auth.json` |
| AGENTS.md global | ✅ | no se pone en el home |
| `~/.agents/skills` (178) | ✅ | con solo `CODEX_HOME` carga las 182. **Con `HOME` real**, un `[[skills.config]] enabled = false` por cada `SKILL.md` (ruta del link y ruta resuelta) deja solo la elegida: 2/2 corridas, 13.562 tokens. Aislar `HOME` también funciona, pero rompe git (sin identidad), `gh` (sin sesión) y cargo/rustup (sin toolchain) si no se enlazan los dotfiles y se exportan `RUSTUP_HOME`/`CARGO_HOME`: **se descarta** |
| Skills incluidas `.system` (5) | ✅ | `[[skills.config]] path = "<home>/skills/.system/<x>/SKILL.md"` · `enabled = false` |
| Skill elegida | ✅ copiada | **un `SKILL.md` que es symlink no se carga**; copiado sí |
| Plugins de la cuenta (figma, notion, drive…) y el MCP `codex_apps` | ✅ **solo con features** | `[features] apps = false · plugins = false · remote_plugin = false · plugin_discovery_prefetch = false`: 4/4 corridas limpias. Con solo `[plugins."x@openai-curated-remote"] enabled = false` cargaban en 2 de 4 corridas (14k vs 18–42k tokens) |
| MCP | ✅ | solo los `[mcp_servers.*]` del config del home (4/4: solo `context7`) |
| Hooks del home | ✅ con confianza | sin confianza no corren (confirmado). Con `--dangerously-bypass-hook-trust` corre el hook del home y **no** el de un repo sin confianza. El `trusted_hash` no coincide con sha256 del comando ni del handler: hay que obtenerlo de Codex. Decisión del dueño (2026-11-13): registrar confianza entra en H |
| AGENTS.md y skills **del repo** | ❌ siempre cargan (correcto) | |
| Config administrada `/etc/codex` | ❌ siempre carga | hoy solo `hooks = true` |
| Retomar sesión | ✅ desde el mismo home | home global: `no rollout found` |
| Dos terminales a la vez | ✅ | 14.578 y 13.900 tokens, sin cruce |
| Helpers PATH | ⚠️ | Codex no los crea si el home está bajo `/tmp`: el home real va en `~/.local/state/…` |

| Medición | Input tokens |
|---|---|
| Home global | 42.648 |
| Solo `CODEX_HOME` | 23.874 |
| `CODEX_HOME` + `HOME` | 18.697 |
| + `.system` y features de apps/plugins apagados | **13.720** |

## Credenciales: renovación sin probar

Los tokens estaban vigentes durante las pruebas (Claude 7,6 h, Codex 4 días), así que ningún home renovó. **No se probó qué pasa al renovar**, porque no hay forma segura de forzarlo con la credencial real. El riesgo: si el provider escribe la credencial renovada con rename, el symlink se reemplaza por un archivo y el global se queda con un refresh token viejo; si el provider rota refresh tokens, la sesión global se invalida. Opciones a evaluar: que la Console detecte que el link se rompió y lo restaure moviendo el archivo nuevo al original (AUTH-preserving, a confirmar), o usar un token de larga duración por variable de entorno (AUTH-modifying: requiere autorización del dueño).

## Preflight de modelo rechazado (2026-11-13)

Los dos DENY "Malformed model-preflight bootstrap" fueron **correctos**: el agente agregó `2>&1 | head` y `| tail`, y el gate (`check-route-execution.py:_model_preflight_call`) rechaza cualquier comando con `; & | < > $` o salto de línea. Se reprodujo sin ejecutar nada: con pipe → `None`, sin pipe → válido. El preflight sin pipe se emitió bien. No es una degradación ni un control que falló.

## Condiciones de aceptación

| # | Condición | Claude | Codex |
|---|---|---|---|
| 1 | Login sin duplicar ni volver a iniciar sesión | PASS | PASS |
| 2 | El mínimo excluye todo lo no elegido | PASS (salvo `cc-plugin-*`) | PASS con features apagados |
| 3 | Las fuentes del repo se identifican aparte | PASS | PASS |
| 4 | Costo real de tarea, reintentos y tiempo | PASS en tokens y tiempo; reintentos: 0 rechazos medibles con los hooks actuales | solo contexto inicial |
| 5 | Concurrencia y retomar sin tocar el global | PASS | PASS |
| — | Renovación de credencial | PENDIENTE | PENDIENTE |
| — | Hooks por home | PASS | PASS con confianza (mecanismo del hash pendiente) |

## Reglas para implementar H

1. El home es persistente por lanzamiento (`~/.local/state/wyss-console/launch-homes/<id>`); lo volátil es la composición.
2. En Codex, `HOME` **no** se aísla: las skills de `~/.agents/skills` que no se eligen se apagan una por una en el `config.toml` del home, regenerado en cada lanzamiento (una skill nueva agregada después del lanzamiento carga hasta el próximo).
3. En Codex, las skills van copiadas, no enlazadas.
4. El panel muestra "siempre carga" para: fuentes del repo, `cc-plugin-*`, `/etc/codex`.
5. Los controles obligatorios del modo no se pueden desmarcar desde un preset. `r3-gate` no es uno: está apagado por decisión del dueño.

## Cómo reproducir

```sh
H=$(mktemp -d -p ~/.local/state); ln -s ~/.claude/.credentials.json $H/.credentials.json
echo '{"enabledPlugins":{"design@synced":false,"engineering@synced":false}}' > $H/settings.json
CLAUDE_CONFIG_DIR=$H CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 claude -p 'Respond only with OK.' \
  --strict-mcp-config --output-format stream-json --verbose < /dev/null
# init: tools/skills/mcp_servers/plugins; result.usage: input + cache_creation + cache_read
```

Codex: `CODEX_HOME=<home> codex exec --skip-git-repo-check --json '<prompt>'`. Se lee `turn.completed.usage.input_tokens` y se le pide al modelo que liste sus skills y MCP.
