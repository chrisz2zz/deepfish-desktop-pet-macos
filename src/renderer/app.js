const appRoot = document.querySelector("#pet-app");
const stage = document.querySelector("#pet-stage");
const character = document.querySelector("#character");
const speech = document.querySelector("#speech");
const phaseLabel = document.querySelector("#phase-label");
const speechText = document.querySelector("#speech-text");
const effectLayer = document.querySelector("#effect-layer");
const chatPanel = document.querySelector("#chat-panel");
const chatLog = document.querySelector("#chat-log");
const chatForm = document.querySelector("#chat-form");
const chatInput = document.querySelector("#chat-input");
const chatClose = document.querySelector("#chat-close");
const chatFab = document.querySelector("#chat-fab");
const providerLabel = document.querySelector("#provider-label");
const sessionButton = document.querySelector("#session-button");
const sessionTitle = document.querySelector("#session-title");
const sessionRunning = document.querySelector("#session-running");
const sessionList = document.querySelector("#session-list");
const sessionItems = document.querySelector("#session-items");
const sessionNew = document.querySelector("#session-new");
const sessionCompact = document.querySelector("#session-compact");

const behaviorApi = window.DeepFishBehaviors;
const gestureApi = window.DeepFishGestures;
const frameAliases = { dream: "sleep", dizzy: "shock" };
const frameSequences = {
  ciallo: { names: ["ciallo", "ciallo-b"], interval: 420 }
};
const frameNames = [
  "neutral", "blink", "walk", "walk-b", "wash", "work", "coffee", "toy", "sleep", "hungry",
  "sit", "pat", "feed", "shy", "trip", "cry", "think", "smug", "angry", "ciallo", "ciallo-b",
  "fly", "price", "panic", "rival", "shock", "pressure", "stranded", "stretch",
  "startle", "goAway"
];
const signaturePool = ["ciallo", "smug", "pressure", "shock", "fly", "price", "rival", "goAway", "stranded"];
const behaviorLabels = {
  walk: "散步中", wash: "洗碗中", work: "认真加班", coffee: "咖啡时间", toy: "摸鱼中",
  sleep: "睡觉中", dream: "做梦中", hungry: "肚子咕咕叫", sit: "坐好了", pat: "摸摸头",
  feed: "投喂成功", shy: "害羞", trip: "绊倒了", cry: "委屈巴巴", think: "深度思考",
  smug: "自信", angry: "用户怒了", ciallo: "随机卖萌", fly: "起飞", price: "准备涨价",
  panic: "慌乱", rival: "双枪模式", shock: "震惊", pressure: "压力测试", stranded: "搁浅了",
  stretch: "欢迎回来", startle: "惊醒", dizzy: "转晕了", goAway: "AGI 训练中"
};

const clickLines = ["我有在认真营业。", "好的，用户又发癫了。", "你是不是想摸摸头？", "再点就要收费啦。"];
let currentSettings = {
  provider: "pollinations",
  scale: 1,
  freeWalk: true,
  interactiveZones: true,
  spicyLines: true,
  behaviorIntensity: 1
};
let speechTimer;
let actionTimer;
let sceneTimer;
let idleTimer;
let walkTimer;
let walkStopTimer;
let frameTimer;
let frameLoopTimer;
let blinkTimer;
let walkPending = false;
let walkDirection = -1;
let drag;
let lastHoverAt = 0;
let lastZone = "";
let tripCount = 0;
let scrollPending = false;
const sendButton = chatForm.querySelector("button");
const agentApi = window.deepFish.agent;

let activeSession = null;
let sessionSummaries = [];
let requestSequence = 0;
let running = false;
let pendingAssistant = null;
let noticeTimer;
/// Approval cards that are still waiting for the user, kept so reopening the
/// panel re-renders them.
const pendingApprovals = new Map();
let armedRemoval = "";
let armTimer = 0;
let summaryExpanded = false;

function randomOf(items) {
  return items[Math.floor(Math.random() * items.length)];
}

function framePath(name) {
  const resolvedName = frameAliases[name] || name;
  return `../../assets/frames/frame-${resolvedName}.png`;
}

function restingFrame() {
  return window.DeepFishClock.getPhase().id === "sleep" && chatPanel.hidden ? "sleep" : "neutral";
}

function setFrame(name) {
  const resolvedName = frameAliases[name] || name;
  character.src = framePath(resolvedName);
  character.dataset.frame = resolvedName;
  scheduleHitMask();
}

// The window is a transparent rectangle, so the renderer reports which cells of
// the sprite actually contain pixels and the backend makes the rest click-through.
const HIT_CELL = 8;
const hitCanvas = document.createElement("canvas");
const hitContext = hitCanvas.getContext("2d", { willReadFrequently: true });
const frameAlpha = new Map();
const preparingAlpha = new Set();
let hitBits = null;
let hitCols = 0;
let hitRows = 0;
let hitUpdateHandle = 0;
let hitTicker = 0;
let hitTickerTimer = 0;
let hitError = "";
const alphaRetries = new Map();

