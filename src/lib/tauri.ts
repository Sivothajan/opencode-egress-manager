import { invoke } from "@tauri-apps/api/core";

export interface CommandOptions {
  /** Throw instead of returning the rejected error string. */
  strict?: boolean;
}

function backendAvailable(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as unknown as {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: unknown;
  };
  return Boolean(w.__TAURI_INTERNALS__ || w.__TAURI__);
}

/// True when running inside the native Tauri webview (not a plain browser).
export function isTauriEnvironment(): boolean {
  return backendAvailable();
}

/**
 * After an SPA route change, WebView2 can leave DOM focus on <body>. Return focus
 * to the active navigation link so keyboard navigation continues from a sensible
 * place. No-op in browsers and whenever a control already has focus.
 */
export function focusActiveNavOnRouteChange(): void {
  if (!isTauriEnvironment()) return;
  if (typeof document === "undefined") return;
  if (document.activeElement !== document.body) return;
  const active = document.querySelector<HTMLElement>("nav a.active");
  active?.focus({ preventScroll: true });
}

/**
 * Invoke a typed Tauri command. Tests mock this module, so components never
 * import `@tauri-apps/api/core` directly.
 */
export async function cmd<T>(
  name: string,
  args: Record<string, unknown> = {},
  options: CommandOptions = {},
): Promise<T> {
  if (!backendAvailable()) {
    throw new BackendUnavailableError(name);
  }
  try {
    return (await invoke(name, args)) as T;
  } catch (error) {
    const message =
      typeof error === "string"
        ? error
        : error instanceof Error
          ? error.message
          : JSON.stringify(error);
    if (options.strict) throw new Error(message);
    throw new CommandError(name, message);
  }
}

export class BackendUnavailableError extends Error {
  constructor(command: string) {
    super(`Tauri backend is unavailable (command: ${command})`);
    this.name = "BackendUnavailableError";
  }
}

export class CommandError extends Error {
  readonly command: string;
  constructor(command: string, message: string) {
    super(message);
    this.name = "CommandError";
    this.command = command;
  }
}

export function isBackendUnavailable(error: unknown): boolean {
  return error instanceof BackendUnavailableError;
}
