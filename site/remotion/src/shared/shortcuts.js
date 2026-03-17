const directionSymbols = {
  left: "←",
  right: "→",
  up: "↑",
  down: "↓",
};

const inferDirection = (field) => {
  if (field.scene?.direction) {
    return field.scene.direction;
  }
  if (field.configPath.includes(".left")) return "left";
  if (field.configPath.includes(".right")) return "right";
  if (field.configPath.includes(".up")) return "up";
  if (field.configPath.includes(".down")) return "down";
  return "right";
};

const buildEvents = (frames, combo, label = "Example binding") =>
  frames.map((frame) => ({
    frame,
    combo,
    label,
  }));

export const shortcutEventsFor = (field) => {
  const direction = inferDirection(field);
  const arrow = directionSymbols[direction];

  if (field.scene?.template === "browser-tab-axis") {
    return [
      { frame: 14, combo: ["⌘", "→"], label: "Example binding" },
      { frame: 50, combo: ["⌘", "↓"], label: "Example binding" },
      { frame: 86, combo: ["⌘", "↑"], label: "Example binding" },
    ];
  }

  if (field.scene?.template === "terminal-host-tabs") {
    return [
      { frame: 16, combo: ["⌘", arrow], label: "Focus example" },
      { frame: 48, combo: ["⌘", "⇧", arrow], label: "Move example" },
    ];
  }

  if (field.groupId === "resize" || field.configPath.startsWith("resize")) {
    return buildEvents([16, 48], ["⌥", "⌘", arrow], "Example binding");
  }

  if (field.groupId === "move" || field.configPath.startsWith("move")) {
    return buildEvents([16, 48], ["⌘", "⇧", arrow], "Example binding");
  }

  if (field.groupId === "focus" || field.configPath.startsWith("focus")) {
    return buildEvents([16, 48], ["⌘", arrow], "Example binding");
  }

  return buildEvents([18], ["⌘", "→"], "Example binding");
};