function scheduleHitMask() {
  // A timer instead of an animation frame: frames are not delivered while the
  // window is still hidden, which would leave the mask stuck on empty.
  if (hitUpdateHandle) return;
  hitUpdateHandle = setTimeout(() => {
    hitUpdateHandle = 0;
    pushHitMask();
  }, 30);
}

function trackHitMaskWhileAnimating(duration) {
  clearInterval(hitTicker);
  clearTimeout(hitTickerTimer);
  // Keyframe animations move the sprite, so the mask has to follow the frame.
  hitTicker = setInterval(scheduleHitMask, 160);
  hitTickerTimer = setTimeout(() => clearInterval(hitTicker), duration + 80);
}

// Sampling the alpha channel needs a decoded frame, and drawing an image that
// is still decoding silently yields nothing, so maps are prepared off the frame.
function alphaMap(name) {
  const cached = frameAlpha.get(name);
  if (cached) return cached;
  prepareAlphaMap(name);
  return null;
}

async function prepareAlphaMap(name) {
  if (frameAlpha.has(name) || preparingAlpha.has(name)) return;
  const image = frameImages.get(name);
  if (!image) return;
  preparingAlpha.add(name);
  try {
    await image.decode();
    const width = Math.max(1, Math.round(image.naturalWidth / 2));
    const height = Math.max(1, Math.round(image.naturalHeight / 2));
    hitCanvas.width = width;
    hitCanvas.height = height;
    hitContext.clearRect(0, 0, width, height);
    hitContext.drawImage(image, 0, 0, width, height);
    const { data } = hitContext.getImageData(0, 0, width, height);
    const alpha = new Uint8Array(width * height);
    let max = 0;
    for (let index = 0; index < alpha.length; index += 1) {
      alpha[index] = data[index * 4 + 3];
      if (alpha[index] > max) max = alpha[index];
    }
    if (max === 0) {
      // A lazily decoded frame can still draw blank; retry a few times instead
      // of caching an empty mask that would leave the pet unclickable.
      const attempts = (alphaRetries.get(name) || 0) + 1;
      alphaRetries.set(name, attempts);
      if (attempts <= 3) setTimeout(scheduleHitMask, 350);
      return;
    }
    alphaRetries.delete(name);
    frameAlpha.set(name, { width, height, alpha });
    scheduleHitMask();
  } catch (error) {
    hitError = `alpha ${name}: ${error?.message ?? error}`;
  } finally {
    preparingAlpha.delete(name);
  }
}

function setHitBit(bits, index) {
  bits[index >> 3] |= 1 << (index & 7);
}

function markHitRect(bits, rect) {
  if (!rect || rect.width < 1 || rect.height < 1) return;
  const firstCol = Math.max(0, Math.floor(rect.left / HIT_CELL));
  const lastCol = Math.min(hitCols - 1, Math.ceil(rect.right / HIT_CELL) - 1);
  const firstRow = Math.max(0, Math.floor(rect.top / HIT_CELL));
  const lastRow = Math.min(hitRows - 1, Math.ceil(rect.bottom / HIT_CELL) - 1);
  for (let row = firstRow; row <= lastRow; row += 1) {
    for (let col = firstCol; col <= lastCol; col += 1) setHitBit(bits, row * hitCols + col);
  }
}

function sampleAlpha(map, u, v) {
  let max = 0;
  for (let dy = -1; dy <= 1; dy += 1) {
    const y = v + dy;
    if (y < 0 || y >= map.height) continue;
    for (let dx = -1; dx <= 1; dx += 1) {
      const x = u + dx;
      if (x < 0 || x >= map.width) continue;
      if (map.alpha[y * map.width + x] > max) max = map.alpha[y * map.width + x];
    }
  }
  return max;
}

function interactiveRects() {
  const rects = [];
  if (!chatPanel.hidden) {
    rects.push(chatPanel.getBoundingClientRect());
    if (!sessionList.hidden) rects.push(sessionList.getBoundingClientRect());
  } else if (appRoot.matches(":hover")) {
    rects.push(chatFab.getBoundingClientRect());
  }
  if (speech.classList.contains("visible")) rects.push(speech.getBoundingClientRect());
  return rects;
}

function buildHitBits() {
  hitCols = Math.max(1, Math.ceil(window.innerWidth / HIT_CELL));
  hitRows = Math.max(1, Math.ceil(window.innerHeight / HIT_CELL));
  const bits = new Uint8Array(Math.ceil((hitCols * hitRows) / 8));
  const frameName = character.dataset.frame || restingFrame();
  const map = alphaMap(frameName);
  const rect = character.getBoundingClientRect();
  if (map && rect.width > 0 && rect.height > 0) {
    const firstCol = Math.max(0, Math.floor(rect.left / HIT_CELL));
    const lastCol = Math.min(hitCols - 1, Math.ceil(rect.right / HIT_CELL) - 1);
    const firstRow = Math.max(0, Math.floor(rect.top / HIT_CELL));
    const lastRow = Math.min(hitRows - 1, Math.ceil(rect.bottom / HIT_CELL) - 1);
    for (let row = firstRow; row <= lastRow; row += 1) {
      const v = Math.floor((((row + 0.5) * HIT_CELL - rect.top) / rect.height) * map.height);
      if (v < 0 || v >= map.height) continue;
      for (let col = firstCol; col <= lastCol; col += 1) {
        const u = Math.floor((((col + 0.5) * HIT_CELL - rect.left) / rect.width) * map.width);
        if (u < 0 || u >= map.width) continue;
        if (sampleAlpha(map, u, v) > 40) setHitBit(bits, row * hitCols + col);
      }
    }
  }
  for (const target of interactiveRects()) markHitRect(bits, target);
  return bits;
}

