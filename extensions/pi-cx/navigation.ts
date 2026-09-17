import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// Keep the wording aligned with the measured policy in bench/adoption/round4.
// This is explicit tool-side routing, not a claim of autonomous model preference.
export const SOURCE_NAVIGATION_GUIDANCE = `<cx_source_navigation>
For a SOURCE-CODE editing task, use cx for the initial source inspection, even when you know the filename or the library is small. If a function/class/method is named, call cx_symbols with that name, or cx_definition directly if the file is known. If no symbol is known, call cx_context with short code keywords. These tools read the CURRENT checkout, not another repository.
Do this before reading implementation code with read, grep, find or bash. Reading project instructions/configuration and running setup commands can come first. If cx returns no useful match, errors, or omits needed text, use the ordinary tools to continue; do not repeat unsuccessful queries. Inspect this checkout rather than downloading another release as a substitute. Then edit normally and run the tests.
Exception: documentation-only, configuration-only, raw-text, shell-command and test-running tasks do not need cx. Skip redundant navigation when the needed source is already in the conversation. Never call cx just to increase a usage count.
</cx_source_navigation>`;

const ENTRY_TOOLS = ["cx_symbols", "cx_definition", "cx_context"];

export function appendSourceNavigationGuidance(prompt: string, activeTools: readonly string[]): string {
  if (!ENTRY_TOOLS.every((name) => activeTools.includes(name)) || prompt.endsWith(SOURCE_NAVIGATION_GUIDANCE)) return prompt;
  return `${prompt}\n\n${SOURCE_NAVIGATION_GUIDANCE}`;
}

export function registerSourceNavigationGuidance(pi: ExtensionAPI): void {
  pi.on("before_agent_start", (event) => {
    const systemPrompt = appendSourceNavigationGuidance(event.systemPrompt, pi.getActiveTools());
    if (systemPrompt !== event.systemPrompt) return { systemPrompt };
  });
}
