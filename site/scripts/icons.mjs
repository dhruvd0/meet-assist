// Extracts the Solar icons the page uses into src/solar-subset.json so the
// bundle carries only those glyphs and never calls the Iconify API.
import { getIcons } from "@iconify/utils";
import { icons } from "@iconify-json/solar";
import { writeFileSync } from "node:fs";

const names = [
  "refresh-circle-linear",
  "hand-stars-linear",
  "lightbulb-linear",
  "magnifer-linear",
  "shield-check-linear",
  "microphone-linear",
  "laptop-linear",
  "keyboard-linear",
  "download-minimalistic-linear",
  "play-linear",
  "close-circle-linear",
  "home-2-linear",
  "settings-linear",
  "pause-linear",
  "stop-linear",
  "folder-linear",
  "history-linear",
];
const subset = getIcons(icons, names);
if (!subset || (subset.not_found && subset.not_found.length)) {
  throw new Error(`Missing Solar icons: ${subset?.not_found?.join(", ")}`);
}
writeFileSync(
  new URL("../src/solar-subset.json", import.meta.url),
  JSON.stringify(subset),
);
