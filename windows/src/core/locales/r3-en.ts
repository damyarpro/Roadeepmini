// English strings for Settings → Shortcuts (the global shortcuts and the
// island's own keys). Registered through registerMessages by
// settings/shortcuts-section.ts.

export const r3En: Record<string, string> = {
  "shortcuts.title": "Shortcuts",
  "shortcuts.desc": "Keys that work from any app, and the keys the open island answers to.",

  "shortcuts.globalHead": "From any app",
  "shortcuts.globalHint":
    "Click a shortcut and press the new keys: Esc cancels, Backspace turns it off. A shortcut you turn on stops working in other apps that use the same keys.",
  "shortcuts.islandHead": "In the open island",
  "shortcuts.islandHint": "These work while the island has the keyboard, as in the chat and the planner.",

  "shortcuts.action.openChat": "Open the chat",
  "shortcuts.action.toggleIsland": "Open or close the island",
  "shortcuts.action.goToAlert": "Go to the waiting permission or question",
  "shortcuts.action.jumpToTerminal": "Go to the session's terminal",
  "shortcuts.action.nextPill": "Next item on the island",
  "shortcuts.action.prevPill": "Previous item on the island",
  "shortcuts.action.muteToggle": "Turn sounds off or on",

  "shortcuts.status.active": "Active",
  "shortcuts.status.off": "Off",
  "shortcuts.status.inUse": "In use by another app",
  "shortcuts.status.duplicate": "Taken by “{name}”",
  "shortcuts.status.invalid": "Not a valid shortcut",
  "shortcuts.status.typesCharacter": "Types “{char}”",
  "shortcuts.status.unavailable": "Not available",
  "shortcuts.status.paused": "Paused while recording",

  "shortcuts.none": "None",
  "shortcuts.recordLabel": "{name}: {combo}. Click to change.",
  "shortcuts.recording": "Press the keys…",
  "shortcuts.recordingHint": "Esc cancels · Backspace turns it off",
  "shortcuts.saved": "Shortcut saved: {combo}",
  "shortcuts.turnedOff": "“{name}” turned off",
  "shortcuts.needsModifier": "Hold Ctrl, Alt or the Windows key with it.",
  "shortcuts.unsupportedKey": "That key can't be part of a shortcut.",
  "shortcuts.typesNote":
    "{combo} types “{char}” on one of your keyboard layouts, so it can't be a shortcut. Pick another key.",
  "shortcuts.clash": "{combo} is already “{name}”. Change that one first, or pick other keys.",
  "shortcuts.reset": "Reset to defaults",
  "shortcuts.resetDone": "Shortcuts are back to their defaults",

  "shortcuts.island.cycle": "Next or previous item on the island",
  "shortcuts.island.byNumber": "Go to item 1 to 9",
  "shortcuts.island.pin": "Keep the island open",
  "shortcuts.island.settings": "Open Settings",
  "shortcuts.island.send": "Send the message",
  "shortcuts.island.close": "Close the island",
  "shortcuts.or": "or",
  "shortcuts.to": "to",
};
