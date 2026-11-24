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

## Prototype Builder

Open **Prototype Builder** from the dashboard and choose **New prototype**. Drag a component from the palette or use **Add**. Move components freely, change their stacking in **Layers**, and edit geometry, copy, and appearance in **Properties**. Use the Hand tool or Space-drag to pan. Side handles resize width, and bottom corners resize width and minimum height together. **Preview** fits the authored screen and allows sample input in the built-in Sign-in card.

Choose **Open bundled sample** to start offline, or paste a public GitHub repository URL and choose **Import GitHub starter**. A compatible repo contains `examples/prototype-builder/wits-starter.json` on its default branch. The [public starter](https://github.com/Vincenthe3231/wits-prototype-starter) is ready to import. WITS reads the version 1 dashboard data without requiring a clone, dependency installation, or project build.

Choose **Save variant** to save locally, or **Save as new** to keep a separate copy. Saved variants reopen offline. Older saves open in the new editor and upgrade to v4 on an explicit save. The saved project theme is independent of the WITS appearance toggle.

### Bring your own component

Use **Import component file** to review a JSON definition, then **Add to palette** to install it. Start with the [pricing card example](examples/prototype-builder/pricing-card.wits-component.json) and [v1 JSON Schema](examples/prototype-builder/wits-component-v1.schema.json). Definitions use stacks, wrapping rows, text, and visual buttons. A text or button label can bind to a field declared in `fields`. Styles use the schema's typed values and project color tokens.

WITS validates field references, rejects duplicate JSON keys, and reports a code, JSON Pointer, and source location for incompatible files. Imports are limited to 64 KiB, 64 elements, six tree levels, and 16 fields. Names and field labels use at most 100 UTF-8 bytes, and authored text uses at most 2000 bytes. Increase the revision to publish a changed definition. A saved variant embeds the exact revisions used by its layers, so removing a palette entry leaves those variants usable.

## Markdown images

In Visual mode, use **Insert image** to choose a local PNG, JPEG, GIF, or WebP, or paste an image from the clipboard. The limit is 16 MiB per image. Select an image and press **Ctrl+C** to copy it. Use the block handle to drag an image block, or use **Block actions** to move it with the keyboard.

On save, imported images go into a `wits-images/` folder beside the Markdown file. Save As copies these attachments with the document. Keep the folder alongside the Markdown when moving or sharing it.

In Source mode, paste a clipboard image with **Ctrl+V** to insert its Markdown reference at the cursor. You can also write a relative reference such as `![Screenshot](./screenshots/login.png)` for an image already inside the document folder. Switch to Visual to preview it. When moving or saving a copy of the Markdown to another folder, copy any manually linked image folders yourself.

## Crawler and Media Downloads

Open **Crawler** in the dashboard. **Crawl** collects pages within the seed URL paths by default. Expand **Advanced settings** to allow additional domains, enter exclusions, or set a proxy. **Auto** uses the Rust HTTP crawler and launches Obscura for sparse or failed pages. **Browser** uses Obscura for page traversal and **Static** runs without a browser. Use **Choose folder** under **Download path** to save assets in a per-job subfolder at your chosen destination. Leaving it blank saves assets in WITS Tools app data. Deleting a job keeps files saved in a custom folder. Job history is stored in app data, and **Export JSON** saves the selected job and its page and asset records.

In **Media Downloads**, enter a page URL and choose **Capture**. Play the media in the private WebView2 window, signing in there if the page requires it. Select **Download** beside a detected stream in WITS Tools. Keep the window open until the download starts. Captured streams and their request context clear when the window closes. New streams and new downloads appear at the top of their respective lists. Captured streams show the first capture time and playback duration when the manifest provides it. With no filename override, captured downloads use the page title or the last component of the page URL.

Use **Direct media URL** for a known file, HLS playlist, or DASH manifest. The default destination is Windows Downloads. Segmented HLS output uses `.ts` and DASH saves the selected video representation as `.mp4`. The Downloads list shows file size, playback duration when known, and an ETA while the transfer has enough progress data.

Set **Download workers** under **Download settings** to control parallel segments or byte ranges per file. The default is 4 and the range is 1 to 16. Direct files use parallel ranges only when the size is known, the file is larger than 8 MiB, and the server supports byte ranges. Other direct files use a single stream. More workers can help when individual requests are the bottleneck. They may have little effect when the server caps overall throughput or the connection is already saturated.

### Proxy or DNS

Both Crawl and Media Downloads accept a custom HTTP, HTTPS, SOCKS5, or SOCKS5H proxy URL, or a DNS-over-HTTPS resolver. Leave the field blank for default networking. Enter a known resolver address such as Cloudflare `1.1.1.1`, Google `8.8.8.8`, Quad9 `9.9.9.9`, or AdGuard `94.140.14.14` to select that provider's encrypted endpoint. Paste the full HTTPS endpoint for NextDNS, including your profile ID. For other providers, use their `https://…/dns-query` endpoint or prefix a custom endpoint with `doh://` in place of `https://`.

DNS selection is scoped to crawler requests and does not change Windows settings. The capture browser supports DNS-over-HTTPS and unauthenticated HTTP or SOCKS5 proxies. Close and reopen the capture window after changing the route.

### Capture browser diagnostics

In development, the launching terminal shows `[crawler::capture-browser …]` records. Match navigation start and completion by navigation ID. Failed navigation records include WebView2's error code and the HTTP status when available. The trace reports media response status and whether replay headers were present, without printing their values or full URLs.

### Windows x64 browser and CLI

The desktop build hooks stage Obscura v0.2.7 and its required DLLs with a verified SHA-256. Allow the first build to retrieve the pinned archive using Windows PowerShell. Run `bun run prepare:crawler-browser` to stage it explicitly. The generated files live in `src-tauri/resources/crawler-browser/`. Keep the `crawler-browser/` folder beside the application and console binaries when copying a portable build.

The installers include `wits-tools-cli.exe`. Use `--help` after any command to view its options.

```powershell
.\wits-tools-cli.exe crawl https://example.com --mode static
.\wits-tools-cli.exe export <job-id> --output crawl.json
.\wits-tools-cli.exe info https://example.com/playlist.m3u8
.\wits-tools-cli.exe download https://example.com/playlist.m3u8 --output .\downloads
.\wits-tools-cli.exe proxy run
```

The opt-in CLI proxy listens on loopback. It can observe ordinary HTTP media and tunnels HTTPS without decrypting it.

## Errors and notifications

Rust tools own their native error codes. Markdown Editor defines those codes in `src-tauri/src/markdown_editor/error.rs` and validates them at the IPC boundary in `src/api/markdown-editor.ts`. Put user-facing messages in `src/const/markdown-editor/errors.ts`. Keep native diagnostics out of user-facing copy.

Report a failure from the action that owns it. Use `src/composables/use-notifications.ts` to publish it through the shared `AppToaster.vue`. Treat picker cancellation as a normal outcome. Keep recovery controls available in the owning view, including **Save As** for an external file conflict. Use the status tokens and notification rules in [DESIGN.md](DESIGN.md).

- If image paste fails in Source or Visual mode, show one error toast and leave the Markdown unchanged.
- If draft recovery writes keep failing, show one warning toast and keep the recovery status visible. A successful write clears the status and allows a later outage to trigger a new toast.
