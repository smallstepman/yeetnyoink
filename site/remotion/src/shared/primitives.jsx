import React from "react";
import { palette, frameStyle } from "./theme.js";

const rotationFor = {
  right: 0,
  down: 90,
  left: 180,
  up: -90,
};

export const StageCard = ({ children }) => (
  <div
    style={{
      position: "absolute",
      left: 0,
      top: 0,
      width: 860,
      height: 430,
      borderRadius: 34,
      background: "rgba(16,20,32,0.78)",
      border: "1px solid rgba(244,241,234,0.08)",
      boxShadow: "0 28px 72px rgba(0,0,0,0.24)",
      overflow: "hidden",
    }}
  >
    <div
      style={{
        position: "absolute",
        inset: 0,
        backgroundImage:
          "linear-gradient(rgba(244,241,234,0.04) 1px, transparent 1px), linear-gradient(90deg, rgba(244,241,234,0.04) 1px, transparent 1px)",
        backgroundSize: "36px 36px",
      }}
    />
    {children}
  </div>
);

export const MetaPanel = ({ title, children, style }) => (
  <div
    style={{
      position: "absolute",
      right: 0,
      top: 0,
      width: 280,
      minHeight: 430,
      padding: 24,
      borderRadius: 30,
      background: "rgba(244,241,234,0.08)",
      border: "1px solid rgba(244,241,234,0.08)",
      color: palette.paper,
      ...style,
    }}
  >
    <div style={{ fontSize: 18, textTransform: "uppercase", letterSpacing: "0.12em", color: palette.mint, fontWeight: 700 }}>
      {title}
    </div>
    <div style={{ marginTop: 18, display: "grid", gap: 14 }}>{children}</div>
  </div>
);

export const WindowFrame = ({ x, y, width, height, title, accent = palette.panel, children, scale = 1, opacity = 1 }) => (
  <div
    style={{
      position: "absolute",
      left: x,
      top: y,
      width,
      height,
      transform: `scale(${scale})`,
      transformOrigin: "top left",
      opacity,
      ...frameStyle,
    }}
  >
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        height: 58,
        padding: "0 22px",
        backgroundColor: accent,
        color: palette.ink,
        fontSize: 20,
        fontWeight: 700,
      }}
    >
      <span style={{ display: "inline-flex", gap: 8 }}>
        <span style={{ width: 11, height: 11, borderRadius: 999, backgroundColor: "rgba(23,27,36,0.28)" }} />
        <span style={{ width: 11, height: 11, borderRadius: 999, backgroundColor: "rgba(23,27,36,0.18)" }} />
        <span style={{ width: 11, height: 11, borderRadius: 999, backgroundColor: "rgba(23,27,36,0.12)" }} />
      </span>
      <span>{title}</span>
    </div>
    <div style={{ position: "absolute", inset: 58 }}>{children}</div>
  </div>
);

export const PaneBlock = ({ x, y, width, height, fill, label, active = false, muted = false }) => (
  <div
    style={{
      position: "absolute",
      left: x,
      top: y,
      width,
      height,
      borderRadius: 24,
      backgroundColor: fill,
      border: active ? `8px solid ${palette.mint}` : `2px solid rgba(23,27,36,${muted ? 0.05 : 0.1})`,
      boxShadow: active ? "0 0 0 10px rgba(124,227,194,0.14)" : muted ? "none" : "0 12px 30px rgba(23,27,36,0.08)",
      opacity: muted ? 0.42 : 1,
      display: "flex",
      alignItems: "flex-end",
      padding: 16,
      color: fill === palette.canvasMuted ? palette.paper : palette.ink,
      fontSize: 18,
      fontWeight: 700,
    }}
  >
    {label}
  </div>
);

export const FlowArrow = ({ x, y, length, direction = "right", color = palette.mint, progress = 1 }) => {
  const rotation = rotationFor[direction] ?? 0;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width: length * progress,
        height: 16,
        borderRadius: 999,
        backgroundColor: color,
        transform: `rotate(${rotation}deg)`,
        transformOrigin: "left center",
      }}
    >
      <div
        style={{
          position: "absolute",
          right: -6,
          top: -11,
          width: 38,
          height: 38,
          borderTop: "18px solid transparent",
          borderBottom: "18px solid transparent",
          borderLeft: `28px solid ${color}`,
          opacity: progress > 0.08 ? 1 : 0,
        }}
      />
    </div>
  );
};

export const FocusRing = ({ x, y, width, height, opacity = 1 }) => (
  <div
    style={{
      position: "absolute",
      left: x - 10,
      top: y - 10,
      width: width + 20,
      height: height + 20,
      borderRadius: 28,
      border: `8px solid ${palette.mint}`,
      boxShadow: "0 0 0 12px rgba(124,227,194,0.14)",
      opacity,
    }}
  />
);

