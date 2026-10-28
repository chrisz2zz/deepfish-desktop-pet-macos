(function exposeDesktopBridge(root) {
  const tauri = root.__TAURI__;
  if (!tauri || root.deepFish) return;

  const invoke = tauri.core.invoke;
  const listen = tauri.event.listen;

  function subscribe(event, callback) {
    let unlisten;
    let disposed = false;
    listen(event, ({ payload }) => callback(payload)).then((release) => {
      if (disposed) release();
      else unlisten = release;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }

  const round = (value) => Math.round(Number(value) || 0);

  // Tauri rejects with the raw Rust error value, but the renderer expects Error objects.
  const call = (command, args) =>
    invoke(command, args).catch((error) => {
      throw error instanceof Error
        ? error
        : new Error(typeof error === "string" ? error : JSON.stringify(error));
    });

  root.deepFish = {
    // Returns the resulting position so the renderer knows whether the bubble
    // band ended up above the screen edge.
    move: (delta, topRoom) =>
      call("move_pet", { dx: round(delta?.x), dy: round(delta?.y), topRoom: topRoom ?? null }),
    /// Pulls the window back on screen before the chat panel needs its space.
    ensureVisible: () => call("ensure_pet_visible"),
    walkStep: (delta) => call("walk_pet", { dx: round(delta?.x), dy: round(delta?.y) }),
    showMenu: () => {
      call("show_pet_menu").catch(() => {});
    },
    openSettings: () => {
      call("open_settings").catch(() => {});
    },
    getSettings: () => call("get_settings"),
    saveSettings: (settings) => call("save_settings", { settings }),
    // Agent harness: sessions, context compaction and the tool loop live in Rust.
    agent: {
      listSessions: () => call("agent_sessions"),
      openSession: (sessionId) => call("agent_open_session", { sessionId: sessionId ?? null }),
      newSession: (title) => call("agent_new_session", { title: title ?? null }),
      renameSession: (sessionId, title) =>
        call("agent_rename_session", { sessionId, title }),
      deleteSession: (sessionId) => call("agent_delete_session", { sessionId }),
      send: (sessionId, text, requestId) =>
        call("agent_send", { sessionId, requestId, text }),
      cancel: (sessionId) => call("agent_cancel", { sessionId }),
      approve: (callId, allow) => call("agent_approval", { callId, allow }),
      compact: (sessionId) => call("agent_compact", { sessionId }),
      onEvent: (callback) => subscribe("agent-event", callback)
    },
    setHitMask: (mask) => {
      call("set_hit_mask", {
        cols: mask.cols,
        rows: mask.rows,
        cell: mask.cell,
        bits: mask.bits
      }).catch(() => {});
    },
    pinClickThrough: (pinned) => {
      call("pin_click_through", { pinned }).catch(() => {});
    },
    onSpeak: (callback) => subscribe("pet-speak", callback),
    onWake: (callback) => subscribe("wake-event", callback),
    onCommand: (callback) => subscribe("pet-command", callback),
    onSettings: (callback) => subscribe("settings-updated", callback)
  };
})(window);
