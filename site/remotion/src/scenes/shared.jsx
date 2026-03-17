import React from "react";
import { palette } from "../shared/theme.js";
import { CompassPad, FlowArrow, FocusRing, OptionRail, PaneBlock, TabStrip, Tag, WindowFrame } from "../shared/primitives.jsx";
import { lerp, phaseIndex, pulse, rangeProgress, springProgress } from "../shared/utils.js";

export const renderIntegrationToggle = (field, frame, fps) => {
  const on = phaseIndex(frame, 60, 2) === 1;
  const progress = springProgress(frame, fps, 16, 24);
  return (
    <>
      <WindowFrame x={84} y={86} width={450} height={260} title={`${field.kindLabel.toLowerCase()} profile`} accent={palette.panel}>
        <PaneBlock x={38} y={44} width={166} height={112} fill={palette.accent} label="matching profile" active={on} muted={!on}/>
        <PaneBlock x={226} y={44} width={170} height={112} fill={palette.canvasMuted} label="outer layer" muted={on} />
        <div style={{ position: "absolute", left: 44, top: 184, display: "flex", gap: 10 }}>
          <Tag text={on ? "routing on" : "routing off"} tone={on ? "mint" : "warm"} />
          <Tag text={on ? "internal capability exposed" : "falls through"} tone="accent" />
        </div>
      </WindowFrame>
      <FlowArrow x={318} y={216} length={120} progress={on ? progress : 0.28} color={on ? palette.mint : palette.warm} />
      <OptionRail x={594} y={98} title="Toggle" options={["disabled", "enabled"]} activeIndex={on ? 1 : 0} />
    </>
  );
};

export const renderAnchorWindow = (_field, frame) => {
  const anchored = phaseIndex(frame, 60, 2) === 1;
  const flash = pulse(frame, anchored ? 62 : 14, 18);
  return (
    <>
      <WindowFrame x={182} y={86} width={430} height={260} title="app window" accent={palette.panel}>
        <PaneBlock x={42} y={42} width={346} height={160} fill={palette.accent} label={anchored ? "anchored host window" : "movable host window"} active={anchored} />
      </WindowFrame>
      <FlowArrow x={120} y={216} length={78} direction="right" progress={anchored ? 0.34 : 1} color={anchored ? palette.warm : palette.mint} />
      <FlowArrow x={610} y={216} length={78} direction="right" progress={anchored ? 0.34 : 1} color={anchored ? palette.warm : palette.mint} />
      <FlowArrow x={396} y={104} length={78} direction="down" progress={anchored ? 0.34 : 1} color={anchored ? palette.warm : palette.mint} />
      <FlowArrow x={396} y={348} length={78} direction="down" progress={anchored ? 0.34 : 1} color={anchored ? palette.warm : palette.mint} />
      {anchored ? <FocusRing x={224} y={186} width={346} height={160} opacity={0.3 + flash * 0.8} /> : null}
      <OptionRail x={640} y={104} title="anchor_app_window" options={["false", "true"]} activeIndex={anchored ? 1 : 0} />
    </>
  );
};

export const renderInternalEnabled = (field, frame, fps) => {
  const enabled = phaseIndex(frame, 60, 2) === 0;
  const progress = springProgress(frame, fps, 18, 26);
  return (
    <>
      <WindowFrame x={88} y={72} width={470} height={292} title={`${field.kindLabel.toLowerCase()} surface`} accent={palette.panel}>
        <PaneBlock x={34} y={36} width={170} height={184} fill={palette.panel} label="source split" />
        <PaneBlock x={228} y={36} width={170} height={184} fill={enabled ? palette.accent : palette.canvasMuted} label={enabled ? "internal target" : "outer handoff"} muted={!enabled}/>
      </WindowFrame>
      <WindowFrame x={626} y={118} width={170} height={198} title="outer" accent={palette.mint} opacity={enabled ? 0.42 : 1}>
        <PaneBlock x={18} y={32} width={134} height={96} fill={palette.mint} label="next layer" active={!enabled}/>
      </WindowFrame>
      <FlowArrow x={340} y={224} length={112} progress={enabled ? progress : 0.24} color={enabled ? palette.mint : palette.warm} />
      {!enabled ? (
        <FlowArrow x={548} y={224} length={84} progress={progress} color={palette.warm} />
      ) : null}
      <OptionRail x={594} y={74} title={field.title} options={["enabled", "disabled"]} activeIndex={enabled ? 0 : 1} />
    </>
  );
};