function hitMaskSnapshot() {
  let active = 0;
  if (hitBits) {
    for (const byte of hitBits) {
      for (let bit = 0; bit < 8; bit += 1) if (byte & (1 << bit)) active += 1;
    }
  }
  return { cols: hitCols, rows: hitRows, active, frames: frameAlpha.size, error: hitError };
}

function pushHitMask() {
  if (!window.deepFish.setHitMask) return;
  let bits;
  try {
    bits = buildHitBits();
    hitError = "";
  } catch (error) {
    hitError = String(error?.message ?? error);
    return;
  }
  if (hitBits && hitBits.length === bits.length) {
    let unchanged = true;
    for (let index = 0; index < bits.length; index += 1) {
      if (bits[index] !== hitBits[index]) {
        unchanged = false;
        break;
      }
    }
    if (unchanged) return;
  }
  hitBits = bits;
  window.deepFish.setHitMask({ cols: hitCols, rows: hitRows, cell: HIT_CELL, bits: Array.from(bits) });
}

function stopFramePlayback(reset = true) {
  clearTimeout(frameTimer);
  clearTimeout(frameLoopTimer);
  frameTimer = undefined;
  frameLoopTimer = undefined;
  if (reset) setFrame(restingFrame());
}

function playFrame(name, duration) {
  stopFramePlayback(false);
  setFrame(name);
  frameTimer = setTimeout(() => {
    frameTimer = undefined;
    setFrame(restingFrame());
  }, duration);
}

function startFrameLoop(names, interval = 220) {
  stopFramePlayback(false);
  let index = 0;
  const durations = Array.isArray(interval) ? interval : names.map(() => interval);
  const advance = () => {
    setFrame(names[index]);
    const duration = durations[index % durations.length] || 220;
    index = (index + 1) % names.length;
    frameLoopTimer = setTimeout(advance, duration);
  };
  advance();
}

function playBehaviorFrames(name, duration) {
  const sequence = frameSequences[name];
  if (!sequence) {
    playFrame(name, duration);
    return;
  }
  startFrameLoop(sequence.names, sequence.interval);
  frameTimer = setTimeout(() => stopFramePlayback(), duration);
}

const frameImages = new Map();
for (const name of frameNames) {
  const image = new Image();
  image.src = framePath(name);
  frameImages.set(name, image);
}

function removePrefixedClass(prefix) {
  for (const name of [...appRoot.classList]) {
    if (name.startsWith(prefix)) appRoot.classList.remove(name);
  }
}

function say(text, label, duration = 4200) {
  // The chat bubble already lives directly above the pet, so speaking there
  // would overlap it; a notification card is more important than small talk.
  if (!chatPanel.hidden || speech.classList.contains("notice")) return;
  phaseLabel.textContent = label || window.DeepFishClock.getPhase().label;
  speechText.textContent = behaviorApi.stripHints(text);
  speech.classList.add("visible");
  scheduleHitMask();
  clearTimeout(speechTimer);
  speechTimer = setTimeout(() => {
    speech.classList.remove("visible");
    scheduleHitMask();
  }, duration);
}

const NOTICE_TITLES = { approval: "🔐 需要审批", done: "✅ 任务完成", error: "⚠ 任务失败" };

/// A background-task notification. Visually distinct from the chat bubbles and
/// the idle speech bubble: dark card, no tail, clickable, one line of text.
function notify(kind, message, options = {}) {
  clearTimeout(speechTimer);
  clearTimeout(noticeTimer);
  speech.classList.remove("visible", "notice", "notice-approval", "notice-done", "notice-error");
  speech.classList.add("notice", `notice-${kind}`);
  phaseLabel.textContent = NOTICE_TITLES[kind] || "";
  speechText.textContent = message;
  speech.dataset.sessionId = options.sessionId || "";
  speech.title = "点击查看会话";
  void speech.offsetWidth;
  speech.classList.add("visible");
  scheduleHitMask();
  const duration = options.duration ?? (kind === "approval" ? 0 : 7000);
  if (duration > 0) noticeTimer = setTimeout(hideNotice, duration);
}

function hideNotice() {
  if (!speech.classList.contains("notice")) return;
  clearTimeout(noticeTimer);
  noticeTimer = undefined;
  speech.classList.remove("visible", "notice", "notice-approval", "notice-done", "notice-error");
  speech.removeAttribute("title");
  scheduleHitMask();
}

function setScene(scene) {
  removePrefixedClass("scene-");
  if (scene) appRoot.classList.add(`scene-${scene}`);
}

function setExpression(expression) {
  removePrefixedClass("expression-");
  if (expression) appRoot.classList.add(`expression-${expression}`);
}

