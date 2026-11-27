import type { AsrProfile, LlmProfile } from "./api";

export interface EndpointOption {
  id: "cn" | "global";
  labelKey: "profiles.region.cn" | "profiles.region.global";
  baseUrl: string;
  keyUrl: string;
}

export interface ProviderPreset {
  id: string;
  label: string;
  baseUrl: string;
  endpoints?: EndpointOption[];
  /** Fallback default model when live `/models` discovery has not run yet. */
  model: string;
  /** Curated fallback models used before an API key is entered or if `/models` is unavailable. */
  models: string[];
  keyUrl: string;
}

export interface AsrPreset extends ProviderPreset {
  engine: AsrProfile["engine"];
  extraOptions?: [string, string][];
}

export interface LlmPreset extends ProviderPreset {
  engine: LlmProfile["engine"];
  format: LlmProfile["format"];
}

export const asrPresets: AsrPreset[] = [
  {
    id: "groq",
    label: "Groq Whisper",
    engine: "openai_compatible",
    baseUrl: "https://api.groq.com/openai/v1",
    model: "whisper-large-v3-turbo",
    models: ["whisper-large-v3-turbo", "whisper-large-v3"],
    keyUrl: "https://console.groq.com/keys",
  },
  {
    id: "mistral",
    label: "Mistral Voxtral",
    engine: "openai_compatible",
    baseUrl: "https://api.mistral.ai/v1",
    model: "voxtral-mini-transcribe-3",
    models: ["voxtral-mini-transcribe-3", "voxtral-small-transcribe-3"],
    extraOptions: [["response_format", ""]],
    keyUrl: "https://console.mistral.ai/api-keys",
  },
  {
    id: "openai",
    label: "OpenAI Whisper",
    engine: "openai_compatible",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-transcribe",
    models: ["gpt-transcribe", "gpt-4o-mini-transcribe", "gpt-4o-transcribe", "whisper-1"],
    extraOptions: [["response_format", "json"]],
    keyUrl: "https://platform.openai.com/api-keys",
  },
  {
    id: "openrouter",
    label: "OpenRouter Whisper",
    engine: "openai_compatible",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "openai/whisper-large-v3-turbo",
    models: ["openai/whisper-large-v3-turbo", "openai/whisper-large-v3", "openai/whisper-1"],
    keyUrl: "https://openrouter.ai/settings/keys",
  },
  {
    id: "siliconflow",
    label: "SiliconFlow ASR",
    engine: "openai_compatible",
    baseUrl: "https://api.siliconflow.cn/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.siliconflow.cn/v1",
        keyUrl: "https://cloud.siliconflow.cn/account/ak",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.siliconflow.com/v1",
        keyUrl: "https://cloud.siliconflow.com/account/ak",
      },
    ],
    model: "FunAudioLLM/SenseVoiceSmall",
    models: ["FunAudioLLM/SenseVoiceSmall", "TeleAI/TeleSpeechASR"],
    extraOptions: [
      ["response_format", ""],
      ["send_temperature", "false"],
      ["send_language", "false"],
      ["send_prompt", "false"],
    ],
    keyUrl: "https://cloud.siliconflow.cn/account/ak",
  },
  {
    id: "stepfun",
    label: "StepFun ASR",
    engine: "openai_compatible",
    baseUrl: "https://api.stepfun.com/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.stepfun.com/v1",
        keyUrl: "https://platform.stepfun.com/interface-key",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.stepfun.ai/v1",
        keyUrl: "https://platform.stepfun.ai/interface-key",
      },
    ],
    model: "stepaudio-2.5-asr",
    models: ["stepaudio-2.5-asr", "step-asr"],
    extraOptions: [
      ["response_format", "json"],
      ["send_temperature", "false"],
      ["send_language", "false"],
      ["send_prompt", "false"],
    ],
    keyUrl: "https://platform.stepfun.com/interface-key",
  },
  {
    id: "unisound-u2",
    label: "Unisound U2-ASR",
    engine: "unisound_u2",
    baseUrl: "https://maas-api.unisound.com",
    model: "u2-asr",
    models: ["u2-asr"],
    keyUrl: "https://maas.unisound.com/",
  },
  {
    id: "zhipu",
    label: "Zhipu GLM ASR",
    engine: "openai_compatible",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://open.bigmodel.cn/api/paas/v4",
        keyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.z.ai/api/paas/v4",
        keyUrl: "https://z.ai/model-api",
      },
    ],
    model: "glm-asr-2512",
    models: ["glm-asr-2512", "glm-asr"],
    extraOptions: [
      ["response_format", ""],
      ["send_temperature", "false"],
      ["send_language", "false"],
    ],
    keyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
  },
];

