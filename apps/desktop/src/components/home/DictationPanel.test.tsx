import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { mockSettings } from "../../lib/tauri-mock";
import { makeT } from "../../i18n";
import { DictationPanel } from "./DictationPanel";

const t = makeT("en");

function props(overrides: Partial<Parameters<typeof DictationPanel>[0]> = {}) {
  return {
    settings: structuredClone(mockSettings),
    apiStatus: {
      asr: { state: "ready" as const, profile_id: "asr", credential_id: "key", message: null },
      llm: { state: "ready" as const, profile_id: "llm", credential_id: "key", message: null },
    },
    session: "idle" as const,
    mode: "dictate" as const,
    targetLanguage: "English",
    level: 0,
    locale: "en" as const,
    t,
    onModeChange: vi.fn(),
    onTargetChange: vi.fn(),
    onStart: vi.fn(),
    onCancel: vi.fn(),
    onOpenHotkeys: vi.fn(),
    ...overrides,
  };
}

describe("DictationPanel", () => {
  it("blocks translation until the LLM is ready", () => {
    const panelProps = props({
      mode: "translate",
      apiStatus: {
        asr: { state: "ready", profile_id: "asr", credential_id: "key", message: null },
        llm: { state: "missing_key", profile_id: "llm", credential_id: null, message: null },
      },
    });
    const { rerender } = render(<DictationPanel {...panelProps} />);
    expect(screen.getByRole("button", { name: "Start translating" })).toBeDisabled();
    expect(screen.getByText("Translation requires a working LLM provider.")).toBeInTheDocument();

    rerender(<DictationPanel {...props({ mode: "translate" })} />);
    expect(screen.getByRole("button", { name: "Start translating" })).toBeEnabled();
  });

  it("shows the hotkey for the selected mode and links to its settings", async () => {
    const user = userEvent.setup();
    const panelProps = props();
    render(<DictationPanel {...panelProps} />);
    expect(screen.getByText("Right Alt")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Change hotkeys/ }));
    expect(panelProps.onOpenHotkeys).toHaveBeenCalledOnce();
  });

  it("keeps the saved translation target selectable", () => {
    render(<DictationPanel {...props({ mode: "translate", targetLanguage: "English" })} />);
    expect(screen.getByRole("combobox")).toHaveTextContent("English");
  });

  it("starts and cancels an active session", async () => {
    const user = userEvent.setup();
    const panelProps = props();
    const { rerender } = render(<DictationPanel {...panelProps} />);
    await user.click(screen.getByRole("button", { name: "Start dictating" }));
    expect(panelProps.onStart).toHaveBeenCalledOnce();

    const recordingProps = props({ session: "recording", level: 0.8 });
    rerender(<DictationPanel {...recordingProps} />);
    await user.click(screen.getByRole("button", { name: "Cancel and discard" }));
    expect(recordingProps.onCancel).toHaveBeenCalledOnce();
  });
});
