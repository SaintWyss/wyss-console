# Project Insight — plan de implementación (listo para Haiku)

Objetivo: dentro de la ventana de Proyectos, ver de un proyecto:

1. **Datos generales**: archivos, carpetas, tamaño, git, licencia, manifests, scripts.
2. **Tecnologías**: lenguajes (por extensión) y frameworks/librerías/testing/DB/infra/CI (por manifests).
3. **Arquitectura**: patrones inferidos por la estructura de carpetas (Clean/Hexagonal, Layered, MVC, Feature-based, Monorepo, Microservicios, Desktop Tauri, Full-stack…), con % de confianza y evidencia.
4. **UML**: diagrama de clases generado de los tipos declarados en el código (class/interface/struct/enum/trait + extends/implements + campos/métodos), con exportación a Mermaid y PlantUML.
5. **README con vista previa en vivo**: editor a la izquierda, render Markdown a la derecha, actualizado en cada tecla; botón Guardar.

## ¿Qué servicios / MCP tienen que correr?

**Ninguno MCP.** Todo es local y sin red:

| Pieza | Dónde corre | Ya existe |
|---|---|---|
| Escaneo de carpeta, detección de tecnologías/arquitectura, extracción de tipos, leer/guardar README | Backend Rust de Tauri (`src-tauri/src/projects/insight.rs`) | ✅ escrito (falta compilar en una máquina con GTK) |
| Comandos IPC `projects_insight`, `projects_readme_save` | `src-tauri/src/projects/commands.rs` | ✅ registrados + `build.rs` + `capabilities/projects.json` |
| Render Markdown, diagrama UML SVG, exportes Mermaid/PlantUML | Frontend Svelte (sin dependencias nuevas) | ❌ por hacer (pasos 2–6) |

Requisitos para correr: lo mismo que la app hoy (`npm run desktop:dev`). El proyecto tiene que tener carpeta enlazada y **disponible** (`root.availability === 'available'`); si no, el backend responde `root_missing` / `root_identity_changed` / `root_ineligible`.

> Opcional a futuro (NO hacer ahora): análisis con IA (resumen de arquitectura por Claude) reutilizando el runtime de agentes existente (`f8`). No requiere MCP.

## Seguridad (respetar las reglas del repo)

- El frontend **nunca** recibe ni manda rutas: solo `projectId`. La ruta la resuelve `ProjectsService::project_root` (verifica device/inode del binding).
- El walk no sigue symlinks, salta `node_modules`, `target`, `.git`, etc., y está acotado (20k archivos, profundidad 12, 256 KB por fuente, 400 tipos).
- `save_readme` solo escribe el README (o crea `README.md`), máx 512 KB, escritura atómica (tmp + rename), rechaza si el README es un symlink.
- El Markdown se renderiza **escapando todo el HTML** (no `{@html}` de texto crudo sin escapar). Links solo `http(s):`, `mailto:` o `#`.

---

## Paso 0 — Verificar backend (ya escrito)

Archivos tocados: `src-tauri/src/projects/{insight.rs,mod.rs,service.rs,commands.rs}`, `src-tauri/build.rs`, `src-tauri/capabilities/projects.json`.

```bash
npm run rust:fmt
```
```bash
node scripts/tool.mjs cargo test --locked --manifest-path src-tauri/Cargo.toml --lib insight
```
```bash
npm run rust:clippy
```

En el contenedor cloud no compila (falta `gdk-3.0`); hay que correrlo local. Arreglar warnings de clippy si aparecen (probablemente `collapsible_if`, `needless_range_loop` en `architecture()`). Si el build valida que cada permiso de `capabilities` exista, el `build.rs` ya genera `allow-projects-insight` y `allow-projects-readme-save`.

Forma JSON que devuelve `projects_insight` (camelCase):

