/** Defines and resolves authored colors and component styles without changing the app theme. */
import type { CSSProperties } from "vue";

export const roles = ["screen", "surface", "text", "muted", "border", "accent", "onAccent"] as const;
export type ColorRole = typeof roles[number];
export type FontChoice = "system" | "serif" | "mono";
export type Theme = Record<ColorRole, string> & { font: FontChoice };
export type Style = {
  fill?: string; color?: string; border?: string; radius?: number;
  padding?: number; gap?: number; fontSize?: number; weight?: number;
};

// Capture legacy theme colors once, before the document receives its own scoped tokens.
const light: Theme = { screen: "#fefaf3", surface: "#e6f2dd", text: "#24312b", muted: "#51645b",
  border: "#51645b", accent: "#24483d", onAccent: "#ffffff", font: "system" };
const tokens = ["--surface-editor", "--surface-toolbar", "--text-primary", "--text-muted",
  "--text-muted", "--accent-strong", "--text-on-accent"] as const;
const hex = /^#[0-9a-f]{6}$/i;
export const validTheme = (theme: Theme) => roles.every((role) => hex.test(theme[role]))
  && ["system", "serif", "mono"].includes(theme.font);

export function currentTheme(): Theme {
  const theme = { ...light };
  if (typeof document === "undefined") return theme;
  const styles = getComputedStyle(document.documentElement);
  roles.forEach((role, index) => {
    const value = styles.getPropertyValue(tokens[index]!).trim();
    if (hex.test(value)) theme[role] = value;
  });
  return theme;
}

// Map local token roles for the existing built-ins and for imported primitives.
export function themeCss(theme: Theme): CSSProperties {
  // Native input rendering follows the authored screen instead of the host appearance.
  const channels = theme.screen.slice(1).match(/.{2}/g)?.map((channel) => Number.parseInt(channel, 16)) ?? [];
  const brightness = 0.2126 * (channels[0] ?? 255) + 0.7152 * (channels[1] ?? 255) + 0.0722 * (channels[2] ?? 255);
  return {
    "--surface-editor": theme.screen, "--surface-toolbar": theme.surface, "--text-primary": theme.text,
    "--text-muted": theme.muted, "--prototype-border": theme.border, "--accent-strong": theme.accent,
    "--text-on-accent": theme.onAccent, color: theme.text, backgroundColor: theme.screen,
    colorScheme: brightness < 128 ? "dark" : "light",
    "--tool-card-raised": "0 6px 18px #0002",
    fontFamily: { system: '"Segoe UI Variable", "Segoe UI", system-ui, sans-serif', serif: "Georgia, serif",
      mono: "Consolas, monospace" }[theme.font],
  };
}

export const styleFields = {
  fill: "Fill", color: "Text color", border: "Border color", radius: "Corner radius",
  padding: "Padding", gap: "Gap", fontSize: "Font size", weight: "Font weight",
} as const;

// Keep renderer-independent values bounded before mapping them to known CSS properties.
export function validStyle(style: Style) {
  return Object.entries(style).every(([key, value]) => {
    if (key === "fill" || key === "color" || key === "border") {
      return typeof value === "string" && (hex.test(value) || roles.includes(value as ColorRole));
    }
    return Object.prototype.hasOwnProperty.call(styleFields, key) && Number.isInteger(value) && Number(value) >=
      (key === "fontSize" ? 1 : key === "weight" ? 100 : 0) && Number(value) <= (key === "weight" ? 900 : 512);
  });
}

export function styleCss(style: Style | undefined, theme: Theme): CSSProperties {
  if (!style) return {};
  const color = (value?: string) => value && (roles.includes(value as ColorRole) ? theme[value as ColorRole] : value);
  const px = (value?: number) => value === undefined ? undefined : value + "px";
  return {
    backgroundColor: color(style.fill), color: color(style.color),
    border: style.border ? "1px solid " + color(style.border) : undefined,
    borderRadius: px(style.radius), padding: px(style.padding), gap: px(style.gap),
    fontSize: px(style.fontSize), fontWeight: style.weight,
  };
}
