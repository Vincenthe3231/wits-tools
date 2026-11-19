# WITS Tools

WITS Tools is a desktop app built with Tauri, Rust, Vue, TypeScript, and Vite. Bun runs the frontend scripts.

## Frontend structure

```text
src/
├── api/
├── assets/
├── components/
├── composables/
├── const/
├── pages/
├── routes/
├── styles/
├── utils/
├── App.vue
└── main.ts
```

| Path | Responsibility |
| --- | --- |
| `src/api/` | Frontend communication with Rust, including typed Tauri command wrappers and event handling. |
| `src/assets/` | Images and other static files imported by frontend code. |
| `src/components/` | Vue components. Group components specific to one tool in a named subfolder. |
| `src/composables/` | Vue composables that use reactivity or lifecycle hooks. Group tool-specific composables by tool. |
| `src/const/` | Constants grouped by module, with one file per module where useful. |
| `src/pages/` | Route-level Vue pages grouped by tool. |
| `src/routes/` | Vue Router setup and route registration. Use dynamic imports for tool pages. |
| `src/styles/` | Global and reusable stylesheets. When Tailwind is added, import its entry stylesheet from `main.ts`. |
| `src/utils/` | Helpers such as formatting or validation functions. Group tool-specific helpers by tool. |
| `src/App.vue` | Root Vue component and app shell. |
| `src/main.ts` | Vue entry point for mounting the app and registering app-wide dependencies. |

Use consistent tool names across folders. For example, a JSON tool could have a page in `pages/json-tools/`, route registration in `routes/json-tools.ts`, and Tauri invocation wrappers in `api/json-tools.ts`.

## Tauri structure

| Path | Responsibility |
| --- | --- |
| `src-tauri/src/main.rs` | Desktop entry point that calls the Tauri library's `run()` function. |
| `src-tauri/src/lib.rs` | Tauri setup, plugin registration, and the command handler list. |
| `src-tauri/src/` | Rust modules grouped by tool as features are added. Keep Tauri commands thin and put tool logic in ordinary Rust functions. |
| `src-tauri/Cargo.toml` | Rust package, dependencies, and build profiles. |
| `src-tauri/tauri.conf.json` | App, frontend build hook, and bundle configuration. |
| `src-tauri/capabilities/` | Tauri capability definitions and permissions. |

### Rust module conventions

Start each tool in one Rust module within the existing Cargo package. As the tool grows, split it by operation or workflow. Keep Tauri commands thin and have them call ordinary Rust functions that can be reused and tested without launching the desktop app.

For example, a JSON tool with several operations could grow into this layout:

```text
src-tauri/src/
├── lib.rs
├── main.rs
├── json_tools.rs
└── json_tools/
    ├── commands.rs
    ├── dto.rs
    ├── error.rs
    ├── formatter.rs
    └── validator.rs
```

Use `json_tools.rs` to declare child modules. Register the tool's commands in the existing Tauri command handler in `lib.rs`. Add the files shown above as their responsibilities emerge.

| Definition | Where it belongs |
| --- | --- |
| Constants | Keep them beside the operation that uses them. Add a tool-local `constants.rs` when several operations share a meaningful group of constants. |
| Request and response structs | Keep them in `commands.rs` while they serve a small set of commands. Move them to a tool-local `dto.rs` when the frontend contract grows. Use Serde to define the data passed between Vue and Rust. |
| Tool data and behavior | Define structs and enums alongside the logic that owns them. Give a type its own file when it has enough methods or rules to warrant one. |
| Traits | Define a trait near the code that depends on it when callers need interchangeable implementations. Reuse standard traits such as `std::io::Read` where they fit. |
| Errors | Keep operation-specific errors with their operation. Move errors shared across the tool into `error.rs` and translate them into frontend responses at the command boundary. |

Split large modules around named responsibilities. For example, an import workflow can keep its coordinating function in `import.rs` and move input reading and parsing into `import/read.rs` and `import/parse.rs`. Keep items private by default, use `pub(super)` within the parent module's scope, and use `pub(crate)` when other tools in the crate need access. Put genuinely shared code in a module named for what it does, such as `settings.rs`.

## Development and release

Run these commands from the project root.

| Command | Purpose |
| --- | --- |
| `bun install` | Install frontend dependencies. |
| `bun run dev` | Start the Vite frontend development server. |
| `bun run tauri dev` | Run the desktop app with the Vite development server. |
| `bun run build` | Type-check the frontend and build it into `dist/`. |
| `bun run tauri build` | Run the frontend build and package the desktop release. |

The release currently builds the frontend as one Vite application. Lazy routes let the webview load tool pages on demand. Independent tool builds would require separate build targets and a step that assembles their outputs for Tauri.
