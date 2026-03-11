const HOST_NAME = "com.yeet_and_yoink.chromium_bridge";
const RECONNECT_DELAY_MINUTES = 1 / 60;
const RECONNECT_ALARM = "yny-native-reconnect";

let nativePort = null;

function log(message, error) {
  if (error) {
    console.error(`[yny-chromium-bridge] ${message}`, error);
  } else {
    console.log(`[yny-chromium-bridge] ${message}`);
  }
}

function chromeCall(invoke) {
  return new Promise((resolve, reject) => {
    invoke((result) => {
      const lastError = chrome.runtime.lastError;
      if (lastError) {
        reject(new Error(lastError.message));
      } else {
        resolve(result);
      }
    });
  });
}

function getLastFocusedWindow(queryOptions) {
  return chromeCall((done) => chrome.windows.getLastFocused(queryOptions, done));
}

function getTab(tabId) {
  return chromeCall((done) => chrome.tabs.get(tabId, done));
}

function updateTab(tabId, properties) {
  return chromeCall((done) => chrome.tabs.update(tabId, properties, done));
}

function moveTab(tabId, properties) {
  return chromeCall((done) => chrome.tabs.move(tabId, properties, done));
}

function createWindow(properties) {
  return chromeCall((done) => chrome.windows.create(properties, done));
}

function clearReconnectAlarm() {
  return chromeCall((done) => chrome.alarms.clear(RECONNECT_ALARM, done)).catch(() => {});
}

function scheduleReconnect() {
  return chrome.alarms.create(RECONNECT_ALARM, {
    delayInMinutes: RECONNECT_DELAY_MINUTES
  });
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
  const browserWindow = await getLastFocusedWindow({
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
      await updateTab(targetTab.id, { active: true });
      return {};
    }
    case "move_tab": {
      const direction = normalizeDirection(message.direction);
      const targetIndex = moveTargetIndex(state, direction);
      if (targetIndex === null) {
        throw new Error(`cannot move the current tab ${direction}`);
      }
      await moveTab(state.activeTab.id, {
        windowId: state.window.id,
        index: targetIndex
      });
      await updateTab(state.activeTab.id, { active: true });
      return {};
    }
    case "tear_out":
      await createWindow({ tabId: state.activeTab.id });
      return {};
    case "merge_tab": {
      const direction = normalizeMergeDirection(message.direction);
      if (state.window.id === message.source_window_id) {
        throw new Error("source and target browser windows are identical");
      }
      const sourceTab = await getTab(message.source_tab_id);
      const targetIndex = mergeTargetIndex(state, sourceTab, direction);
      await moveTab(sourceTab.id, {
        windowId: state.window.id,
        index: targetIndex
      });
      await updateTab(sourceTab.id, { active: true });
      return {};
    }
    default:
      throw new Error(`unsupported browser bridge command: ${message.command}`);
  }
}

function sendResponse(payload) {
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
    sendResponse({
      id: message.id,
      ok: true,
      ...payload
    });
  } catch (error) {
    sendResponse({
      id: message.id,
      ok: false,
      error: error && error.message ? error.message : String(error)
    });
  }
}

async function connectNative() {
  if (nativePort) {
    return;
  }

  await clearReconnectAlarm();

  try {
    nativePort = chrome.runtime.connectNative(HOST_NAME);
  } catch (error) {
    log("failed to connect to native host", error);
    await scheduleReconnect();
    return;
  }

  nativePort.onMessage.addListener((message) => {
    void onNativeMessage(message);
  });
  nativePort.onDisconnect.addListener(() => {
    const lastError = chrome.runtime.lastError;
    if (lastError) {
      log(`native host disconnected: ${lastError.message}`);
    } else {
      log("native host disconnected");
    }
    nativePort = null;
    void scheduleReconnect();
  });
}

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === RECONNECT_ALARM) {
    void connectNative();
  }
});

chrome.runtime.onStartup.addListener(() => {
  void connectNative();
});

chrome.runtime.onInstalled.addListener(() => {
  void connectNative();
});

void connectNative();
