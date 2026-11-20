import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { focusActiveNavOnRouteChange, isTauriEnvironment } from "./tauri";

describe("focusActiveNavOnRouteChange", () => {
  beforeEach(() => {
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("detects the Tauri environment", () => {
    expect(isTauriEnvironment()).toBe(true);
  });

  it("moves focus to the active nav link when body has focus", () => {
    render(
      <nav>
        <a href="#/">Home</a>
        <a href="#/settings" className="active">
          Settings
        </a>
      </nav>,
    );
    const settings = screen.getByRole("link", { name: "Settings" });
    expect(document.activeElement).toBe(document.body);

    focusActiveNavOnRouteChange();
    expect(settings).toHaveFocus();
  });

  it("does not steal focus from an already focused control", () => {
    render(
      <nav>
        <a href="#/">Home</a>
        <a href="#/settings" className="active">
          Settings
        </a>
        <button type="button">Action</button>
      </nav>,
    );
    const action = screen.getByRole("button", { name: "Action" });
    action.focus();

    focusActiveNavOnRouteChange();
    expect(action).toHaveFocus();
  });
});
