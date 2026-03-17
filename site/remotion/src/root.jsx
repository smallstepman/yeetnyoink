import React from "react";
import { Composition, registerRoot } from "remotion";
import { sceneEntries } from "./catalog.js";
import { FieldScene } from "./scenes/field-scene.jsx";

const Root = () => {
  return (
    <>
      {sceneEntries.map((field) => (
        <Composition
          key={field.compositionId}
          id={field.compositionId}
          component={FieldScene}
          defaultProps={{ field }}
          durationInFrames={field.durationInFrames}
          fps={field.fps}
          width={field.width}
          height={field.height}
        />
      ))}
    </>
  );
};

registerRoot(Root);