```ts
{
  name: string; files: number; folders: number; bytes: number; truncated: boolean;
  languages: { name: string; files: number; bytes: number }[];
  technologies: { name: string; category: 'language'|'framework'|'library'|'tooling'|'testing'|'database'|'infrastructure'|'ci'; version: string|null; source: string }[];
  architecture: { name: string; description: string; confidence: number; evidence: string[] }[];
  structure: { name: string; dir: boolean; files: number }[];
  manifests: string[];
  scripts: { source: string; name: string; command: string }[];
  git: boolean; license: string|null;
  readme: { name: string; content: string; truncated: boolean } | null;
  types: { name: string; kind: 'class'|'interface'|'struct'|'enum'|'trait'; file: string; language: string; extends: string[]; implements: string[]; fields: string[]; methods: string[] }[];
}
```
`projects_readme_save({ request: { id, content } })` → `string` (nombre del archivo).

## Paso 1 — Contrato TS: `src/lib/contracts/insight.ts`

- Exportar las interfaces de arriba (`Insight`, `Technology`, `Pattern`, `TypeDecl`, …).
- `parseInsight(value: unknown): Insight` validando tipos con el mismo estilo que `src/lib/contracts/projects.ts` (helpers `rec`, `str`, `list`, … — copiarlos o exportarlos desde allí). Si no encaja → throw.
- Test: `tests/insight-contract.test.ts` con un objeto válido y uno inválido.

## Paso 2 — Cliente: `src/lib/desktop/projects-client.ts`

Agregar a la interfaz `ProjectsClient` y a `desktopProjects`:

```ts
insight(projectId: string): Promise<Insight>;
saveReadme(projectId: string, content: string): Promise<string>;
```
```ts
async insight(id) {
  return checked(await call('projects_insight', { request: { id } }), parseInsight);
},
async saveReadme(id, content) {
  return checked(await call('projects_readme_save', { request: { id, content } }), (v) => {
    if (typeof v !== 'string') throw new Error('name');
    return v;
  });
},
```
Buscar con `grep -rn "ProjectsClient" tests src` los fakes de test y agregarles estos dos métodos (pueden `throw`), para que `npm run typecheck` pase.

## Paso 3 — Markdown seguro: `src/lib/features/insight/markdown.ts`

Función pura `renderMarkdown(src: string): string` (sin dependencias):
1. `escape()` de `& < > " '` sobre TODO el texto primero.
2. Bloques: ```` ``` ```` fences (con lenguaje como clase), `#`…`######`, `>` citas, listas `-`/`*`/`1.`, tareas `- [ ]`/`- [x]`, `---`, tablas GFM (`|a|b|` + `|---|`), párrafos.
3. Inline (sobre texto ya escapado): `` `code` ``, `**bold**`, `*em*`/`_em_`, `~~del~~`, `[txt](url)`, `![alt](url)`, autolinks `https://…`.
4. `safeUrl(url)`: permitir solo `https:`, `http:`, `mailto:`, `#…`, y rutas relativas (estas se muestran como texto/link deshabilitado: no hay acceso a archivos locales desde el webview). Todo lo demás → `#`.
5. Imágenes: solo `https:`; si no, mostrar `[imagen: alt]`.

Tests `tests/markdown.test.ts`: encabezados, listas, tabla, code fence, y **XSS**: `<script>`, `<img onerror>`, `[x](javascript:alert(1))` deben salir escapados/neutralizados.

## Paso 4 — UML: `src/lib/features/insight/uml.ts`

Funciones puras:
- `relations(types)`: `{from, to, kind: 'extends'|'implements'}[]` solo cuando `to` existe en `types`.
- `toMermaid(types)`: `classDiagram` con `class X { <<interface>> +field +method() }`, `Base <|-- X`, `Iface <|.. X`.
- `toPlantUml(types)`: `@startuml … @enduml` (`class`, `interface`, `enum`, `X --|> Base`, `X ..|> Iface`).
- `layout(types, rels)`: layout por niveles (los padres arriba): nivel = profundidad en la cadena extends/implements; ancho de caja = max(nombre, miembros)*7px+16; alto = 24 + 16*(miembros); posicionar en grilla por nivel. Devuelve `{ boxes: {name,x,y,w,h}[], width, height }`.
- Filtrado: por carpeta (prefijo de `file`) y por texto; por defecto mostrar solo tipos con alguna relación si hay más de 60.

