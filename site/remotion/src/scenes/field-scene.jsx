import React from "react";
import { useCurrentFrame, useVideoConfig } from "remotion";
import { SceneShell } from "../shared/layout.jsx";
import { renderBrowserScene } from "./browser.jsx";
import { renderEditorScene } from "./editor.jsx";
import { renderTerminalScene } from "./terminal.jsx";

const renderSceneForField = (field, frame, fps) => {
  switch (field.kind) {
    case "browser":
      return renderBrowserScene(field, frame, fps);
    case "editor":
      return renderEditorScene(field, frame, fps);
    case "terminal":
      return renderTerminalScene(field, frame, fps);
    default:
      return null;
  }
};

export const FieldScene = ({ field }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return <SceneShell field={field}>{renderSceneForField(field, frame, fps)}</SceneShell>;
};