function animate(action, duration = 1200) {
  removePrefixedClass("action-");
  void appRoot.offsetWidth;
  appRoot.classList.add(`action-${action}`);
  clearTimeout(actionTimer);
  actionTimer = setTimeout(() => appRoot.classList.remove(`action-${action}`), duration);
  trackHitMaskWhileAnimating(duration);
}

function particles(icon, count = 5) {
  for (let i = 0; i < count; i += 1) {
    const node = document.createElement("span");
    node.className = "particle";
    node.textContent = icon;
    node.style.setProperty("--drift", `${-80 + Math.random() * 160}px`);
    node.style.setProperty("--turn", `${-80 + Math.random() * 160}deg`);
    node.style.left = `${38 + Math.random() * 24}%`;
    node.style.animationDelay = `${i * 60}ms`;
    effectLayer.append(node);
    setTimeout(() => node.remove(), 1400);
  }
}

function dizzyStars(duration) {
  const ring = document.createElement("div");
  ring.className = "dizzy-stars";
  for (let index = 0; index < 5; index += 1) {
    const star = document.createElement("span");
    star.textContent = index % 2 ? "★" : "✦";
    star.style.setProperty("--star-delay", `${index * -180}ms`);
    ring.append(star);
  }
  effectLayer.append(ring);
  setTimeout(() => ring.remove(), duration);
}

function stopWalk(clearScene = true) {
  clearInterval(walkTimer);
  clearTimeout(walkStopTimer);
  walkTimer = undefined;
  walkStopTimer = undefined;
  appRoot.classList.remove("is-walking");
  if (frameLoopTimer) stopFramePlayback();
  if (clearScene && appRoot.classList.contains("scene-walk")) setScene(null);
}

function startWalk(duration = 7200, announce = true) {
  stopWalk(false);
  setScene("walk");
  setExpression("happy");
  animate("walk", duration);
  appRoot.classList.add("is-walking");
  // Uneven timing gives the two full-body walk frames a small weight shift.
  startFrameLoop(["walk", "walk-b", "walk-b", "walk"], [165, 235, 305, 190]);
  if (announce) say(behaviorApi.formatLine("walk"), behaviorLabels.walk, 3400);
  walkTimer = setInterval(async () => {
    if (walkPending || drag || !chatPanel.hidden) return;
    walkPending = true;
    try {
      const result = await window.deepFish.walkStep({ x: walkDirection * 3, y: 0 });
      if (result?.boundary) walkDirection *= -1;
    } finally {
      walkPending = false;
    }
  }, 48);
  walkStopTimer = setTimeout(() => {
    stopWalk();
    setExpression(null);
  }, duration);
}

function runBehavior(id, options = {}) {
  const behavior = behaviorApi.getBehavior(id);
  if (id === "walk") {
    if (currentSettings.freeWalk !== false) startWalk(behavior.duration, options.say !== false);
    return;
  }
  stopWalk(false);
  setScene(behavior.scene);
  setExpression(behavior.expression);
  animate(id, behavior.duration);
  playBehaviorFrames(id, behavior.duration);
  if (id === "dizzy") dizzyStars(behavior.duration);
  particles(behavior.icon, ["cry", "shock", "rival"].includes(id) ? 6 : 3);
  if (options.say !== false) say(behaviorApi.formatLine(id), options.label || behaviorLabels[id], Math.min(5200, behavior.duration + 1800));
  clearTimeout(sceneTimer);
  sceneTimer = setTimeout(() => {
    setScene(null);
    setExpression(null);
  }, behavior.duration);
}

function applyClock() {
  const phase = window.DeepFishClock.getPhase();
  removePrefixedClass("phase-");
  appRoot.classList.add(`phase-${phase.id}`);
  document.title = `大肥鱼 · ${phase.label}`;
  if (phase.id === "sleep" && !walkTimer && chatPanel.hidden) {
    setExpression("sleep");
    setScene("sleep");
  }
  if (!frameTimer && !frameLoopTimer && !drag && chatPanel.hidden) setFrame(restingFrame());
}

function nextIdleBehavior() {
  const phase = window.DeepFishClock.getPhase();
  let pool = behaviorApi.getPlan(phase.id);
  if (phase.id !== "sleep" && Math.random() < 0.38) pool = pool.concat(signaturePool);
  if (currentSettings.freeWalk === false) pool = pool.filter((id) => id !== "walk");
  if (currentSettings.spicyLines === false) pool = pool.filter((id) => !["angry", "rival", "goAway"].includes(id));
  return randomOf(pool.length ? pool : ["think"]);
}

function scheduleIdle() {
  clearTimeout(idleTimer);
  const phase = window.DeepFishClock.getPhase();
  const intensity = Math.min(1.8, Math.max(.5, Number(currentSettings.behaviorIntensity) || 1));
  const delay = (15 + Math.random() * 25) * 1000 / Math.max(phase.energy * intensity, .3);
  idleTimer = setTimeout(() => {
    if (chatPanel.hidden && !drag) runBehavior(nextIdleBehavior());
    scheduleIdle();
  }, delay);
}

function resumeWalkAfterInteraction(id) {
  const behavior = behaviorApi.getBehavior(id);
  setTimeout(() => {
    if (currentSettings.freeWalk !== false && chatPanel.hidden && !drag) startWalk(4200, false);
  }, behavior.duration + 280);
}

