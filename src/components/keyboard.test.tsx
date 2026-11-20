import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

describe("keyboard accessibility", () => {
  it("moves focus with Tab and Shift+Tab", async () => {
    const user = userEvent.setup();
    render(
      <div>
        <button type="button">One</button>
        <button type="button">Two</button>
      </div>,
    );

    await user.tab();
    expect(screen.getByRole("button", { name: "One" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Two" })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "One" })).toHaveFocus();
  });

  it("activates a focused button with Enter and Space", async () => {
    const user = userEvent.setup();
    const onEnter = vi.fn();
    const onSpace = vi.fn();
    render(
      <div>
        <button type="button" onKeyDown={onEnter} onClick={onEnter}>
          Enter target
        </button>
        <button type="button" onKeyDown={onSpace} onClick={onSpace}>
          Space target
        </button>
      </div>,
    );

    await user.tab();
    await user.keyboard("{Enter}");
    expect(onEnter).toHaveBeenCalled();

    await user.tab();
    await user.keyboard(" ");
    expect(onSpace).toHaveBeenCalled();
  });
});
