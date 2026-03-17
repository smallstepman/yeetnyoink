import React from "react";
import { palette } from "../shared/theme.js";
import { FlowArrow, FocusRing, OptionRail, PaneBlock, TabStrip, Tag, WindowFrame } from "../shared/primitives.jsx";
import { phaseIndex, rangeProgress, springProgress } from "../shared/utils.js";
import {
  renderAnchorWindow,
  renderIntegrationToggle,
  renderSnapBack,
  renderTearOffStrategy,
  renderTearOffToggle,
} from "./shared.jsx";

const focusActionLabels = {
  ignore: "ignore",
  focus_previous_tab: "focus previous tab",
  focus_next_tab: "focus next tab",
  focus_first_tab: "focus first tab",
  focus_last_tab: "focus last tab",
};

const moveActionLabels = {
  ignore: "ignore",
  move_tab_backward: "move tab backward",
  move_tab_forward: "move tab forward",
  move_tab_to_first_position: "move tab to first position",
  move_tab_to_last_position: "move tab to last position",
};

const directionSymbols = {
  left: "←",
  right: "→",
  up: "↑",
  down: "↓",
};

const actionLabelFor = (field) => {
  const values = field.values ?? [];
  const value = values[phaseIndex(0, 1, Math.max(values.length, 1))] ?? values[0];
  return field.scene.actionKind === "focus"
    ? focusActionLabels[value] ?? value
    : moveActionLabels[value] ?? value;
};

const renderBrowserTabAxis = (field, frame, fps) => {
  const axisIndex = phaseIndex(frame, 40, 3);
  const modes = ["horizontal", "vertical", "vertical_flipped"];
  const activeIndex = phaseIndex(frame, 24, 4);
  return (
    <>
      <WindowFrame x={88} y={92} width={566} height={256} title="browser window" accent={palette.panel}>
        <TabStrip x={24} y={24} labels={["tab 1", "tab 2", "tab 3", "tab 4"]} activeIndex={activeIndex} accent={palette.accent} />
        <PaneBlock x={34} y={106} width={486} height={108} fill={palette.canvasMuted} label="page content" />
      </WindowFrame>
      <OptionRail x={698} y={92} title="tab_axis" options={modes} activeIndex={axisIndex} />
      {axisIndex === 0 ? <FlowArrow x={350} y={184} length={150} direction="right" progress={springProgress(frame, fps, 18)} /> : null}
      {axisIndex === 1 ? <FlowArrow x={400} y={248} length={122} direction="up" progress={springProgress(frame, fps, 18)} /> : null}
      {axisIndex === 2 ? <FlowArrow x={400} y={128} length={122} direction="down" progress={springProgress(frame, fps, 18)} /> : null}
      <div style={{ position: "absolute", left: 700, top: 288, width: 218, color: palette.paper, fontSize: 18, lineHeight: 1.5 }}>
        {axisIndex === 0
          ? "West/east keep the browser tab mapping."
          : axisIndex === 1
            ? "North/south now drive previous/next tab."
            : "North/south stay vertical, but previous/next are flipped."}
      </div>
    </>
  );
};

const renderBrowserDirectionAction = (field, frame, fps) => {
  const direction = field.scene.direction;
  const activeIndex = phaseIndex(frame, 22, 4);
  const actionIndex = phaseIndex(frame, 30, Math.max(field.values.length, 1));
  const action = field.values[actionIndex] ?? field.values[0];
  const label = field.scene.actionKind === "focus" ? focusActionLabels[action] ?? action : moveActionLabels[action] ?? action;
  return (
    <>
      <WindowFrame x={94} y={90} width={566} height={256} title="browser window" accent={palette.panel}>
        <TabStrip x={24} y={24} labels={["tab 1", "tab 2", "tab 3", "tab 4"]} activeIndex={activeIndex} accent={palette.accent} />
        <PaneBlock x={34} y={106} width={486} height={108} fill={palette.canvasMuted} label="page content" />
        <FocusRing x={64 + activeIndex * 132} y={114} width={120} height={50} opacity={0.92} />
      </WindowFrame>
      <FlowArrow x={660} y={214} length={126} direction={direction} progress={springProgress(frame, fps, 18)} color={palette.mint} />
      <OptionRail x={820} y={90} title={field.title} options={field.values} activeIndex={actionIndex} />
      <div style={{ position: "absolute", left: 820, top: 296, display: "grid", gap: 10 }}>
        <Tag text={`direction ${directionSymbols[direction]}`} tone="mint" />
        <Tag text={label} tone="accent" />
      </div>
    </>
  );
};

export const renderBrowserScene = (field, frame, fps) => {
  switch (field.scene.template) {
    case "integration-toggle":
      return renderIntegrationToggle(field, frame, fps);
    case "anchor-window":
      return renderAnchorWindow(field, frame, fps);
    case "browser-tab-axis":
      return renderBrowserTabAxis(field, frame, fps);
    case "browser-direction-action":
      return renderBrowserDirectionAction(field, frame, fps);
    case "tear-off-toggle":
      return renderTearOffToggle(field, frame, fps);
    case "tear-off-strategy":
      return renderTearOffStrategy(field, frame, fps);
    case "snap-back":
      return renderSnapBack(field, frame, fps);
    default:
      return renderIntegrationToggle(field, frame, fps);
  }
};