export const renderAllowedDirections = (field, frame) => {
  const optionSets = [
    ["left", "right", "up", "down"],
    ["left", "right", "down"],
    ["left", "right"],
  ];
  const index = phaseIndex(frame, 36, optionSets.length);
  const allowed = optionSets[index];
  return (
    <>
      <WindowFrame x={70} y={90} width={360} height={250} title={`${field.kindLabel.toLowerCase()} grid`} accent={palette.panel}>
        <PaneBlock x={36} y={42} width={124} height={140} fill={palette.accent} label="active" active />
        <PaneBlock x={186} y={42} width={124} height={64} fill={palette.panel} label="target" muted={!allowed.includes("right")}/>
        <PaneBlock x={186} y={118} width={124} height={64} fill={palette.canvasMuted} label="blocked" muted={!allowed.includes("down")}/>
      </WindowFrame>
      <CompassPad x={474} y={98} allowed={allowed} activeDirection="right" />
      <OptionRail x={692} y={98} title="Allowed set" options={["all", "W/E/S", "W/E"]} activeIndex={index} />
    </>
  );
};

export const renderTearOffToggle = (field, frame, fps) => {
  const enabled = phaseIndex(frame, 60, 2) === 1;
  const detach = springProgress(frame, fps, 18, 24);
  const floatingX = lerp(274, 44, enabled ? detach : 0);
  return (
    <>
      <WindowFrame x={256} y={86} width={430} height={268} title="source window" accent={palette.panel}>
        <PaneBlock
          x={34}
          y={40}
          width={126}
          height={156}
          fill={palette.accent}
          label={field.scene.unitLabel}
          active={!enabled}
          muted={enabled}
        />
        <PaneBlock x={182} y={40} width={206} height={70} fill={palette.panel} label="neighbor" />
        <PaneBlock x={182} y={126} width={206} height={70} fill={palette.canvasMuted} label="neighbor" />
      </WindowFrame>
      <WindowFrame x={floatingX} y={126} width={180} height={210} title="new window" accent={palette.mint} opacity={enabled ? detach : 0.22} scale={0.82 + detach * 0.18}>
        <PaneBlock x={22} y={30} width={136} height={112} fill={palette.mint} label="torn out" active={enabled} />
      </WindowFrame>
      <FlowArrow x={194} y={224} length={54} progress={enabled ? detach : 0.2} color={enabled ? palette.mint : palette.warm} />
      <OptionRail x={720} y={96} title={field.title} options={["disabled", "enabled"]} activeIndex={enabled ? 1 : 0} />
    </>
  );
};

export const renderTearOffStrategy = (_field, frame) => {
  const index = phaseIndex(frame, 38, 3);
  const positions = [
    { x: 204, y: 188 },
    { x: 96, y: 188 },
    { x: 32, y: 188 },
  ];
  const pane = positions[index];
  return (
    <>
      <WindowFrame x={84} y={86} width={420} height={268} title="edge test" accent={palette.panel}>
        <PaneBlock x={32} y={46} width={140} height={160} fill={palette.panel} label="left column" />
        <PaneBlock x={pane.x} y={46} width={140} height={160} fill={palette.accent} label="candidate" active />
      </WindowFrame>
      <OptionRail x={560} y={88} title="strategy" options={["only_if_edgemost", "once_it_neighbors_with_window_edge", "always"]} activeIndex={index} />
      <div style={{ position: "absolute", left: 566, top: 296, width: 220, color: palette.paper, fontSize: 18, lineHeight: 1.5 }}>
        {index === 0
          ? "Candidate must already sit at the outer edge."
          : index === 1
            ? "Candidate must neighbor the window edge along the allowed directions."
            : "Candidate may tear out without waiting for a stricter edge test."}
      </div>
    </>
  );
};

export const renderSnapBack = (field, frame, fps) => {
  const merge = rangeProgress(frame, 56, 108);
  const detach = springProgress(frame, fps, 16, 26);
  const floatingX = lerp(66, 268, merge);
  return (
    <>
      <WindowFrame x={256} y={86} width={430} height={268} title="target window" accent={palette.panel}>
        <PaneBlock x={34} y={40} width={126} height={156} fill={palette.panel} label="target" />
        <PaneBlock x={182} y={40} width={206} height={70} fill={palette.accent} label={field.scene.unitLabel} active={merge > 0.72} />
        <PaneBlock x={182} y={126} width={206} height={70} fill={palette.canvasMuted} label="neighbor" />
      </WindowFrame>
      <WindowFrame x={floatingX} y={126} width={180} height={210} title="torn-out" accent={palette.mint} opacity={1 - merge * 0.95} scale={0.84 + detach * 0.16}>
        <PaneBlock x={22} y={30} width={136} height={112} fill={palette.mint} label="floating" active />
      </WindowFrame>
      <FlowArrow x={194} y={224} length={62} progress={1 - merge * 0.12} color={palette.mint} />
      <FlowArrow x={438} y={224} length={86} progress={merge} color={palette.accent} />
      <OptionRail x={720} y={96} title={field.title} options={["off", "on"]} activeIndex={1} />
    </>
  );
};
