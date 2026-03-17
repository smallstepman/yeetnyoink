import { allDocFields } from "../../assets/docs-catalog.mjs";

const FPS = 30;
const WIDTH = 1280;
const HEIGHT = 720;
const DURATION_IN_FRAMES = 120;

const sceneEntries = allDocFields.map((field) => ({
  ...field,
  fps: FPS,
  width: WIDTH,
  height: HEIGHT,
  durationInFrames: DURATION_IN_FRAMES,
}));

export { DURATION_IN_FRAMES, FPS, HEIGHT, WIDTH, sceneEntries };