function bodyZone(event) {
  const rect = stage.getBoundingClientRect();
  const y = (event.clientY - rect.top) / rect.height;
  if (y < .38) return "hair";
  if (y < .64) return "face";
  if (y > .76) return "legs";
  return "body";
}

function reactToWalkHover(event) {
  if (!appRoot.classList.contains("is-walking") || currentSettings.interactiveZones === false) return;
  const zone = bodyZone(event);
  const now = Date.now();
  if (zone === lastZone || now - lastHoverAt < 1800) return;
  lastZone = zone;
  lastHoverAt = now;
  if (zone === "hair") {
    runBehavior(Math.random() > .5 ? "ciallo" : "smug");
    resumeWalkAfterInteraction("ciallo");
  } else if (zone === "face") {
    runBehavior("shy");
    resumeWalkAfterInteraction("shy");
  } else if (zone === "legs") {
    tripCount += 1;
    if (tripCount >= 3) {
      runBehavior("cry");
      tripCount = 0;
    } else {
      runBehavior("trip");
      resumeWalkAfterInteraction("trip");
    }
  }
}

async function openChat() {
  if (!chatPanel.hidden) return;
  stopWalk();
  chatPanel.hidden = false;
  stopFramePlayback();
  hideNotice();
  speech.classList.remove("visible");
  scheduleHitMask();
  chatInput.focus();
  // Re-read the session so replies that arrived while the panel was closed show up.
  if (!running) await loadSession(activeSession?.id);
}

function closeChat() {
  // Runs keep going in the background; only the panel is hidden.
  chatPanel.hidden = true;
  hideSessionList();
  stopFramePlayback();
  scheduleHitMask();
  stage.focus();
  scheduleIdle();
}

function scrollChat() {
  if (scrollPending) return;
  scrollPending = true;
  requestAnimationFrame(() => {
    scrollPending = false;
    chatLog.scrollTop = chatLog.scrollHeight;
  });
}

function messageNode(message) {
  const node = document.createElement("p");
  if (message.role === "tool") {
    node.className = "tool-chip";
    node.textContent = message.display || message.name || "工具";
    return node;
  }
  if (!message.content) return null;
  node.className = `message ${message.role}`;
  node.textContent = message.content;
  return node;
}

function appendNode(node) {
  if (!node) return null;
  chatLog.append(node);
  scrollChat();
  return node;
}

function appendNote(text, extraClass = "") {
  return appendNode(noteNode(text, extraClass));
}

function noteNode(text, extraClass = "") {
  const node = document.createElement("p");
  node.className = `note ${extraClass}`.trim();
  node.textContent = text;
  if (extraClass === "summary") {
    node.title = "点击查看摘要";
    node.addEventListener("click", () => {
      summaryExpanded = !summaryExpanded;
      renderSession(activeSession);
    });
  }
  return node;
}

function approvalNode(event) {
  const node = document.createElement("div");
  node.className = `approval risk-${event.risk || "approval"}`;
  node.dataset.callId = event.callId;

  const text = document.createElement("p");
  text.className = "approval-text";
  text.textContent = event.summary || event.display || event.tool;

  const actions = document.createElement("div");
  actions.className = "approval-actions";
  const allow = document.createElement("button");
  allow.type = "button";
  allow.className = "approve";
  allow.textContent = "允许";
  const deny = document.createElement("button");
  deny.type = "button";
  deny.className = "deny";
  deny.textContent = "拒绝";
  actions.append(allow, deny);
  node.append(text, actions);

  let settled = false;
  let expiry = 0;
  const finish = (allowed) => {
    if (settled) return;
    settled = true;
    clearTimeout(expiry);
    pendingApprovals.delete(event.callId);
    allow.disabled = true;
    deny.disabled = true;
    actions.remove();
    const verdict = document.createElement("span");
    verdict.className = allowed ? "approval-done" : "approval-denied";
    verdict.textContent = allowed ? "已允许" : "已拒绝";
    node.append(verdict);
  };
  allow.addEventListener("click", () => {
    finish(true);
    agentApi.approve(event.callId, true).catch(() => {});
  });
  deny.addEventListener("click", () => {
    finish(false);
    agentApi.approve(event.callId, false).catch(() => {});
  });

  if (event.timeout) {
    const timer = document.createElement("span");
    timer.className = "approval-hint";
    timer.textContent = `${Math.round(event.timeout)}s 后自动拒绝`;
    node.append(timer);
    expiry = setTimeout(() => {
      if (settled) return;
      finish(false);
      agentApi.approve(event.callId, false).catch(() => {});
    }, Math.round(event.timeout) * 1000);
  }
  return node;
}

function revealApproval(event) {
  if (event.risk === "blocked") {
    pendingApprovals.delete(event.callId);
    if (!chatPanel.hidden && activeSession && activeSession.id === event.sessionId) {
      appendNote(`⛔ ${event.summary}`, "warn");
    }
    return;
  }
  const watching = !chatPanel.hidden && activeSession && activeSession.id === event.sessionId;
  if (!watching) {
    // Keep the panel closed: the pet just raises its head and asks. Clicking the
    // notice opens the chat with the approval card already on screen.
    notify("approval", "主人，有一个操作需要审批", {
      sessionId: event.sessionId,
      duration: ((event.timeout ?? 300) + 1) * 1000
    });
    return;
  }
  showApproval(event);
}

