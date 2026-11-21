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
| `src/routes/` | Home for Vue Router setup and lazy route registrations when the app has multiple tools. |
| `src/styles/` | Global and reusable stylesheets. `main.ts` imports `styles/app.css`, which loads Tailwind and defines the global design tokens. |
| `src/utils/` | Helpers such as formatting or validation functions. Group tool-specific helpers by tool. |
| `src/App.vue` | Root Vue component and app shell. |
| `src/main.ts` | Vue entry point for mounting the app and registering app-wide dependencies. |

Use the same tool name across folders. The Markdown editor uses `markdown-editor` for its Vue page, components, composables, constants, helpers, and API wrapper. `App.vue` renders the dashboard through Vue Router and retains the editor session across navigation. See [DESIGN.md](DESIGN.md) for token values and UI styling rules.

The dashboard page lives in `src/pages/dashboard/`. Dashboard components, catalog constants, search helpers, and styles live in `components/dashboard/`, `const/dashboard/`, `utils/dashboard/`, and `styles/dashboard/` under `src/`.

## Code formatting and conventions

### Required comments for generated code

- Start every generated source file with a short comment block explaining what the file does and what it owns.
- Add a meaningful comment above every logical block of generated code, including blocks added to existing files. Explain what the block does and why it matters. Call out constraints, state transitions, ordering, and error handling where they affect the logic.
- Write comments in plain language using the file's native comment syntax. Describe the block's purpose and behavior in terms a reader can understand without tracing every line.

### Vue, TypeScript, and CSS

- Use two-space indentation, double quotes in TypeScript, and semicolons. Group external imports and project imports with a blank line between them. Separate types, related state, derived state, and functions with blank lines.
- Order Vue single-file component blocks as `<script setup lang="ts">`, `<template>`, and `<style scoped>` when local CSS is needed. Use PascalCase for Vue component filenames and kebab-case for TypeScript module filenames. Use camelCase for functions and variables, and uppercase snake case for named scalar constants such as `RECOVERY_DELAY`.
- Put each attribute on its own line when an opening tag wraps. Indent children and leave blank lines between distinct template sections. Keep reusable behavior in composables and stateless helpers in `utils/`. Put typed Tauri calls in `api/`.
- Keep Tailwind utilities close to the markup. Use scoped CSS for component-specific selectors and animation. Write one CSS declaration per line and leave a blank line between rules. Use semantic tokens for shared roles.

### Rust

- Use `rustfmt` with four-space indentation. Group standard library, external crate, and local imports with blank lines between groups. Keep attributes directly above their item and leave blank lines between distinct declarations and functions.
- Use snake_case for modules and functions, PascalCase for types, and uppercase snake case for constants. Keep command handlers thin, use typed errors in Rust, and map results to the frontend contract at the command boundary. Follow the module conventions below as a tool grows.

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

The dashboard filters a frontend tool catalog and uses browser storage for appearance. Add a module under `src-tauri/src/dashboard/` when a dashboard operation needs native persistence or system access.

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

Run these commands from the project root. To launch the desktop app for development, run `bun install` once, then:

```powershell
bun run tauri dev
```

| Command | Purpose |
| --- | --- |
| `bun install` | Install frontend dependencies. |
| `bun run dev` | Start the Vite frontend development server. |
| `bun run tauri dev` | Run the desktop app with the Vite development server. |
| `bun run build` | Type-check the frontend and build it into `dist/`. |
| `bun run tauri build` | Run the frontend build and package the desktop release. |
| `bun run tauri build --no-bundle` | Build the release executable without installers. |
| `bun test` | Run the frontend tests. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Check Rust formatting. |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml` | Run the Rust tests. |

On Windows, the release executable is `src-tauri/target/release/wits-tools.exe`. The bundled release also writes the NSIS setup executable and MSI installer under `src-tauri/target/release/bundle/`. The release builds the frontend as one Vite application.

## Markdown images

In Visual mode, use **Insert image** to choose a local PNG, JPEG, GIF, or WebP, or paste an image from the clipboard. The limit is 16 MiB per image. Select an image and press **Ctrl+C** to copy it. Use the block handle to drag an image block, or use **Block actions** to move it with the keyboard.

On save, imported images go into a `wits-images/` folder beside the Markdown file. Save As copies these attachments with the document. Keep the folder alongside the Markdown when moving or sharing it.

In Source mode, paste a clipboard image with **Ctrl+V** to insert its Markdown reference at the cursor. You can also write a relative reference such as `![Screenshot](./screenshots/login.png)` for an image already inside the document folder. Switch to Visual to preview it. When moving or saving a copy of the Markdown to another folder, copy any manually linked image folders yourself.
