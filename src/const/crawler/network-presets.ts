/** Lists the public resolver shortcuts accepted by the crawler backend. */
export const DNS_PRESETS = [
  { value: "1.1.1.1", label: "Cloudflare DNS" },
  { value: "8.8.8.8", label: "Google Public DNS" },
  { value: "9.9.9.9", label: "Quad9" },
  { value: "94.140.14.14", label: "AdGuard DNS" },
] as const;
