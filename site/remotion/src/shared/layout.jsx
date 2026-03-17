import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { shortcutEventsFor } from "./shortcuts.js";
import { Keycap, MetaPanel, StageCard, Tag } from "./primitives.jsx";
import { palette } from "./theme.js";
import { clamp } from "./utils.js";

const ShortcutOverlay = ({ events }) => {
  const frame = useCurrentFrame();
  const active = events.find((event) => frame >= event.frame && frame <= event.frame + 16);
  const latest = [...events].reverse().find((event) => frame >= event.frame) ?? events[0];
  const pulse = active
    ? interpolate(frame, [active.frame, active.frame + 8, active.frame + 16], [0.22, 1, 0.22], clamp)
    : 0.16;
  const scale = active
    ? interpolate(frame, [active.frame, active.frame + 8, active.frame + 16], [0.94, 1.04, 0.97], clamp)
    : 0.96;
  const event = active ?? latest;
  if (!event){
    return null;
  }
  return (
    <div
      style={{
        position: "absolute",
        right: 64,
        top: 64,
        padding: 18,
        borderRadius: 24,
        backgroundColor: `rgba(255,255,255,${0.08 + pulse * 0.2})`,
        border: `1px solid rgba(255,255,255,${0.1 + pulse * 0.2})`,
        boxShadow: `0 22px 46px rgba(9,11,18,${0.16 + pulse * 0.12})`,
        transform: `scale(${scale})`,
        opacity: 0.4 + pulse * 0.7,
      }}
    >
      <div
        style={{
          fontSize: 13,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color: palette.inkSoft,
          fontWeight: 700,
        }}
      >
        {event.label}
      </div>
      <div style={{ marginTop: 10, display: "flex", gap: 8 }}>
        {event.combo.map((part, index) => (
          <Keycap key={`${part}-${index}` } text={part} />
        ))}
      </div>
    </div>
  );
};

export const SceneShell = ({ field, children }) => {
  const frame = useCurrentFrame();
  const shortcutEvents = shortcutEventsFor(field);
  const summaryOpacity = interpolate(frame, [0, 14], [0, 1], clamp);

  return (
    <AbsoluteFill
      style={{
        backgroundColor: palette.canvas,
        color: palette.paper,
        fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif",
        overflow: "hidden",
      }}
    >
      <AbsoluteFill
        style={{
          backgroundImage:
            "linear-gradient(rgba(244,241,234,0.04) 1px, transparent 1px), linear-gradient(90deg, rgba(244,241,234,0.04) 1px, transparent 1px)",
          backgroundSize: "42px 42px",
          opacity: 0.8,
        }}
      />
      <div
        style={{
          position: "absolute",
          inset: 36,
          borderRadius: 42,
          border: "1px solid rgba(244,241,234,0.08)",
          background:
            "radial-gradient(circle at top right, rgba(124,227,194,0.14), transparent 22%), radial-gradient(circle at bottom left, rgba(94,103,255,0.18), transparent 28%)",
        }}
      />

      <div style={{ position: "absolute", left: 64, top: 58, width: 720, opacity: summaryOpacity }}>
        <div
          style={{
            width: "fit-content",
            padding: "10px 16px",
            borderRadius: 999,
            backgroundColor: "rgba(244,241,234,0.1)",
            color: palette.mint,
            letterSpacing: "0.14em",
            textTransform: "uppercase",
            fontWeight: 700,
            fontSize: 17,
          }}
        >
          {field.kindLabel} config
        </div>
        <div style={{ marginTop: 14, fontSize: 54, fontWeight: 800, letterSpacing: "-0.05em", lineHeight: 1.02 }}>
          {field.title}
        </div>
        <div style={{ marginTop: 12, fontSize: 24, color: palette.inkSoft, lineHeight: 1.5 }}>
          {field.summary}
        </div>
      </div>

      <div style={{ position: "absolute", left: 64, top: 220 }}>
        <StageCard>{children}</StageCard>
      </div>

      <MetaPanel title="Field details" style={{ right: 64, top: 220 }}>
        <Tag text={field.configPath} tone="accent" />
        <Tag text={`default: ${field.defaultValue}`} tone="mint" />
        <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
          {(field.values ?? []).slice(0, 5).map((value) => (
            <Tag key={`${field.slug}-${value}`} text={value} />
          ))}
        </div>
        {field.note ? (
          <div
            style={{
              padding: 14,
              borderRadius: 20,
              backgroundColor: "rgba(255,184,77,0.12)",
              border: "1px solid rgba(255,184,77,0.18)",
              color: palette.paper,
              fontSize: 15,
              lineHeight: 1.5,
            }}
          >
            {field.note}
          </div>
        ) : null}
      </MetaPanel>

      <div
        style={{
          position: "absolute",
          left: 64,
          right: 64,
          bottom: 40,
          padding: "18px 22px",
          borderRadius: 24,
          backgroundColor: "rgba(244,241,234,0.08)",
          border: "1px solid rgba(244,241,234,0.1)",
          color: palette.paper,
          fontSize: 22,
          lineHeight: 1.5,
        }}
      >
        {field.behavior}
      </div>

      <ShortcutOverlay events={shortcutEvents} />
    </AbsoluteFill>
  );
};
