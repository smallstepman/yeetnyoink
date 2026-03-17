import React from "react";
import { palette } from "../shared/theme.js";
import { FlowArrow, OptionRail, PaneBlock, Tag, WindowFrame } from "../shared/primitives.jsx";
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

const renderManageTerminal = (_field, frame) => {
  const managed = phaseIndex(frame, 60, 2) === 1;
  return (
    <>
      <WindowFrame x={88} y={88} width={520} height={278} title="editor surface" accent={palette.panel}>
        <PaneBlock x={34} y={42} width={200} height={166} fill={palette.accent} label="code split" active />
        <PaneBlock
          x={256}
          y={42}
          width={220}
          height={166}
          fill={palette.canvasMuted}
          label="embedded terminal"
          muted={!managed}
          active={managed}
        />
      </WindowFrame>
      <OptionRail x={652} y={92} title="manage_terminal" options={["false", "true"]} activeIndex={managed ? 1 : 0} />
      <div style={{ position: "absolute", left: 656, top: 302, display: "grid", gap: 10 }}>
        <Tag text={managed ? "terminal participates" : "terminal excluded"} tone={managed ? "mint" : "warm"} />
      </div>
    </>
  );
};

const renderEditorScope = (_field, frame) => {
  const options = ["buffer", "window", "workspace"];
  const index = phaseIndex(frame, 36, options.length);
  return (
    <>
      <WindowFrame x={82} y={86} width={558} height={286} title="editor workspace" accent={palette.panel}>
        <PaneBlock x={34} y={40} width={146} height={168} fill={palette.accent} label="buffer" active={index === 0} />
        <PaneBlock x={198} y={40} width={146} height={168} fill={palette.panel} label="window" active={index === 1} muted={index !== 1} />
        <PaneBlock x={362} y={40} width={146} height={168} fill={palette.canvasMuted} label="workspace" active={index === 2} muted={index !== 2} />
      </WindowFrame>
      <OptionRail x={682} y={92} title="tear_off_scope" options={options} activeIndex={index} />
    </>
  );
};

const renderEditorTerminalApp = (_field, frame, fps) => {
  const progress = springProgress(frame, fps, 18);
  return (
    <>
      <WindowFrame x={70} y={104} width={280} height={236} title="terminal host" accent={palette.panel}>
        <PaneBlock x={28} y={42} width={224} height={118} fill={palette.panel} label="terminal chain" />
      </WindowFrame>
      <WindowFrame x={430} y={86} width={334} height={272} title="editor UI" accent={palette.accent}>
        <PaneBlock x={34} y={40} width={266} height={156} fill={palette.accent} label="editor surface" active />
      </WindowFrame>
      <FlowArrow x={350} y={218} length={76} progress={progress} color={palette.mint} />
      <OptionRail x={820} y={94} title="ui.terminal.app" options={["unset", "wezterm", "kitty", "alacritty"]} activeIndex={1} />
    </>
  );
};

const renderEditorTerminalMux = (_field, frame) => {
  const options = ["inherit", "resolved from app", "ambiguous", "explicit backend"];
  const index = phaseIndex(frame, 30, options.length);
  return (
    <>
      <WindowFrame x={86} y={100} width={500} height={248} title="editor terminal UI" accent={palette.panel}>
        <PaneBlock x={34} y={42} width={190} height={126} fill={palette.accent} label="editor" active />
        <PaneBlock x={246} y={42} width={220} height={126} fill={palette.canvasMuted} label={index === 2 ? "no single backend" : "resolved backend"} muted={index === 2} />
      </WindowFrame>
      <OptionRail x={636} y={98} title="ui.terminal.mux_backend" options={options} activeIndex={index} />
      <div style={{ position: "absolute", left: 640, top: 302, display: "grid", gap: 10 }}>
        <Tag text={index === 0 ? "inherit request" : index === 1 ? "inherits host backend" : index === 2 ? "needs explicit choice" : "forces chosen backend"} tone={index === 2 ? "warm" : "mint"} />
      </div>
    </>
  );
};

const renderEditorGraphicalApp = (_field, frame, fps) => {
  const progress = springProgress(frame, fps, 18);
  return (
    <>
      <WindowFrame x={114} y={102} width={586} height={252} title="graphical app shell" accent={palette.panel}>
        <PaneBlock x={34} y={42} width={250} height={126} fill={palette.panel} label="graphical UI target" />
        <PaneBlock x={304} y={42} width={248} height={126} fill={palette.accent} label="editor surface" active />
      </WindowFrame>
      <FlowArrow x={314} y={212} length={86} progress={progress} color={palette.mint} />
      <OptionRail x={744} y={102} title="ui.graphical.app" options={["unset", "vscode", "emacs"]} activeIndex={1} />
    </>
  );
};

export const renderEditorScene = (field, frame, fps) => {
  switch (field.scene.template) {
    case "integration-toggle":
      return renderIntegrationToggle(field, frame, fps);
    case "anchor-window":
      return renderAnchorWindow(field, frame, fps);
    case "editor-manage-terminal":
      return renderManageTerminal(field, frame, fps);
    case "editor-scope":
      return renderEditorScope(field, frame, fps);
    case "internal-enabled":
      return renderInternalEnabled(field, frame, fps);
    case "allowed-directions":
      return renderAllowedDirections(field, frame, fps);
    case "tear-off-toggle":
      return renderTearOffToggle(field, frame, fps);
    case "tear-off-strategy":
      return renderTearOffStrategy(field, frame, fps);
    case "snap-back":
      return renderSnapBack(field, frame, fps);
    case "editor-terminal-app":
      return renderEditorTerminalApp(field, frame, fps);
    case "editor-terminal-mux":
      return renderEditorTerminalMux(field, frame, fps);
    case "editor-graphical-app":
      return renderEditorGraphicalApp(field, frame, fps);
    default:
      return renderIntegrationToggle(field, frame, fps);
  }
};