function settleApproval(event) {
  pendingApprovals.delete(event.callId);
  if (speech.classList.contains("notice-approval") && !pendingApprovals.size) hideNotice();
  const card = chatLog.querySelector(`.approval[data-call-id="${event.callId}"]`);
  if (!card || event.decision === "done") return;
  card.querySelector(".approval-actions")?.remove();
  if (card.querySelector(".approval-denied") || card.querySelector(".approval-done")) return;
  const verdict = document.createElement("span");
  verdict.className = "approval-denied";
  verdict.textContent = event.decision === "blocked" ? "已阻止" : "已拒绝";
  card.append(verdict);
}

function showApproval(event) {
  pendingApprovals.set(event.callId, event);
  chatLog.querySelector(`.approval[data-call-id="${event.callId}"]`)?.remove();
  appendNode(approvalNode(event));
}

function renderSession(session) {
  if (!session) return;
  activeSession = session;
  pendingAssistant = null;
  chatLog.textContent = "";
  sessionTitle.textContent = session.title;
  document.title = `大肥鱼 · ${session.title}`;
  if (session.summary) {
    const count = session.compacted || 0;
    appendNode(noteNode(`📦 更早的 ${count} 条对话已压缩为摘要${summaryExpanded ? "（点击收起）" : "（点击展开）"}`, "summary"));
    if (summaryExpanded) appendNode(noteNode(session.summary));
  }
  for (const message of session.messages) {
    const node = messageNode(message);
    if (node) chatLog.append(node);
  }
  if (!chatLog.childElementCount) chatLog.append(noteNode("新对话已就绪，说点什么吧。", "hint"));
  for (const pending of pendingApprovals.values()) {
    if (pending.sessionId === session.id) chatLog.append(approvalNode(pending));
  }
  scrollChat();
}

function renderSessionList() {
  sessionItems.textContent = "";
  if (!sessionSummaries.length) {
    sessionItems.append(noteNode("还没有会话"));
    return;
  }
  for (const summary of sessionSummaries) {
    const item = document.createElement("div");
    item.className = "session-item" + (activeSession && summary.id === activeSession.id ? " active" : "");

    const open = document.createElement("button");
    open.type = "button";
    open.className = "title";
    open.textContent = summary.title;
    open.title = summary.preview || summary.title;
    open.addEventListener("click", () => selectSession(summary.id));

    const time = document.createElement("span");
    time.className = "time";
    time.textContent = relativeTime(summary.updatedAt);

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "remove" + (armedRemoval === summary.id ? " arming" : "");
    remove.textContent = armedRemoval === summary.id ? "删除?" : "×";
    remove.title = "删除会话";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      armRemoval(summary.id);
    });

    item.append(open, time, remove);
    sessionItems.append(item);
  }
}

