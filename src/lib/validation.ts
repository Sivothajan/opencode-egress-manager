import { z } from "zod";

export const egressModeSchema = z.enum(["warp", "clean", "tor", "custom"]);
export const warpScopeSchema = z.enum(["tun", "isolated"]);
export const proxyTypeSchema = z.enum(["socks5", "http"]);

export const proxyPoolSchema = z
  .string()
  .min(1, "Enter at least one proxy address")
  .max(4096, "Proxy list is too long")
  .refine((value) => !value.includes("\u0000"), "Proxy list contains invalid characters");

export interface ParsedProxy {
  host: string;
  port: number;
  type: "socks5" | "http";
  hasCredentials: boolean;
}

/** Preview parser for the proxy pool editor (Rust remains authoritative). */
export function parseProxyPool(raw: string): ParsedProxy[] {
  const out: ParsedProxy[] = [];
  for (const token of raw.split(/[\s,;\n\r]+/)) {
    const text = token.trim();
    if (!text) continue;
    let rest = text;
    let type: "socks5" | "http" = "socks5";
    const scheme = rest.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):\/\//);
    if (scheme) {
      const normalized = scheme[1].toLowerCase();
      type =
        normalized === "http" || normalized === "https"
          ? "http"
          : normalized.startsWith("socks")
            ? "socks5"
            : type;
      rest = rest.slice(scheme[0].length);
    }
    const hasCredentials = rest.includes("@");
    if (hasCredentials) rest = rest.slice(rest.lastIndexOf("@") + 1);
    let host = "";
    let port = 0;
    if (rest.startsWith("[")) {
      const end = rest.indexOf("]");
      if (end === -1) continue;
      host = rest.slice(1, end);
      port = Number(rest.slice(end + 1).replace(/^:/, "")) || 0;
    } else {
      const idx = rest.lastIndexOf(":");
      if (idx <= 0) continue;
      host = rest.slice(0, idx);
      port = Number(rest.slice(idx + 1)) || 0;
    }
    if (!host || port <= 0 || port > 65535) continue;
    out.push({ host, port, type, hasCredentials });
  }
  return out;
}

export const cleanEndpointSchema = z
  .string()
  .trim()
  .regex(/^(?:\d{1,3}\.){3}\d{1,3}:\d{1,5}$/, "Use an IPv4 endpoint such as 162.159.192.1:2408")
  .refine((value) => {
    const [ip, port] = value.split(":");
    const octets = ip.split(".").map(Number);
    return octets.every((o) => o >= 0 && o <= 255) && Number(port) > 0 && Number(port) <= 65535;
  }, "Endpoint IP/port is out of range");

export const edgeOverrideSchema = z
  .string()
  .trim()
  .max(64)
  .refine((value) => {
    if (value.toLowerCase() === "auto") return true;
    return z.ipv6().safeParse(value).success;
  }, "Enter 'auto' or a valid IPv6 address");

export const themeSchema = z.enum(["system", "light", "dark"]);

export const settingsPatchSchema = z.object({
  autoResume: z.boolean().optional(),
  strictMode: z.boolean().optional(),
  autoRepair: z.boolean().optional(),
  autostart: z.boolean().optional(),
  theme: themeSchema.optional(),
});

export type ValidationResult<T> = { success: true; data: T } | { success: false; error: string };

export function validate<T>(schema: z.ZodType<T>, value: unknown): ValidationResult<T> {
  const result = schema.safeParse(value);
  if (result.success) return { success: true, data: result.data };
  const first = result.error.issues[0];
  return { success: false, error: first?.message ?? "Invalid value" };
}
