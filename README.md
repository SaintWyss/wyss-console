# Wyss Console

Personal native development workspace for CachyOS/KDE: persistent terminals,
provider configuration, Arsenal, Native agents and supervised Managed workflows.
Private repository; Tauri 2, Svelte 5, TypeScript and Rust.

| Question                                         | Read                                 |
| ------------------------------------------------ | ------------------------------------ |
| What is the project and its scope?               | [Product](docs/PRODUCT.md)           |
| What exists and what is still unverified?        | [Current state](docs/STATUS.md)      |
| What remains, and in what order?                 | [Remaining roadmap](docs/ROADMAP.md) |
| Which decisions and restrictions apply?          | [Decisions](docs/DECISIONS.md)       |
| How are components and state divided?            | [Architecture](docs/ARCHITECTURE.md) |
| How do I develop, check, install and recover it? | [Development](docs/DEVELOPMENT.md)   |

Detailed target behavior and acceptance criteria live in [Requirements](docs/REQUIREMENTS.md).
Specialized contracts are linked from the architecture and decisions pages.
For source review and implementation by models, follow [Engineering execution](docs/EXECUTION.md).

Start a development window with `npm ci` followed by `npm run desktop:dev` after
installing the [prerequisites](docs/DEVELOPMENT.md#prerequisites-and-commands).
For installation and session-preserving updates, use the
[package procedure](docs/DEVELOPMENT.md#ordinary-package-installation-and-launch).

Source availability, recorded tests and acceptance on an installed system are
separate. The [status page](docs/STATUS.md) states the current limits.