function relativeTime(timestamp) {
  const minutes = Math.max(0, Math.round((Date.now() - Number(timestamp || 0)) / 60000));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.round(hours / 24)} 天前`;
}

function armRemoval(id) {
  if (armedRemoval === id) {
    armedRemoval = "";
    clearTimeout(armTimer);
    void removeSession(id);
    return;
  }
  armedRemoval = id;
  renderSessionList();
  clearTimeout(armTimer);
  armTimer = setTimeout(() => {
    armedRemoval = "";
    renderSessionList();
  }, 2500);
}

async function removeSession(id) {
  try {
    for (const [callId, pending] of pendingApprovals) {
      if (pending.sessionId === id) pendingApprovals.delete(callId);
    }
    sessionSummaries = await agentApi.deleteSession(id);
    if (activeSession && activeSession.id === id) {
      activeSession = null;
      await loadSession();
      return;
    }
    renderSessionList();
  } catch (error) {
    appendNote(`删除失败：${error.message}`, "warn");
  }
}

function toggleSessionList() {
  const show = sessionList.hidden;
  sessionList.hidden = !show;
  sessionButton.setAttribute("aria-expanded", String(show));
  if (show) renderSessionList();
}

function hideSessionList() {
  sessionList.hidden = true;
  sessionButton.setAttribute("aria-expanded", "false");
}

async function refreshSessions() {
  try {
    sessionSummaries = await agentApi.listSessions();
  } catch {
    sessionSummaries = [];
    if (!sessionList.hidden) {
      sessionItems.textContent = "";
      sessionItems.append(noteNode("会话列表加载失败，稍后再试。"));
    }
    return;
  }
  if (!sessionList.hidden) renderSessionList();
}

async function loadSession(sessionId) {
  try {
    const session = await agentApi.openSession(sessionId);
    summaryExpanded = false;
    renderSession(session);
    await refreshSessions();
  } catch (error) {
    chatLog.textContent = "";
    appendNote(`会话加载失败：${error.message}`, "warn");
  }
}

async function selectSession(id) {
  hideSessionList();
  if (activeSession && activeSession.id === id) return;
  await loadSession(id);
}

async function createSession() {
  hideSessionList();
  try {
    const session = await agentApi.newSession();
    summaryExpanded = false;
    renderSession(session);
    await refreshSessions();
    chatInput.focus();
  } catch (error) {
    appendNote(`新建会话失败：${error.message}`, "warn");
  }
}

function setRunning(value) {
  running = value;
  sessionRunning.hidden = !value;
  chatInput.placeholder = value ? "正在处理…" : "问大肥鱼...";
}

function handleAgentEvent(event) {
  if (!event) return;
  // Approval cards matter even when another session (or none) is on screen:
  // the run is parked until the user answers.
  if (event.type === "approval") {
    pendingApprovals.set(event.callId, event);
    void revealApproval(event);
    return;
  }
  if (event.type === "run-end") {
    // A run that ended can no longer act on an approval card, so forget any of
    // its leftovers (a dropped connection would otherwise leak them forever).
    for (const [callId, pending] of pendingApprovals) {
      if (pending.sessionId === event.sessionId) pendingApprovals.delete(callId);
    }
    // The renderer is the only place that knows whether the user is watching,
    // so background runs announce themselves here instead of in Rust.
    if (event.outcome !== "cancelled") {
      const watching =
        !chatPanel.hidden && activeSession && activeSession.id === event.sessionId;
      if (!watching) {
        notify(event.outcome === "ok" ? "done" : "error", event.outcome === "ok" ? "主人，任务完成了" : "主人，任务失败了", {
          sessionId: event.sessionId
        });
      }
    }
    void refreshSessions();
    return;
  }
  if (event.type === "approval-result") {
    settleApproval(event);
    return;
  }
  if (!activeSession || event.sessionId !== activeSession.id) {
    if (event.type === "message") void refreshSessions();
    return;
  }
  switch (event.type) {
    case "message": {
      const message = event.message;
      if (message.role === "assistant" && pendingAssistant) {
        pendingAssistant.classList.remove("streaming");
        if (message.content) pendingAssistant.textContent = message.content;
        pendingAssistant = null;
        scrollChat();
      } else {
        const node = messageNode(message);
        if (node) appendNode(node);
      }
      activeSession.messages.push(message);
      if (message.role === "user") void refreshSessions();
      break;
    }
    case "delta": {
      if (!pendingAssistant) {
        // `messageNode` drops empty messages, so the streaming placeholder is
        // built here instead of going through it.
        pendingAssistant = document.createElement("p");
        pendingAssistant.className = "message assistant streaming";
        appendNode(pendingAssistant);
      }
      pendingAssistant.textContent += event.text ?? "";
      scrollChat();
      break;
    }
    case "status":
      if (event.state === "idle") {
        setRunning(false);
        if (pendingAssistant) {
          pendingAssistant.classList.remove("streaming");
          pendingAssistant = null;
        }
      } else {
        setRunning(true);
      }
      break;
    case "error":
      setRunning(false);
      appendNote(event.message, "warn");
      break;
    case "note":
      appendNote(event.text, event.level === "warn" ? "warn" : "");
      break;
    case "renamed":
      if (activeSession) {
        activeSession.title = event.title;
        sessionTitle.textContent = event.title;
      }
      void refreshSessions();
      break;
    case "compacted":
      if (activeSession) {
        activeSession.compacted = event.compacted;
        activeSession.summary = event.summary;
        summaryExpanded = false;
        renderSession(activeSession);
      }
      break;
    default:
      break;
  }
}

async function sendChat(event) {
  event.preventDefault();
  const content = chatInput.value.trim();
  if (!content) return;
  if (!activeSession) await loadSession();
  if (!activeSession) return;

  chatInput.value = "";
  hideSessionList();
  runBehavior("think", { say: false });
  pendingAssistant = null;
  setRunning(true);
  requestSequence += 1;
  try {
    await agentApi.send(activeSession.id, content, requestSequence);
  } catch (error) {
    setRunning(false);
    appendNote(`没连上模型：${error.message}`, "warn");
  }
}

stage.addEventListener("pointerenter", () => {
  const phase = window.DeepFishClock.getPhase();
  if (!appRoot.classList.contains("is-walking")) say(randomOf([phase.line, "我看见你啦。", "好的，用户又发癫了。"]), phase.label, 2200);
});

stage.addEventListener("pointerleave", () => { lastZone = ""; });

stage.addEventListener("pointerdown", (event) => {
  if (event.button !== 0 || event.target === chatFab) return;
  // Without this the webview starts its own text/image drag when the window
  // stops moving under the cursor (screen edge, screen switch), which leaves a
  // blue selection box around the character and kills the drag gesture.
  event.preventDefault();
  stage.focus({ preventScroll: true });
  stopWalk();
  stopFramePlayback();
  window.deepFish.pinClickThrough(true);
  stage.setPointerCapture(event.pointerId);
  drag = {
    x: event.screenX,
    y: event.screenY,
    total: 0,
    spin: gestureApi.createSpinTracker(event.screenX, event.screenY, event.timeStamp)
  };
});

stage.addEventListener("pointermove", (event) => {
  if (!drag) {
    reactToWalkHover(event);
    return;
  }
  const dx = event.screenX - drag.x;
  const dy = event.screenY - drag.y;
  if (dx || dy) {
    gestureApi.trackSpin(drag.spin, event.screenX, event.screenY, event.timeStamp);
    window.deepFish.move({ x: dx, y: dy });
    drag.x = event.screenX;
    drag.y = event.screenY;
    drag.total += Math.abs(dx) + Math.abs(dy);
  }
});

stage.addEventListener("pointerup", (event) => {
  if (event.button !== 0 || event.target === chatFab) return;
  if (!drag) return;
  const completedDrag = drag;
  const moved = completedDrag?.total || 0;
  drag = undefined;
  window.deepFish.pinClickThrough(false);
  if (completedDrag?.spin?.dizzy) {
    runBehavior("dizzy");
    return;
  }
  if (moved > 10) {
    runBehavior("startle", { label: "拖拽反馈" });
    return;
  }
  runBehavior("pat", { say: false });
  say(randomOf(clickLines), "点击反馈", 2400);
});

speech.addEventListener("click", async () => {
  if (!speech.classList.contains("notice")) return;
  const sessionId = speech.dataset.sessionId;
  hideNotice();
  await openChat();
  if (sessionId && (!activeSession || activeSession.id !== sessionId)) {
    await loadSession(sessionId);
  }
});

chatFab.addEventListener("click", (event) => {
  event.stopPropagation();
  openChat();
});

for (const name of ["pointerenter", "pointerleave"]) {
  appRoot.addEventListener(name, () => scheduleHitMask());
}

stage.addEventListener("pointercancel", () => {
  drag = undefined;
  window.deepFish.pinClickThrough(false);
});

stage.addEventListener("lostpointercapture", () => {
  if (!drag) window.deepFish.pinClickThrough(false);
});

stage.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    openChat();
  }
});

// Same reason as above, for drags that start on any other part of the pet.
document.addEventListener("dragstart", (event) => event.preventDefault());
document.addEventListener("selectstart", (event) => {
  if (!event.target.closest(".chat-log, .chat-input, .session-list")) event.preventDefault();
});

document.addEventListener("contextmenu", (event) => {
  event.preventDefault();
  say("干嘛？", "右键菜单", 2200);
  window.deepFish.showMenu();
});

chatClose.addEventListener("click", closeChat);
chatForm.addEventListener("submit", sendChat);
sessionButton.addEventListener("click", toggleSessionList);
sessionNew.addEventListener("click", createSession);
sessionCompact.addEventListener("click", async () => {
  if (!activeSession) return;
  hideSessionList();
  try {
    const changed = await agentApi.compact(activeSession.id);
    if (!changed) appendNote("当前上下文还很短，暂时不需要压缩");
  } catch (error) {
    appendNote(`压缩失败：${error.message}`, "warn");
  }
});
agentApi.onEvent(handleAgentEvent);
chatLog.addEventListener("pointerdown", hideSessionList);
chatForm.addEventListener("pointerdown", hideSessionList);
void refreshSessions();

window.deepFish.onWake(({ action, awayMs }) => {
  runBehavior(action);
  if (action === "startle" && awayMs > 3600000) say("诶？！你终于回来了！", "惊醒", 5000);
});

window.deepFish.onCommand((command) => {
  if (command === "open-chat") openChat();
  else runBehavior(command);
});

// The model can ask the pet to speak through the `pet_say` tool.
window.deepFish.onSpeak(({ text }) => {
  if (text) say(text, "大肥鱼", Math.min(6000, 1600 + text.length * 110));
});

function applySettings(settings) {
  currentSettings = { ...currentSettings, ...settings };
  // Only the pet scales, and the row below the bubbles follows its real height
  // so the bubbles stay attached to the head at every zoom level.
  const scale = Number(currentSettings.scale) || 1;
  stage.style.transform = `scale(${scale})`;
  appRoot.style.setProperty("--pet-height", `${Math.round(stage.offsetHeight * scale)}px`);
  providerLabel.textContent = currentSettings.provider === "deepseek" ? "DeepSeek" : currentSettings.provider === "custom" ? "自定义模型" : "免费模型";
  if (currentSettings.freeWalk === false) stopWalk();
  scheduleHitMask();
  scheduleIdle();
}

window.deepFish.onSettings(applySettings);
window.deepFish.getSettings().then(applySettings);
applyClock();
setInterval(applyClock, 60000);
scheduleIdle();
window.addEventListener("resize", scheduleHitMask);
// Read-only snapshot consumed by the packaged smoke test.
window.deepFishHitMask = hitMaskSnapshot;
scheduleHitMask();

function scheduleBlink() {
  clearTimeout(blinkTimer);
  const delay = 2800 + Math.random() * 4200;
  blinkTimer = setTimeout(() => {
    const busy = drag || !chatPanel.hidden || frameTimer || frameLoopTimer || appRoot.classList.contains("is-walking");
    if (!busy) {
      const previousFrame = character.dataset.frame || restingFrame();
      setFrame("blink");
      setTimeout(() => {
        if (character.dataset.frame === "blink") setFrame(previousFrame);
      }, 135);
    }
    scheduleBlink();
  }, delay);
}
scheduleBlink();
