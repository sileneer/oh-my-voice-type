import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Settings } from "../lib/api";
import { mockSettings } from "../lib/tauri-mock";
import { makeT } from "../i18n";

const mocks = vi.hoisted(() => ({
  saveAsrProfile: vi.fn(),
  saveLlmProfile: vi.fn(),
  listModels: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: mocks,
}));

import ProviderProfiles from "./ProviderProfiles";

const t = makeT("en");

beforeAll(() => {
  HTMLElement.prototype.hasPointerCapture = vi.fn(() => false);
  HTMLElement.prototype.setPointerCapture = vi.fn();
  HTMLElement.prototype.releasePointerCapture = vi.fn();
  HTMLElement.prototype.scrollIntoView = vi.fn();
  globalThis.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

function emptySettings(): Settings {
  return {
    ...structuredClone(mockSettings),
    asr_profiles: [],
    llm_profiles: [],
    active_asr_profile_id: null,
    active_llm_profile_id: null,
  };
}

function renderProfiles(settings = emptySettings()) {
  const onChanged = vi.fn(async () => {});
  const onError = vi.fn();
  render(
    <ProviderProfiles
      settings={settings}
      apiStatus={{
        asr: { state: "missing_profile", profile_id: null, credential_id: null, message: null },
        llm: { state: "missing_profile", profile_id: null, credential_id: null, message: null },
      }}
      t={t}
      onChanged={onChanged}
      onError={onError}
    />,
  );
  return { onChanged, onError };
}

describe("ProviderProfiles", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.saveAsrProfile.mockResolvedValue(emptySettings());
    mocks.saveLlmProfile.mockResolvedValue(emptySettings());
    mocks.listModels.mockResolvedValue({ outcome: "no_matching_models", models: [] });
  });

  it("creates an ASR provider from a preset with only an API key", async () => {
    const { onChanged, onError } = renderProfiles();
    expect(screen.queryByText("Credentials")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "New ASR profile" }));
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "OpenAI Whisper" }));
    expect(screen.getByRole("link", { name: "Get an API key" })).toHaveAttribute(
      "href",
      "https://platform.openai.com/api-keys",
    );
    await userEvent.type(screen.getByLabelText("API key"), "sk-test-key");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.saveAsrProfile).toHaveBeenCalledOnce());
    expect(mocks.saveAsrProfile).toHaveBeenCalledWith(
      expect.objectContaining({
        label: "OpenAI Whisper",
        preset: "openai",
        engine: "openai_compatible",
        base_url: "https://api.openai.com/v1",
        model: "gpt-transcribe",
        extra_options: [["response_format", "json"]],
      }),
      "sk-test-key",
      false,
    );
    expect(onChanged).toHaveBeenCalledOnce();
    expect(onError).not.toHaveBeenCalled();
  });

  it("fetches live available models after a valid API key is entered and lets the user choose from the dropdown", async () => {
    mocks.listModels.mockResolvedValue({
      outcome: "ok",
      models: [
        { id: "gpt-transcribe" },
        { id: "gpt-4o-mini-transcribe" },
        { id: "whisper-1" },
      ],
    });
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "OpenAI Whisper" }));
    await userEvent.type(screen.getByLabelText("API key"), "sk-openai-live-key");

    await waitFor(() => expect(mocks.listModels).toHaveBeenCalledWith(
      expect.objectContaining({
        role: "asr",
        protocol: "open_ai",
        base_url: "https://api.openai.com/v1",
        allow_unfiltered_fallback: false,
        api_key: "sk-openai-live-key",
      }),
    ));
    await waitFor(() => expect(screen.getByText("3 available")).toBeInTheDocument());

    await userEvent.click(screen.getByRole("combobox", { name: "Choose available model" }));
    await userEvent.click(screen.getByRole("option", { name: "whisper-1" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.saveAsrProfile).toHaveBeenCalledOnce());
    expect(mocks.saveAsrProfile.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        preset: "openai",
        model: "whisper-1",
      }),
    );
  });

  it("shows a calm neutral notice without an error alert when /models lists no ASR models", async () => {
    mocks.listModels.mockResolvedValue({ outcome: "no_matching_models", models: [] });
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "Groq Whisper" }));
    await userEvent.type(screen.getByLabelText("API key"), "gsk-valid-key-1234");

    await waitFor(() =>
      expect(screen.getByText(/did not list speech models/i)).toBeInTheDocument(),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Model")).toHaveValue("whisper-large-v3-turbo");
  });

  it("switches dual-region endpoints and updates the API key console link", async () => {
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "New LLM profile" }));
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "MiniMax" }));
    expect(screen.getByRole("link", { name: "Get an API key" })).toHaveAttribute(
      "href",
      "https://platform.minimaxi.com/user-center/basic-information/interface-key",
    );

    await userEvent.click(screen.getByRole("button", { name: /Global/ }));
    expect(screen.getByRole("link", { name: "Get an API key" })).toHaveAttribute(
      "href",
      "https://platform.minimax.io/",
    );
    await userEvent.type(screen.getByLabelText("API key"), "sk-minimax-global");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.saveLlmProfile).toHaveBeenCalledOnce());
    expect(mocks.saveLlmProfile.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        preset: "minimax",
        base_url: "https://api.minimax.io/v1",
        model: "MiniMax-M3.1-Flash",
      }),
    );
  });

  it("shows required custom fields immediately when Custom provider is selected", async () => {
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "New LLM profile" }));
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "Custom provider" }));

    expect(screen.getByLabelText("Base URL")).toBeVisible();
    expect(screen.getByLabelText("Model")).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    // Malformed URL or insecure remote HTTP should keep Save disabled with inline guidance
    await userEvent.type(screen.getByLabelText("Base URL"), "http://llm.example.com/v1");
    await userEvent.type(screen.getByLabelText("Model"), "my-model");
    await userEvent.type(screen.getByLabelText("API key"), "sk-custom");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await userEvent.clear(screen.getByLabelText("Base URL"));
    await userEvent.type(screen.getByLabelText("Base URL"), "https://llm.example.com/v1");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("clears live-model error when the API key is cleared", async () => {
    mocks.listModels.mockRejectedValueOnce(new Error("models: authentication failed"));
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "OpenAI Whisper" }));
    const keyInput = screen.getByLabelText("API key");
    await userEvent.type(keyInput, "sk-bad-key-1234");
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/authentication failed/),
    );
    await userEvent.clear(keyInput);
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("does not query listModels with the saved key after changing region host on an existing profile", async () => {
    const settings: Settings = {
      ...structuredClone(mockSettings),
      llm_profiles: [
        {
          id: "llm-minimax",
          label: "MiniMax",
          engine: "custom",
          format: "open_ai_chat",
          preset: "minimax",
          base_url: "https://api.minimaxi.com/v1",
          model: "MiniMax-M3.1-Flash",
          credential_id: "cred-mm",
          credential_hint: "9999",
          allow_insecure_http: false,
        },
      ],
      active_llm_profile_id: "llm-minimax",
    };
    mocks.listModels.mockResolvedValue({ outcome: "ok", models: [{ id: "MiniMax-M3.1-Flash" }] });
    renderProfiles(settings);
    await userEvent.click(screen.getByRole("button", { name: "Edit: MiniMax" }));
    await waitFor(() => expect(mocks.listModels).toHaveBeenCalledTimes(1));

    mocks.listModels.mockClear();
    await userEvent.click(screen.getByRole("button", { name: /Global/ }));
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(mocks.listModels).not.toHaveBeenCalled();
  });

  it("applies StepFun ASR request options from its preset", async () => {
    renderProfiles();
    await userEvent.click(screen.getByRole("button", { name: "New ASR profile" }));
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "StepFun ASR" }));
    await userEvent.type(screen.getByLabelText("API key"), "sk-stepfun-test");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.saveAsrProfile).toHaveBeenCalledOnce());
    expect(mocks.saveAsrProfile.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        preset: "stepfun",
        base_url: "https://api.stepfun.com/v1",
        model: "stepaudio-2.5-asr",
        extra_options: expect.arrayContaining([
          ["response_format", "json"],
          ["send_temperature", "false"],
          ["send_language", "false"],
          ["send_prompt", "false"],
        ]),
      }),
    );
  });

  it("requires a fresh key when switching providers and restores it if switched back", async () => {
    renderProfiles(structuredClone(mockSettings));
    await userEvent.click(screen.getByRole("button", { name: "Edit: OpenAI Whisper" }));
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "Groq Whisper" }));

    expect(screen.getByRole("combobox", { name: "Provider" })).toHaveTextContent("Groq Whisper");
    expect(screen.queryByText(/A key ending in 1234 is saved/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(mocks.saveAsrProfile).not.toHaveBeenCalled();

    // Switching back to the original preset restores the saved key reference
    await userEvent.click(screen.getByRole("combobox", { name: "Provider" }));
    await userEvent.click(screen.getByRole("option", { name: "OpenAI Whisper" }));
    expect(screen.getByText(/A key ending in 1234 is saved/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("keeps an existing key when the key field is left blank", async () => {
    const settings = structuredClone(mockSettings);
    const { onChanged } = renderProfiles(settings);
    await userEvent.click(screen.getByRole("button", { name: "Edit: OpenAI Whisper" }));
    expect(screen.getByText(/A key ending in 1234 is saved/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.saveAsrProfile).toHaveBeenCalledOnce());
    expect(mocks.saveAsrProfile.mock.calls[0]?.[1]).toBeNull();
    expect(onChanged).toHaveBeenCalledOnce();
  });
});
