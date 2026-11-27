import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings, UpdaterSnapshot } from "../lib/api";
import { mockSettings } from "../lib/tauri-mock";
import { makeT } from "../i18n";

const mocks = vi.hoisted(() => ({
  updaterStatus: vi.fn(),
  updaterCheck: vi.fn(),
  updaterInstall: vi.fn(),
  updaterRestart: vi.fn(),
  onUpdaterChanged: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: {
    updaterStatus: mocks.updaterStatus,
    updaterCheck: mocks.updaterCheck,
    updaterInstall: mocks.updaterInstall,
    updaterRestart: mocks.updaterRestart,
  },
  events: { onUpdaterChanged: mocks.onUpdaterChanged },
}));

import { plainNotes, statusMessage, UpdatePanel } from "./UpdatePanel";

const t = makeT("en");

function snapshot(overrides: Partial<UpdaterSnapshot> = {}): UpdaterSnapshot {
  return { status: "idle", current_version: "0.1.0", update: null, progress: null, message: null, checked_at_ms: null, ...overrides };
}

const available = snapshot({
  status: "available",
  update: { version: "0.2.0", notes: "Faster dictation", pub_date: "2026-11-26T12:00:00Z" },
});

function renderPanel(onSave = vi.fn(async (_settings: Settings) => {})) {
  const onError = vi.fn();
  render(<UpdatePanel settings={structuredClone(mockSettings)} locale="en" t={t} onSave={onSave} onError={onError} />);
  return { onSave, onError };
}

describe("UpdatePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.onUpdaterChanged.mockResolvedValue(() => {});
    mocks.updaterInstall.mockResolvedValue(undefined);
    mocks.updaterRestart.mockResolvedValue(undefined);
  });

  it("shows release notes and installs an available update", async () => {
    mocks.updaterStatus.mockResolvedValue(available);
    renderPanel();
    expect(await screen.findByText("Version 0.2.0 is available.")).toBeInTheDocument();
    expect(screen.getByText("Faster dictation")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Download and install" }));
    expect(mocks.updaterInstall).toHaveBeenCalledOnce();
  });

  it("offers a restart once the update is installed", async () => {
    mocks.updaterStatus.mockResolvedValue({ ...available, status: "ready", progress: 1 });
    renderPanel();
    const restart = await screen.findByRole("button", { name: "Restart now" });
    expect(screen.queryByRole("button", { name: "Check for updates" })).not.toBeInTheDocument();
    await userEvent.click(restart);
    expect(mocks.updaterRestart).toHaveBeenCalledOnce();
  });

  it("reports the result of a manual check", async () => {
    mocks.updaterStatus.mockResolvedValue(snapshot());
    mocks.updaterCheck.mockResolvedValue(snapshot({ status: "up-to-date", checked_at_ms: Date.now() }));
    renderPanel();
    await userEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("You are up to date.")).toBeInTheDocument();
    expect(screen.getByText(/Last checked/)).toBeInTheDocument();
  });

  it("saves the automatic check preference", async () => {
    mocks.updaterStatus.mockResolvedValue(snapshot());
    const { onSave } = renderPanel();
    await userEvent.click(await screen.findByRole("switch", { name: "Check for updates automatically" }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0]?.[0].ui.check_for_updates).toBe(false);
  });

  it("strips Markdown markers from release notes", () => {
    expect(plainNotes("## What's Changed\n* Fix by **@dev** in [#4](https://x.test/4)\n"))
      .toBe("What's Changed\n* Fix by @dev in #4 (https://x.test/4)");
  });

  it("formats download progress", () => {
    expect(statusMessage({ ...available, status: "downloading", progress: 0.426 }, t)).toBe("Downloading version 0.2.0… 43%");
    expect(statusMessage({ ...available, status: "downloading", progress: null }, t)).toBe("Downloading version 0.2.0…");
  });
});
