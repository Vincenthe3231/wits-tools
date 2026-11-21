# WITS Tools design tokens

[app.css](src/styles/app.css) loads the global palette and the shared [status tokens](src/styles/status.css). Vue loads it through `src/main.ts`. The dashboard and both editor modes use the same semantic roles. Appearance follows the system until the toolbar theme icon is used. The icon switches directly between Light and Dark, and the app remembers the selection.

## Color tokens

| CSS variable | Light | Dark | Use |
| --- | --- | --- | --- |
| `--surface-editor` | `#fefaf3` | `#17211d` | Base writing surface and dialogs. |
| `--surface-toolbar` | `#e6f2dd` | `#202e28` | Toolbar, menus, and secondary surfaces such as code blocks. |
| `--surface-selected` | `#b1d3b9` | `#344a3f` | Hover, selected controls, and text selection. |
| `--text-primary` | `#24312b` | `#eaf2eb` | Document text, control labels, and text on selected surfaces. |
| `--text-muted` | `#51645b` | `#b5c6bb` | Secondary text on editor and toolbar surfaces. |
| `--accent-strong` | `#24483d` | `#a9d3ba` | Links, focus rings, the caret, selected indicators, and primary button fills. |
| `--text-on-accent` | `#ffffff` | `#17211d` | Text on an `--accent-strong` fill. |

## Status colors

Use these tokens for toasts and inline notices. Each status defines a surface, text, and indicator. The indicator colors icons and borders. All tokens follow the same system or toolbar appearance setting as the workspace.

| Status | Role | Light | Dark |
| --- | --- | --- | --- |
| Error | Surface | `#fbeae7` | `#3a2526` |
| Error | Text | `#72262c` | `#ffdedb` |
| Error | Indicator | `#a03333` | `#f29b98` |
| Warning | Surface | `#faf0d7` | `#372e1b` |
| Warning | Text | `#583c0a` | `#fae6b1` |
| Warning | Indicator | `#795300` | `#e7bf65` |
| Info | Surface | `#e8f1fa` | `#203043` |
| Info | Text | `#21486e` | `#deedff` |
| Info | Indicator | `#285d88` | `#8bbde9` |
| Success | Surface | `#e5f2e8` | `#20372d` |
| Success | Text | `#214b37` | `#d9f5e3` |
| Success | Indicator | `#29634a` | `#8ad4ab` |

CSS variables use the `--status-<kind>-<role>` pattern. For an error notice, apply `bg-status-error-surface`, `text-status-error-text`, and `border-status-error-indicator` with a border-width utility. Use `text-status-error-indicator` on an icon. List complete utility names in Vue variant maps so Tailwind can detect them. In scoped CSS, use the same variables directly.

- **Error** reports a failed action and explains what happened to the user's work. Keep required recovery steps available in the owning view or dialog.
- **Warning** describes a condition that needs attention, such as limited functionality or a risky next step.
- **Info** communicates a change or update the user may need to know about.
- **Success** confirms that a user-initiated action completed. Routine background work can update the existing status text.

Use a clear title and a recognizable icon as well as color. Keep controls keyboard-accessible, pair decorative icons with visible text, and respect reduced motion. Announce an event once, even when its toast and inline recovery control appear together.

### Toast behavior

- Stack toasts at the top right, below the app toolbar, with up to three visible at a time.
- Set the default duration to 8 seconds for errors and warnings and 5 seconds for info and success.
- Place a plain **×** inside the top-right corner of each toast. Give it a 24 px hit area, a visible keyboard focus outline, and the accessible name “Dismiss notification.” Reserve space beside the control so text can wrap.

## UI packages

Use Vue Sonner for the shared toast host and Reka UI for headless controls such as menus, popovers, and dialogs when adding or updating those controls. Use WITS Tools tokens to style both.

All UI work must use only components and UI packages already in the repository. Reuse or compose existing components and installed primitives. Add a new component or UI package only when the user explicitly requests it or when a concrete requirement cannot be met with the existing ones. Limit the exception to that requirement and use WITS Tools design tokens.

## Typography and motion

- Shared navigation motion lives in `src/styles/navigation-motion.css`. Pages slide 24 px over 180 ms and in-tool panels slide 14 px over 140 ms. Dashboard and Source enter from the left. Markdown Editor and Visual enter from the right. The sidebar slides and fades over 160 ms on entry and 100 ms on exit. Menus and dialogs enter over 120 ms. Reduced motion disables these effects.

- The UI font stack is `"Segoe UI Variable", "Segoe UI", system-ui, sans-serif`. The shared `--font-code` token is `"Cascadia Mono", Consolas, monospace` and covers Source mode and code in Visual mode.
- A 4 px caret head follows a damped spring. A slower spring stretches the fading trail up to 180 px behind it. Both modes use the settings in [caret-motion.ts](src/const/markdown-editor/caret-motion.ts).
- Use the native caret during composition, reduced motion, and Windows forced-color mode. New motion should explain a change in position or state and use transform or opacity where possible.

## Adding and using tokens

1. Name tokens by purpose and use lowercase kebab-case. Use CSS variables in global or scoped styles, including within Tailwind utilities where appropriate.
2. Assign both light and dark values for a new semantic color. Keep the raw palette values in the theme declarations. Introduce primitive aliases when multiple semantic roles need a named palette value. Add a component alias when it clarifies an independent component state.
3. Use `--text-primary` on `--surface-selected`. In the light theme, `--text-muted` on that fill is only 3.88:1. Use an indicator and an accessible pressed or selected state alongside the fill when a control represents the active choice.
4. Check ordinary text for at least 4.5:1 contrast and essential visual indicators for at least 3:1 against adjacent colors. Check each theme and the relevant state, including keyboard focus. Update this document alongside the stylesheet when a shared token changes. [WCAG text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) · [WCAG non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)
