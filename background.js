const HOST_NAME = "com.yeet_and_yoink.firefox_bridge";
const RECONNECT_DELAY_MS = 1000;

let nativePort = null;
let reconnectTimer = null;

function log(message, error) {
  if (error) {
    console.error(`[yny-browser-bridge] ${message}`, error);
  } else {
    console.log(`[yny-browser-bridge] ${message}`);
  }
}

function normalizeDirection(rawDirection) {
  const direction = String(rawDirection || "").trim().toLowerCase();
  if (direction === "west" || direction === "left") {
    return "west";
  }
  if (direction === "east" || direction === "right") {
    return "east";
  }
  throw new Error(`unsupported browser tab direction: ${rawDirection}`);
}

function normalizeMergeDirection(rawDirection) {
  const direction = String(rawDirection || "").trim().toLowerCase();
  if (["west", "left"].includes(direction)) {
    return "west";
  }
  if (["east", "right"].includes(direction)) {
    return "east";
  }
  if (["north", "up", "above"].includes(direction)) {
    return "north";
  }
  if (["south", "down", "below"].includes(direction)) {
    return "south";
  }
  throw new Error(`unsupported browser merge direction: ${rawDirection}`);
}

async function focusedWindowState() {
  const browserWindow = await browser.windows.getLastFocused({
    populate: true,
    windowTypes: ["normal"]
  });
  if (!browserWindow || !Array.isArray(browserWindow.tabs) || browserWindow.tabs.length === 0) {
    throw new Error("no focused browser window with tabs is available");
  }

  const activeTab = browserWindow.tabs.find((tab) => tab.active);
  if (!activeTab) {
    throw new Error("focused browser window did not report an active tab");
  }

  const pinnedTabCount = browserWindow.tabs.filter((tab) => tab.pinned).length;
  return {
    window: browserWindow,
    tabs: browserWindow.tabs,
    activeTab,
    pinnedTabCount
  };
}

function focusTargetIndex(state, direction) {
  if (direction === "west") {
    return state.activeTab.index > 0 ? state.activeTab.index - 1 : null;
  }
  if (direction === "east") {
    return state.activeTab.index + 1 < state.tabs.length ? state.activeTab.index + 1 : null;
  }
  return null;
}

function moveTargetIndex(state, direction) {
  if (direction === "west") {
    if (state.activeTab.pinned) {
      return state.activeTab.index > 0 ? state.activeTab.index - 1 : null;
    }
    return state.activeTab.index > state.pinnedTabCount ? state.activeTab.index - 1 : null;
  }

  if (direction === "east") {
    const upperBound = state.activeTab.pinned ? state.pinnedTabCount : state.tabs.length;
    return state.activeTab.index + 1 < upperBound ? state.activeTab.index + 1 : null;
  }

  return null;
}

function tabStatePayload(state) {
  return {
    state: {
      windowId: state.window.id,
      activeTabId: state.activeTab.id,
      activeTabIndex: state.activeTab.index,
      tabCount: state.tabs.length,
      pinnedTabCount: state.pinnedTabCount,
      activeTabPinned: Boolean(state.activeTab.pinned)
    }
  };
}

function mergeTargetIndex(targetState, sourceTab, direction) {
  const appendToTrailingEdge = direction === "west" || direction === "north";
  if (sourceTab.pinned) {
    return appendToTrailingEdge ? targetState.pinnedTabCount : 0;
  }
  return appendToTrailingEdge ? targetState.tabs.length : targetState.pinnedTabCount;
}

async function handleCommand(message) {
  const state = await focusedWindowState();

  switch (message.command) {
    case "get_tab_state":
      return tabStatePayload(state);
    case "focus": {
      const direction = normalizeDirection(message.direction);
      const targetIndex = focusTargetIndex(state, direction);
      if (targetIndex === null) {
        throw new Error(`cannot focus ${direction} inside the current tab strip`);
      }
      const targetTab = state.tabs.find((tab) => tab.index === targetIndex);
      if (!targetTab) {
        throw new Error(`tab index ${targetIndex} does not exist`);
      }
      await browser.tabs.update(targetTab.id, { active: true });
      return {};
    }
    case "move_tab": {
      const direction = normalizeDirection(message.direction);
      const targetIndex = moveTargetIndex(state, direction);
      if (targetIndex === null) {
        throw new Error(`cannot move the current tab ${direction}`);
      }
      await browser.tabs.move(state.activeTab.id, {
        windowId: state.window.id,
        index: targetIndex
      });
      await browser.tabs.update(state.activeTab.id, { active: true });
      return {};
    }
    case "tear_out":
      await browser.windows.create({ tabId: state.activeTab.id });
      return {};
    case "merge_tab": {
      const direction = normalizeMergeDirection(message.direction);
      if (state.window.id === message.source_window_id) {
        throw new Error("source and target browser windows are identical");
      }
      const sourceTab = await browser.tabs.get(message.source_tab_id);
      const targetIndex = mergeTargetIndex(state, sourceTab, direction);
      await browser.tabs.move(sourceTab.id, {
        windowId: state.window.id,
        index: targetIndex
      });
      await browser.tabs.update(sourceTab.id, { active: true });
      return {};
    }
    default:
      throw new Error(`unsupported browser bridge command: ${message.command}`);
  }
}

async function sendResponse(payload) {
  if (!nativePort) {
    return;
  }
  try {
    nativePort.postMessage(payload);
  } catch (error) {
    log("failed to post response to native host", error);
  }
}

async function onNativeMessage(message) {
  try {
    const payload = await handleCommand(message);
    await sendResponse({
      id: message.id,
      ok: true,
      ...payload
    });
  } catch (error) {
    await sendResponse({
      id: message.id,
      ok: false,
      error: error && error.message ? error.message : String(error)
    });
  }
}

function scheduleReconnect() {
  if (reconnectTimer !== null) {
    return;
  }
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectNative();
  }, RECONNECT_DELAY_MS);
}

function connectNative() {
  if (nativePort) {
    return;
  }

  try {
    nativePort = browser.runtime.connectNative(HOST_NAME);
  } catch (error) {
    log("failed to connect to native host", error);
    scheduleReconnect();
    return;
  }

  nativePort.onMessage.addListener((message) => {
    void onNativeMessage(message);
  });
  nativePort.onDisconnect.addListener(() => {
    const lastError = browser.runtime.lastError;
    if (lastError) {
      log(`native host disconnected: ${lastError.message}`);
    } else {
      log("native host disconnected");
    }
    nativePort = null;
    scheduleReconnect();
  });
}

browser.runtime.onStartup.addListener(connectNative);
browser.runtime.onInstalled.addListener(connectNative);
connectNative();
