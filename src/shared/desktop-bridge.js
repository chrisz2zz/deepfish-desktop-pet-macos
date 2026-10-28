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
    move: (delta) => {
      call("move_pet", { dx: round(delta?.x), dy: round(delta?.y) }).catch(() => {});
    },
    walkStep: (delta) => call("walk_pet", { dx: round(delta?.x), dy: round(delta?.y) }),
    showMenu: () => {
      call("show_pet_menu").catch(() => {});
    },
    openSettings: () => {
      call("open_settings").catch(() => {});
    },
    getSettings: () => call("get_settings"),
    saveSettings: (settings) => call("save_settings", { settings }),
    chat: (messages) => call("chat_send", { messages }),
    onWake: (callback) => subscribe("wake-event", callback),
    onCommand: (callback) => subscribe("pet-command", callback),
    onSettings: (callback) => subscribe("settings-updated", callback)
  };
})(window);