export const llmPresets: LlmPreset[] = [
  {
    id: "dashscope",
    label: "Alibaba Qwen",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        keyUrl: "https://bailian.console.aliyun.com/?apiKey=1",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        keyUrl: "https://modelstudio.console.alibabacloud.com/",
      },
    ],
    model: "qwen-flash",
    models: ["qwen-flash", "qwen-plus", "qwen-max"],
    keyUrl: "https://bailian.console.aliyun.com/?apiKey=1",
  },
  {
    id: "anthropic",
    label: "Anthropic Claude",
    engine: "custom",
    format: "anthropic_messages",
    baseUrl: "https://api.anthropic.com",
    model: "claude-haiku-5-5",
    models: ["claude-haiku-5-5", "claude-sonnet-5-5"],
    keyUrl: "https://console.anthropic.com/settings/keys",
  },
  {
    id: "baidu",
    label: "Baidu Qianfan",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://qianfan.baidubce.com/v2",
    model: "ernie-4.5-turbo-32k",
    models: ["ernie-4.5-turbo-32k", "ernie-4.5-turbo-128k", "ernie-speed-8k"],
    keyUrl: "https://console.bce.baidu.com/iam/#/iam/apikey/list",
  },
  {
    id: "cohere",
    label: "Cohere",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.cohere.ai/compatibility/v1",
    model: "command-a-plus-05-2026",
    models: ["command-a-plus-05-2026", "command-a-03-2025"],
    keyUrl: "https://dashboard.cohere.com/api-keys",
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-flash",
    models: ["deepseek-flash", "deepseek-v4-pro", "deepseek-chat"],
    keyUrl: "https://platform.deepseek.com/api_keys",
  },
  {
    id: "gemini",
    label: "Google Gemini",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    model: "gemini-3.1-flash-lite",
    models: ["gemini-3.1-flash-lite", "gemini-3-flash", "gemini-2.5-flash"],
    keyUrl: "https://aistudio.google.com/apikey",
  },
  {
    id: "groq",
    label: "Groq",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.groq.com/openai/v1",
    model: "openai/gpt-oss-20b",
    models: ["openai/gpt-oss-20b", "llama-3.1-8b-instant", "llama-3.3-70b-versatile"],
    keyUrl: "https://console.groq.com/keys",
  },
  {
    id: "minimax",
    label: "MiniMax",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.minimaxi.com/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.minimaxi.com/v1",
        keyUrl: "https://platform.minimaxi.com/user-center/basic-information/interface-key",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.minimax.io/v1",
        keyUrl: "https://platform.minimax.io/",
      },
    ],
    model: "MiniMax-M3.1-Flash",
    models: ["MiniMax-M3.1-Flash", "MiniMax-M3"],
    keyUrl: "https://platform.minimaxi.com/user-center/basic-information/interface-key",
  },
  {
    id: "mistral",
    label: "Mistral",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.mistral.ai/v1",
    model: "mistral-small-latest",
    models: ["mistral-small-latest", "ministral-8b-latest", "mistral-large-latest"],
    keyUrl: "https://console.mistral.ai/api-keys",
  },
  {
    id: "moonshot",
    label: "Moonshot Kimi",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.moonshot.cn/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.moonshot.cn/v1",
        keyUrl: "https://platform.moonshot.cn/console/api-keys",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.moonshot.ai/v1",
        keyUrl: "https://platform.moonshot.ai/",
      },
    ],
    model: "kimi-k2.5",
    models: ["kimi-k2.5", "kimi-k2.7-code"],
    keyUrl: "https://platform.moonshot.cn/console/api-keys",
  },
  {
    id: "openai",
    label: "OpenAI",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-5.4-mini",
    models: ["gpt-5.4-mini", "gpt-5.4-nano", "gpt-4.1-mini"],
    keyUrl: "https://platform.openai.com/api-keys",
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://openrouter.ai/api/v1",
    model: "deepseek/deepseek-v4.1-flash",
    models: [
      "deepseek/deepseek-v4.1-flash",
      "google/gemini-3-flash",
      "anthropic/claude-haiku-5.5",
      "openai/gpt-4o-mini",
    ],
    keyUrl: "https://openrouter.ai/settings/keys",
  },
  {
    id: "siliconflow",
    label: "SiliconFlow",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.siliconflow.cn/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.siliconflow.cn/v1",
        keyUrl: "https://cloud.siliconflow.cn/account/ak",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.siliconflow.com/v1",
        keyUrl: "https://cloud.siliconflow.com/account/ak",
      },
    ],
    model: "Qwen/Qwen3-8B",
    models: ["Qwen/Qwen3-8B", "deepseek-ai/DeepSeek-V3.2"],
    keyUrl: "https://cloud.siliconflow.cn/account/ak",
  },
  {
    id: "stepfun",
    label: "StepFun",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.stepfun.com/v1",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://api.stepfun.com/v1",
        keyUrl: "https://platform.stepfun.com/interface-key",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.stepfun.ai/v1",
        keyUrl: "https://platform.stepfun.ai/interface-key",
      },
    ],
    model: "step-3.5-flash",
    models: ["step-3.5-flash", "step-3.7-flash", "step-5-preview"],
    keyUrl: "https://platform.stepfun.com/interface-key",
  },
  {
    id: "together",
    label: "Together AI",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.together.xyz/v1",
    model: "deepseek-ai/DeepSeek-V4.1-Flash",
    models: [
      "deepseek-ai/DeepSeek-V4.1-Flash",
      "Qwen/Qwen3.5-9B",
      "meta-llama/Llama-3.3-70B-Instruct-Turbo",
    ],
    keyUrl: "https://api.together.ai/settings/api-keys",
  },
  {
    id: "unisound",
    label: "Unisound",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://maas-api.unisound.com/v1",
    model: "u2-flash",
    models: ["u2-flash", "u2", "u2-med", "u2-radimed"],
    keyUrl: "https://maas.unisound.com/",
  },
  {
    id: "volcengine",
    label: "Volcengine Doubao",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    model: "doubao-seed-2.1-turbo",
    models: [
      "doubao-seed-2.1-turbo",
      "doubao-seed-2-1-pro-260915",
      "doubao-seed-1-6-251015",
    ],
    keyUrl: "https://ark.volcengine.com/region:cn-beijing/apikey",
  },
  {
    id: "xai",
    label: "xAI Grok",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://api.x.ai/v1",
    model: "grok-4.3",
    models: ["grok-4.3", "grok-4.7"],
    keyUrl: "https://console.x.ai/",
  },
  {
    id: "zhipu",
    label: "Zhipu GLM",
    engine: "custom",
    format: "open_ai_chat",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    endpoints: [
      {
        id: "cn",
        labelKey: "profiles.region.cn",
        baseUrl: "https://open.bigmodel.cn/api/paas/v4",
        keyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
      },
      {
        id: "global",
        labelKey: "profiles.region.global",
        baseUrl: "https://api.z.ai/api/paas/v4",
        keyUrl: "https://z.ai/model-api",
      },
    ],
    model: "glm-4.7-flash",
    models: ["glm-4.7-flash", "glm-4-flash"],
    keyUrl: "https://open.bigmodel.cn/usercenter/apikeys",
  },
];

export function findAsrPreset(id: string | null | undefined): AsrPreset | undefined {
  return id ? asrPresets.find((preset) => preset.id === id) : undefined;
}

export function findLlmPreset(id: string | null | undefined): LlmPreset | undefined {
  return id ? llmPresets.find((preset) => preset.id === id) : undefined;
}

function normalizeUrl(url: string | null | undefined): string {
  return (url ?? "").trim().replace(/\/+$/, "");
}

/** Resolve the console API-key URL matching the currently selected regional endpoint. */
export function resolvePresetKeyUrl(
  preset: ProviderPreset | undefined,
  baseUrl: string | null | undefined,
): string | undefined {
  if (!preset) return undefined;
  const current = normalizeUrl(baseUrl);
  if (current && preset.endpoints) {
    const matched = preset.endpoints.find((option) => normalizeUrl(option.baseUrl) === current);
    if (matched) return matched.keyUrl;
  }
  return preset.keyUrl;
}
