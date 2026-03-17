import { Easing, interpolate, spring } from "remotion";
import { motion } from "./theme.js";

export const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" };

export const lerp = (from, to, progress) => from + (to - from) * progress;

export const rangeProgress = (frame, start, end, easing = Easing.inOut(Easing.cubic)) =>
  interpolate(frame, [start, end], [0, 1], {
    ...clamp,
    easing,
  });

export const springProgress = (frame, fps, delay, durationInFrames = 24) =>
  spring({
    fps,
    frame: frame - delay,
    config: motion.spring,
    durationInFrames,
  });

export const phaseIndex = (frame, phaseLength, count) => Math.floor(frame / phaseLength) % count;

export const pulse = (frame, start, duration = 18) => {
  const progress = interpolate(frame, [start, start + duration], [0, 1], clamp);
  return Math.sin(progress * Math.PI);
};
