const form = document.querySelector("#settings-form");
const endpoint = document.querySelector("#endpoint");
const model = document.querySelector("#model");
const apiKey = document.querySelector("#api-key");
const keyField = document.querySelector("#key-field");
const keyStatus = document.querySelector("#key-status");
const clearKey = document.querySelector("#clear-key");
const deepseekTools = document.querySelector("#deepseek-tools");
const deepseekSearch = document.querySelector("#deepseek-search");
const deepseekFetch = document.querySelector("#deepseek-fetch");
const alwaysOnTop = document.querySelector("#always-on-top");
const sound = document.querySelector("#sound");
const freeWalk = document.querySelector("#free-walk");
const interactiveZones = document.querySelector("#interactive-zones");
const spicyLines = document.querySelector("#spicy-lines");
const scale = document.querySelector("#scale");
const scaleOutput = document.querySelector("#scale-output");
const careReminders = document.querySelector("#care-reminders");
const waterEvery = document.querySelector("#water-every");
const moveEvery = document.querySelector("#move-every");
const focusMinutes = document.querySelector("#focus-minutes");
const musicPlayer = document.querySelector("#music-player");
const behaviorIntensity = document.querySelector("#behavior-intensity");
const agentLoopLimit = document.querySelector("#agent-loop-limit");
const toolCallLimit = document.querySelector("#tool-call-limit");
const sandboxedCommands = document.querySelector("#sandboxed-commands");
const behaviorOutput = document.querySelector("#behavior-output");
const status = document.querySelector("#status");
const resetProvider = document.querySelector("#reset-provider");

// Seatbelt is macOS-only, so on other platforms the switch would quietly do
// nothing. Disable it and state what actually happens to approved commands.
if (!/Macintosh|Mac OS X/.test(navigator.userAgent)) {
  sandboxedCommands.checked = false;
  sandboxedCommands.disabled = true;
  const sandboxHint = document.querySelector("#sandbox-hint");
  if (sandboxHint) {
    sandboxHint.textContent =
      "系统沙箱依赖 macOS 自带的 sandbox-exec；当前平台没有沙箱，需要审批的命令会直接以你的权限运行。";
  }
}

const presets = {
  pollinations: { endpoint: "https://text.pollinations.ai/openai", model: "openai-fast" },
  deepseek: { endpoint: "https://api.deepseek.com/chat/completions", model: "deepseek-chat" },
  custom: { endpoint: "", model: "" }
};

function selectedProvider() {
  return form.elements.provider.value;
}

function updateKeyVisibility() {
  const needsKey = selectedProvider() !== "pollinations";
  keyField.hidden = !needsKey;
  clearKey.parentElement.hidden = !needsKey;
  deepseekTools.hidden = selectedProvider() !== "deepseek";
}

function selectProvider(provider, fillPreset = true) {
  const radio = form.querySelector(`input[name="provider"][value="${provider}"]`);
  if (radio) radio.checked = true;
  if (fillPreset && presets[provider]) {
    endpoint.value = presets[provider].endpoint;
    model.value = presets[provider].model;
  }
  updateKeyVisibility();
}

form.elements.provider.forEach((radio) => radio.addEventListener("change", () => selectProvider(radio.value)));
scale.addEventListener("input", () => { scaleOutput.value = `${Math.round(Number(scale.value) * 100)}%`; });
behaviorIntensity.addEventListener("input", () => { behaviorOutput.value = `${Math.round(Number(behaviorIntensity.value) * 100)}%`; });
resetProvider.addEventListener("click", () => selectProvider("pollinations"));

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  status.textContent = "正在保存...";
  try {
    const saved = await window.deepFish.saveSettings({
      provider: selectedProvider(),
      endpoint: endpoint.value.trim(),
      model: model.value.trim(),
      apiKey: apiKey.value,
      clearApiKey: clearKey.checked,
      alwaysOnTop: alwaysOnTop.checked,
      sound: sound.checked,
      freeWalk: freeWalk.checked,
      interactiveZones: interactiveZones.checked,
      spicyLines: spicyLines.checked,
      scale: Number(scale.value),
      behaviorIntensity: Number(behaviorIntensity.value),
      careReminders: careReminders.checked,
      waterEveryMin: Number(waterEvery.value),
      moveEveryMin: Number(moveEvery.value),
      focusMinutes: Number(focusMinutes.value),
      musicPlayer: musicPlayer.value,
      deepseekSearch: deepseekSearch.checked,
      deepseekFetch: deepseekFetch.checked,
      agentLoopLimit: Number(agentLoopLimit.value),
      toolCallLimit: Number(toolCallLimit.value),
      sandboxedCommands: sandboxedCommands.checked
    });
    apiKey.value = "";
    clearKey.checked = false;
    keyStatus.textContent = saved.hasApiKey ? "已保存在本机配置文件" : "尚未保存 Key";
    status.textContent = "设置已保存";
  } catch (error) {
    status.textContent = `保存失败：${error.message}`;
  }
});

window.deepFish.getSettings().then((settings) => {
  selectProvider(settings.provider, false);
  endpoint.value = settings.endpoint;
  model.value = settings.model;
  alwaysOnTop.checked = settings.alwaysOnTop;
  sound.checked = settings.sound;
  freeWalk.checked = settings.freeWalk;
  interactiveZones.checked = settings.interactiveZones;
  spicyLines.checked = settings.spicyLines;
  scale.value = settings.scale;
  scaleOutput.value = `${Math.round(Number(settings.scale) * 100)}%`;
  deepseekSearch.checked = Boolean(settings.deepseekSearch);
  deepseekFetch.checked = Boolean(settings.deepseekFetch);
  agentLoopLimit.value = settings.agentLoopLimit;
  toolCallLimit.value = settings.toolCallLimit;
  sandboxedCommands.checked = sandboxedCommands.disabled ? false : Boolean(settings.sandboxedCommands);
  behaviorIntensity.value = settings.behaviorIntensity;
  behaviorOutput.value = `${Math.round(Number(settings.behaviorIntensity) * 100)}%`;
  careReminders.checked = settings.careReminders !== false;
  waterEvery.value = settings.waterEveryMin;
  moveEvery.value = settings.moveEveryMin;
  focusMinutes.value = settings.focusMinutes;
  musicPlayer.value = settings.musicPlayer || "auto";
  keyStatus.textContent = settings.hasApiKey ? "已保存在本机配置文件，留空即可保留" : "尚未保存 Key";
});
