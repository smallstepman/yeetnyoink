import React from "react";
import { palette } from "../shared/theme.js";
import { CompassPad, FlowArrow, OptionRail, PaneBlock, TabStrip, Tag, WindowFrame } from "../shared/primitives.jsx";
import { phaseIndex, springProgress } from "../shared/utils.js";
import {
  renderAllowedDirections,
  renderAnchorWindow,
  renderIntegrationToggle,
  renderInternalEnabled,
  renderSnapBack,
  renderTearOffStrategy,
  renderTearOffToggle,
} from "./shared.jsx";

const renderTerminalMuxBackend = (_field, frame) => {
  const options = ["tmux", "zellij", "wezterm", "kitty"];
  const index = phaseIndex(frame, 28, options.length);
  return (
    <>
      <WindowFrame x={82} y={88} width={462} height={278} title="terminal host" accent={palette.panel}>
        <PaneBlock x={34} y={42} width={176} height={174} fill={palette.accent} label="focused pane" active />
        <PaneBlock x={226} y={42} width={198} height={78} fill={palette.panel} label="pane" />
        <PaneBlock x={226} y={138} width={198} height={78} fill={palette.canvasMuted} label="pane" />
      </WindowFrame>
      <OptionRail x={590} y={86} title="mux_backend" options={options} activeIndex={index} />
      <div style={{ position: "absolute", left: 594, top: 296, display: "grid", gap: 10 }}>
        <Tag text={`backend: ${options[index]}`} tone="mint" />
        <Tag text={index === 0 ? "common default for foot/alacritty/ghostty/iTerm" : index === 2 ? "default for wezterm" : index === 3 ? "default for kitty" : "explicit override"} tone="accent" />
      </div>
    </>
  );
};

const renderTerminalHostTabs = (_field, frame, fps) => {
  const options = ["transparent", "focus", "native_full"];
  const index = phaseIndex(frame, 38, options.length);
  const progress = springProgress(frame, fps, 16);
  return (
    <>
      <WindowFrame x={82} y={88} width={522} height={278} title="terminal host" accent={palette.panel}>
        <TabStrip x={26} y={20} labels={["tab a", "tab b", "tab c"]} activeIndex={1} accent={palette.accent} />
        <PaneBlock x={36} y={102} width={180} height={118} fill={palette.accent} label="focused pane" active />
        <PaneBlock x={236} y={102} width={240} height={118} fill={palette.canvasMuted} label="pane grid" />
      </WindowFrame>
      <FlowArrow x={438} y={156} length={94} progress={index >= 1 ? progress : 0.2} color={index >= 1 ? palette.mint : palette.warm} />
      <FlowArrow x={438} y={276} length={94} progress={index === 2 ? progress : 0.2} color={index === 2 ? palette.accent : palette.warm} />
      <OptionRail x={638} y={92} title="host_tabs" options={options} activeIndex={index} />
      <div style={{ position: "absolute", left: 642, top: 304, display: "grid", gap: 10 }}>
        <Tag text={`focus ${index >= 1 ? "enabled" : "disabled"}`} tone={index >= 1 ? "mint" : "warm"} />
        <Tag text={`move ${index === 2 ? "enabled" : "disabled"}`} tone={index === 2 ? "accent" : "warm"} />
      </div>
    </>
  );
};

const renderLegacyIgnoreTabs = (_field, frame, fps) => {
  const allowTabs = phaseIndex(frame, 60, 2) === 1;
  return (
    <>
      <WindowFrame x={92} y={88} width={566} height={278} title="legacy host-tab fallback" accent={palette.panel}>
        <TabStrip x={28} y={20} labels={["tab a", "tab b"]} activeIndex={0} accent={palette.accent} />
        <PaneBlock x={34} y={102} width={212} height={118} fill={palette.accent} label="edge pane" active />
        <PaneBlock x={286} y={102} width={226} height={118} fill={palette.canvasMuted} label="next tab" muted={!allowTabs}/>
      </WindowFrame>
      <FlowArrow x={334} y={218} length={132} progress={allowTabs ? springProgress(frame, fps, 18) : 0.24} color={allowTabs ? palette.mint : palette.warm} />
      <OptionRail x={704} y={92} title="ignore_tabs" options={["true", "false"]} activeIndex={allowTabs ? 1 : 0} />
    </>
  );
};

const renderTerminalScope = (_field, frame) => {
  const options = ["mux_pane", "mux_window", "mux_session", "terminal_tab"];
  const index = phaseIndex(frame, 28, options.length);
  return (
    <>
      <WindowFrame x={92} y={86} width={540} height={278} title="terminal topology" accent={palette.panel}>
        <TabStrip x={28} y={20} labels={["tab a", "tab b"]} activeIndex={1} accent={palette.panel} />
        <PaneBlock x={36} y={102} width={146} height={118} fill={palette.accent} label="pane" active={index === 0} />
        <PaneBlock x={198} y={102} width={146} height={118} fill={palette.panel} label="pane" active={index === 1} muted={index !== 1} />
        <PaneBlock x={360} y={102} width={146} height={118} fill={palette.canvasMuted} label="session" active={index === 2} muted={index !== 2} />
      </WindowFrame>
      <OptionRail x={684} y={88} title="tear-off scope" options={options} activeIndex={index} />
      <div style={{ position: "absolute", left: 688, top: 302, display: "grid", gap: 10 }}>
        <Tag text={options[index]} tone="mint" />
        <Tag text={index === 3 ? "host-tab unit" : "mux-owned unit"} tone="accent" />
      </div>
    </>
  );
};

export const renderTerminalScene = (field, frame, fps) => {
  switch (field.scene.template) {
    case "integration-toggle":
      return renderIntegrationToggle(field, frame, fps);
    case "anchor-window":
      return renderAnchorWindow(field, frame, fps);
    case "terminal-mux-backend":
      return renderTerminalMuxBackend(field, frame, fps);
    case "terminal-host-tabs":
      return renderTerminalHostTabs(field, frame, fps);
    case "internal-enabled":
      return renderInternalEnabled(field, frame, fps);
    case "allowed-directions":
      return renderAllowedDirections(field, frame, fps);
    case "legacy-ignore-tabs":
      return renderLegacyIgnoreTabs(field, frame, fps);
    case "tear-off-toggle":
      return renderTearOffToggle(field, frame, fps);
    case "tear-off-strategy":
      return renderTearOffStrategy(field, frame, fps);
    case "terminal-scope":
      return renderTerminalScope(field, frame, fps);
    case "snap-back":
      return renderSnapBack(field, frame, fps);
    default:
      return renderIntegrationToggle(field, frame, fps);
  }
};