Tests `tests/uml.test.ts` para `toMermaid`/`toPlantUml`/`relations`.

## Paso 5 — Componentes: `src/lib/features/insight/`

- `InsightPanel.svelte` — props `{ client: ProjectsClient; projectId: string; onClose: () => void }`. Carga `client.insight(projectId)` al montar; estados *cargando / error (mostrar `failure.code` con el mismo texto que usa ProjectsView) / listo*. Tabs: **Resumen · Tecnologías · Arquitectura · UML · README**. Botón "Volver a analizar".
  - *Resumen*: tarjetas (archivos, carpetas, tamaño legible, git, licencia), `structure` como lista, `scripts` en tabla, aviso si `truncated`.
  - *Tecnologías*: barra apilada de lenguajes por `bytes` (excluir Markdown/JSON/YAML de la barra), chips de tecnologías agrupados por `category` con `version` y `source` en tooltip.
  - *Arquitectura*: tarjeta por patrón con barra de confianza, descripción y evidencia; si vacío: "No se detectó un patrón claro".
- `UmlDiagram.svelte` — SVG con `layout()`; cajas con estereotipo (`«interface»`, `«enum»`…), separador campos/métodos, flechas: `extends` línea sólida + triángulo hueco, `implements` línea punteada + triángulo hueco (`<marker>`). Zoom con rueda y pan arrastrando (`viewBox`). Click en caja → muestra `file`. Botones: **Copiar Mermaid**, **Copiar PlantUML**, **Descargar SVG** (Blob + `<a download>`).
- `ReadmeEditor.svelte` — `textarea` (`bind:value`) + `<div class="md">{@html renderMarkdown(text)}</div>` en dos columnas (en pantallas angostas: toggle Editar/Vista). Modos: *Vista*, *Editar*, *Dividido* (default al editar). Indicador "sin guardar"; Guardar → `client.saveReadme`; Ctrl/Cmd+S guarda; si no hay README, botón "Crear README.md". Si `readme.truncated`, deshabilitar guardar (evita perder contenido). Scroll sincronizado opcional (por proporción).

Estilos: usar las variables/clases existentes de `src/app.css` (`.action`, colores) para que encaje con el resto de la UI.

## Paso 6 — Integración en la UI de proyectos

- En `src/lib/features/projects/ProjectRail.svelte` (lista de proyectos), por proyecto con `root?.availability === 'available'`: botón **"Detalles"** (ícono ℹ) que emite `oninsight(project.id)`.
- En `ProjectsView.svelte`: estado `let insightFor = $state<string | null>(null)`; cuando no es null, renderizar `<InsightPanel client={client} projectId={insightFor} onClose={() => (insightFor = null)} />` en lugar del `WorkspaceCanvas` (o como overlay/drawer a la derecha). Pasar `client` desde donde ya se tiene.
- Agregar al `WorkspaceCanvas` toolbar un botón "Detalles del proyecto" cuando `workspace.projectId` no es null.

## Paso 7 — Verificación final

```bash
npm run format
```
```bash
npm run check
```
```bash
npm run check:rust
```
Probar manualmente con `npm run desktop:dev`: abrir un proyecto (p. ej. este repo), ver que detecte Svelte, Vite, Vitest, Tauri, Rust, "Desktop app" y "Feature-based"; abrir UML y README, editar y ver la vista previa en vivo; guardar y confirmar el cambio con `git diff README.md`.

## Criterios de aceptación

- [ ] Ningún comando nuevo acepta ni devuelve rutas absolutas.
- [ ] Markdown no ejecuta HTML/JS del README (tests XSS pasan).
- [ ] La vista previa del README se actualiza en cada tecla sin lag en READMEs de ~100 KB (si hay lag, debounce 100 ms).
- [ ] UML exporta Mermaid y PlantUML válidos.
- [ ] `npm run check` y `npm run check:rust` en verde.