export const Tag = ({ text, tone = "default" }) => {
  const backgrounds = {
    default: "rgba(244,241,234,0.1)",
    accent: "rgba(94,103,255,0.16)",
    mint: "rgba(124,227,194,0.16)",
    warm: "rgba(255,184,77,0.18)",
  };
  const colors = {
    default: palette.paper,
    accent: palette.paper,
    mint: palette.mint,
    warm: palette.warm,
  };
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        minHeight: 34,
        padding: "0 12px",
        borderRadius: 999,
        backgroundColor: backgrounds[tone] ?? backgrounds.default,
        color: colors[tone] ?? colors.default,
        fontSize: 14,
        fontWeight: 700,
      }}
    >
      {text}
    </span>
  );
};

export const OptionRail = ({ x, y, title, options, activeIndex }) => (
  <div
    style={{
      position: "absolute",
      left: x,
      top: y,
      width: 230,
      padding: 18,
      borderRadius: 24,
      background: "rgba(244,241,234,0.08)",
      border: "1px solid rgba(244,241,234,0.08)",
      color: palette.paper,
    }}
  >
    <div style={{ fontSize: 16, textTransform: "uppercase", letterSpacing: "0.12em", color: palette.inkSoft, fontWeight: 700 }}>
      {title}
    </div>
    <div style={{ marginTop: 12, display: "grid", gap: 10 }}>
      {options.map((option, index) => (
        <div
          key={`${title}-${option}`}
          style={{
            padding: "10px 12px",
            borderRadius: 16,
            backgroundColor: index == activeIndex ? "rgba(124,227,194,0.18)" : "rgba(244,241,234,0.05)",
            border: index == activeIndex ? `1px solid ${palette.mint}` : "1px solid rgba(244,241,234,0.05)",
            color: index == activeIndex ? palette.paper : palette.inkSoft,
            fontWeight: index == activeIndex ? 700 : 600,
            fontSize: 15,
          }}
        >
          {option}
        </div>
      ))}
    </div>
  </div>
);

export const TabStrip = ({ x, y, labels, activeIndex, accent = palette.panel }) => (
  <div style={{ position: "absolute", left: x, top: y, display: "flex", gap: 12 }}>
    {labels.map((label, index) => (
      <div
        key={label}
        style={{
          minWidth: 120,
          padding: "12px 18px",
          borderRadius: 18,
          backgroundColor: index === activeIndex ? accent : palette.panel,
          color: palette.ink,
          border: index === activeIndex ? `5px solid ${palette.mint}` : "1px solid rgba(23,27,36,0.1)",
          boxShadow: index === activeIndex ? "0 0 0 8px rgba(124,227,194,0.14)" : "none",
          fontSize: 17,
          fontWeight: 700,
          textAlign: "center",
        }}
      >
        {label}
      </div>
    ))}
  </div>
);

export const CompassPad = ({ x, y, allowed, activeDirection }) => {
  const directions = [
    { key: "up", symbol: "↑", dx: 72, dy: 0 },
    { key: "right", symbol: "→", dx: 144, dy: 72 },
    { key: "down", symbol: "↓", dx: 72, dy: 144 },
    { key: "left", symbol: "←", dx: 0, dy: 72 },
  ];
  return (
    <div style={{ position: "absolute", left: x, top: y, width: 216, height: 216 }}>
      <div
        style={{
          position: "absolute",
          left: 54,
          top: 54,
          width: 108,
          height: 108,
          borderRadius: 30,
          backgroundColor: "rgba(244,241,234,0.08)",
          border: "1px solid rgba(244,241,234,0.08)",
        }}
      />
      {directions.map((direction) => {
        const allowedDirection = allowed.includes(direction.key);
        const active = activeDirection === direction.key;
        return (
          <div
            key={direction.key}
            style={{
              position: "absolute",
              left: direction.dx,
              top: direction.dy,
              width: 72,
              height: 72,
              borderRadius: 22,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              backgroundColor: active
                ? "rgba(124,227,194,0.18)"
                : allowedDirection
                  ? "rgba(94,103,255,0.18)"
                  : "rgba(244,241,234,0.05)",
              border: active
                ? `2px solid ${palette.mint}`
                : allowedDirection
                  ? `1px solid ${palette.accent}`
                  : "1px solid rgba(244,241,234,0.06)",
              color: allowedDirection ? palette.paper : palette.inkSoft,
              fontSize: 28,
              fontWeight: 800,
            }}
          >
            {direction.symbol}
          </div>
        );
      })}
    </div>
  );
};

export const Keycap = ({ text }) => (
  <span
    style={{
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      minWidth: 42,
      height: 42,
      padding: "0 12px",
      borderRadius: 14,
      backgroundColor: "rgba(255,255,255,0.9)",
      color: palette.ink,
      fontSize: 22,
      fontWeight: 800,
      boxShadow: "0 10px 24px rgba(9,11,18,0.16)",
    }}
  >
    {text}
  </span>
);
