import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import DashboardPage from "./DashboardPage";
import { makeState } from "@/tests/fixture";
import { renderWithProviders } from "@/test-utils";
import { getState, hopNow } from "@/services/api";

vi.mock("@/services/api", () => ({
  getState: vi.fn(),
  getInterfaces: vi.fn(),
  fixEverything: vi.fn().mockResolvedValue({ ok: true, message: "fixed" }),
  fixIssue: vi.fn().mockResolvedValue({ ok: true, message: "fixed" }),
  hopNow: vi.fn().mockResolvedValue({ ok: true, message: "Rotated /64" }),
  launchCli: vi.fn().mockResolvedValue({ ok: true, message: "opened" }),
  launchDesktop: vi.fn().mockResolvedValue({ ok: true, message: "launched" }),
  repairWarp: vi.fn().mockResolvedValue({ ok: true, message: "repaired" }),
  setEgressMode: vi.fn().mockResolvedValue({ ok: true, message: "switched" }),
  warpConnect: vi.fn().mockResolvedValue({ ok: true, message: "connected" }),
  warpDisconnect: vi.fn().mockResolvedValue({ ok: true, message: "disconnected" }),
}));

describe("DashboardPage", () => {
  beforeEach(() => {
    vi.mocked(getState).mockResolvedValue(makeState());
  });

  it("renders the healthy summary and egress prefix", async () => {
    renderWithProviders(<DashboardPage />);
    expect(await screen.findByText("Everything is working")).toBeInTheDocument();
    expect(screen.getByText("2606:4700:1234:5678::")).toBeInTheDocument();
  });

  it("shows actionable health issues", async () => {
    vi.mocked(getState).mockResolvedValue(
      makeState({
        health: {
          status: "bad",
          title: "OpenCode needs attention",
          sub: "Fix the problem below.",
          issues: [
            {
              level: "bad",
              title: "Local proxy is down",
              detail: "Port 41987 is not listening.",
              fix: "start_keeper",
              fixLabel: "Start it",
            },
          ],
        },
      }),
    );
    renderWithProviders(<DashboardPage />);
    expect(await screen.findByText("Local proxy is down")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start it" })).toBeInTheDocument();
  });

  it("invokes the hop command when Rotate now is pressed", async () => {
    const user = userEvent.setup();
    renderWithProviders(<DashboardPage />);
    await screen.findByText("Everything is working");
    await user.click(screen.getByRole("button", { name: /rotate now/i }));
    await waitFor(() => expect(hopNow).toHaveBeenCalledTimes(1));
  });

  it("renders an error state when the backend fails", async () => {
    vi.mocked(getState).mockRejectedValue(new Error("backend down"));
    renderWithProviders(<DashboardPage />);
    expect(
      await screen.findByText("Could not read application state", undefined, { timeout: 9000 }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});
