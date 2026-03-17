export const palette = {
  canvas: "#171B24",
  canvasMuted: "#202737",
  paper: "#F4F1EA",
  panel: "#DBE1FF",
  accent: "#5E67FF",
  accentSoft: "#7C85FF",
  mint: "#7CE3C2",
  ink: "#171B24",
  inkSoft: "#A7B0C8",
  warm: "#FFB84D",
  rose: "#FF7A90",
  outline: "rgba(255,255,255,0.14)",
  shadow: "rgba(9, 11, 18, 0.28)",
};

export const motion = {
  spring: {
    damping: 18,
    mass: 0.82,
    stiffness: 120,
  },
};

export const frameStyle = {
  borderRadius: 32,
  backgroundColor: palette.paper,
  border: `2px solid ${palette.outline}`,
  boxShadow: `0 24px 72px ${palette.shadow}`,
  overflow: "hidden",
};
