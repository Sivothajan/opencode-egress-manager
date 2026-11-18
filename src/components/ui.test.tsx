import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Badge, Button, Modal } from "./ui";

describe("Button", () => {
  it("renders its label and honors disabled", async () => {
    const user = userEvent.setup();
    render(<Button disabled>Cannot press</Button>);
    const button = screen.getByRole("button", { name: "Cannot press" });
    expect(button).toBeDisabled();
    await user.click(button);
    // No crash and still present.
    expect(button).toBeInTheDocument();
  });

  it("maps the primary variant to the filled style", () => {
    render(<Button variant="primary">Go</Button>);
    expect(screen.getByRole("button", { name: "Go" })).toHaveClass("bg-primary");
  });
});

describe("Badge", () => {
  it("applies a tone class", () => {
    render(<Badge tone="bad">down</Badge>);
    expect(screen.getByText("down")).toHaveClass("text-destructive");
  });
});

describe("Modal", () => {
  it("exposes an accessible dialog when open", () => {
    render(
      <Modal open onClose={() => {}} title="Confirm" description="Are you sure?">
        Details
      </Modal>,
    );
    expect(screen.getByRole("dialog", { name: "Confirm" })).toBeInTheDocument();
    expect(screen.getByText("Are you sure?")).toBeInTheDocument();
  });
});
