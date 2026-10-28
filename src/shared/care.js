(function exposeCare(root, factory) {
  const api = factory();
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  if (root) root.DeepFishCare = api;
})(typeof window !== "undefined" ? window : globalThis, function createCare() {
  // Pure scheduling logic for the pet's gentle nudges. Keeping it free of DOM
  // and timers makes the quiet-hours and interval rules easy to test.
  const MINUTE = 60 * 1000;
  const MAX_HISTORY = 6 * 60 * MINUTE;
  const QUIET_START = 23;
  const QUIET_END = 8;

  const waterLines = [
    "该喝水啦。杯子就在手边，先喝两口再继续。",
    "主人，补水时间到，顺便活动一下脖子。",
    "蓝色大肥鱼监督你喝水，喝完再摸鱼。",
    "来，喝口水。你认真工作的样子我都有看到。"
  ];

  const moveLines = [
    "坐很久了，起来走两步吧，我帮你看着屏幕。",
    "伸展一下肩膀和腰，回来会更清醒。",
    "起来动一动，顺便看看窗外。",
    "该活动啦，久坐对腰不好，我可不想你难受。"
  ];

  function intervalMinutes(value, fallback, maximum) {
    const number = Number(value);
    if (!Number.isFinite(number) || number <= 0) return fallback;
    return Math.min(maximum, Math.max(10, Math.round(number)));
  }

  function options(settings = {}) {
    return {
      waterMs: intervalMinutes(settings.waterEveryMin, 50, 180) * MINUTE,
      moveMs: intervalMinutes(settings.moveEveryMin, 80, 240) * MINUTE
    };
  }

  function fresh(now = Date.now()) {
    return { waterAt: now, moveAt: now };
  }

  /// Drops garbage and old timestamps: opening the app the next morning should
  /// start a new cycle instead of firing a pile of overdue reminders at once.
  function normalize(state, now = Date.now()) {
    const clean = (value) => {
      const stamp = Number(value);
      if (!Number.isFinite(stamp) || stamp > now || now - stamp > MAX_HISTORY) return now;
      return stamp;
    };
    if (!state || typeof state !== "object") return fresh(now);
    return { waterAt: clean(state.waterAt), moveAt: clean(state.moveAt) };
  }

  function quiet(date) {
    const hour = date.getHours();
    return hour >= QUIET_START || hour < QUIET_END;
  }

  /// Which reminder is due, if any. The more overdue one wins when both are.
  function dueType(state, now = Date.now(), settings = {}) {
    if (quiet(new Date(now))) return null;
    const { waterMs, moveMs } = options(settings);
    const waterOverdue = (now - state.waterAt) / waterMs;
    const moveOverdue = (now - state.moveAt) / moveMs;
    const waterDue = waterOverdue >= 1;
    const moveDue = moveOverdue >= 1;
    if (waterDue && moveDue) return waterOverdue >= moveOverdue ? "water" : "move";
    if (waterDue) return "water";
    if (moveDue) return "move";
    return null;
  }

  /// Marks one reminder as sent. The other one is postponed when it was also
  /// overdue so two bubbles never arrive back to back.
  function markSent(state, type, now = Date.now(), settings = {}) {
    const { waterMs, moveMs } = options(settings);
    if (type === "move") {
      state.moveAt = now;
      if (now - state.waterAt >= waterMs) state.waterAt = now;
    } else {
      state.waterAt = now;
      if (now - state.moveAt >= moveMs) state.moveAt = now;
    }
    return state;
  }

  function line(type, random = Math.random) {
    const pool = type === "move" ? moveLines : waterLines;
    return pool[Math.floor(random() * pool.length) % pool.length];
  }

  return {
    MINUTE,
    waterLines,
    moveLines,
    options,
    fresh,
    normalize,
    quiet,
    dueType,
    markSent,
    line
  };
});
