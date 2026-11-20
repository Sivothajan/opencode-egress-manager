import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

const mocks = vi.hoisted(() => ({
  tauri: { value: true },
  handler: { current: null as null | (() => void) },
  listen: vi.fn((_event: string, cb: () => void) => {
    mocks.handler.current = cb;
    return Promise.resolve(() => {});
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/tauri", () => ({
  isTauriEnvironment: () => mocks.tauri.value,
}));

import { useWindowFocusRestore } from "./useWindowFocusRestore";

function Probe() {
  useWindowFocusRestore();
  return (
    <div>
      <button type="button">First</button>
      <button type="button">Second</button>
      <a href="#target">Link</a>
    </div>
  );
}

function focusIn(el: Element) {
  el.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
}

describe("useWindowFocusRestore", () => {
  beforeEach(() => {
    mocks.handler.current = null;
    mocks.listen.mockClear();
    mocks.tauri.value = true;
    cleanup();
  });

  it("registers a restore listener only inside Tauri", async () => {
    render(<Probe />);
    await waitFor(() =>
      expect(mocks.listen).toHaveBeenCalledWith("window-focus-restore", expect.any(Function)),
    );

    cleanup();
    mocks.listen.mockClear();
    mocks.tauri.value = false;
    render(<Probe />);
    await waitFor(() => expect(screen.getByText("First")).toBeInTheDocument());
    expect(mocks.listen).not.toHaveBeenCalled();
  });

  it("restores the last focused element when activation finds body focused", async () => {
    render(<Probe />);
    await waitFor(() => expect(mocks.handler.current).toBeTruthy());

    const second = screen.getByRole("button", { name: "Second" });
    focusIn(second);
    second.blur();

    (mocks.handler.current as unknown as () => void)();
    expect(document.activeElement).toBe(second);
  });

  it("does not steal focus from an element that already has it", async () => {
    render(<Probe />);
    await waitFor(() => expect(mocks.handler.current).toBeTruthy());

    const link = screen.getByRole("link", { name: "Link" });
    focusIn(link);
    link.focus();

    (mocks.handler.current as unknown as () => void)();
    expect(document.activeElement).toBe(link);
  });

  it("ignores elements that were removed while unfocused", async () => {
    const { container } = render(<Probe />);
    await waitFor(() => expect(mocks.handler.current).toBeTruthy());

    const second = screen.getByRole("button", { name: "Second" });
    focusIn(second);
    container.remove();
    second.remove();

    expect(() => (mocks.handler.current as unknown as () => void)()).not.toThrow();
  });
});
