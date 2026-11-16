import React, { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play } from "lucide-react";
import { formatElapsed } from "./LiveDock";
import type { MeetingPlayer } from "./useMeetingPlayer";
import type { NudgeMarker } from "./recordingTimeline";

/** Arrow keys move the playhead by this much. */
const KEY_STEP_MS = 5000;

/**
 * The meeting's recording as a bar pinned to the bottom of its page: play,
 * scrub, and jump to the points where a nudge was shown. `raised` lifts it
 * clear of the live-meeting dock.
 */
export const PlayerBar: React.FC<{
  player: MeetingPlayer;
  raised: boolean;
  onNudge: (marker: NudgeMarker) => void;
}> = ({ player, raised, onNudge }) => {
  const { t } = useTranslation();
  const trackRef = useRef<HTMLDivElement>(null);
  // While dragging, the bar follows the pointer and the audio waits.
  const [dragMs, setDragMs] = useState<number | null>(null);
  const [hovered, setHovered] = useState<NudgeMarker | null>(null);

  const { durationMs } = player;
  const shownMs = dragMs ?? player.positionMs;
  const percent = (ms: number) =>
    `${Math.min(100, Math.max(0, (ms / durationMs) * 100))}%`;

  const msAt = (clientX: number) => {
    const box = trackRef.current!.getBoundingClientRect();
    const ratio = (clientX - box.left) / box.width;
    const forward = document.dir === "rtl" ? 1 - ratio : ratio;
    return Math.min(1, Math.max(0, forward)) * durationMs;
  };

  return (
    <div
      className={`pointer-events-none sticky z-10 h-0 flex justify-center px-8 ${raised ? "bottom-20" : "bottom-5"}`}
    >
      <div
        role="region"
        aria-label={t("meetings.player.label")}
        className="pointer-events-auto absolute bottom-0 flex items-center gap-3 h-12 w-[calc(100%-4rem)] max-w-[720px] ps-1.5 pe-4 rounded-full bg-surface/95 border border-line-strong backdrop-blur-md shadow-[0_16px_40px_-12px_rgb(0_0_0/0.45),0_2px_6px_-2px_rgb(0_0_0/0.2)]"
      >
        <button
          type="button"
          onClick={player.toggle}
          aria-label={
            player.playing
              ? t("meetings.player.pause")
              : t("meetings.player.play")
          }
          className="w-9 h-9 shrink-0 rounded-full inline-flex items-center justify-center bg-accent/15 text-accent hover:bg-accent/25 transition-colors cursor-pointer"
        >
          {player.playing ? (
            <Pause className="w-3.5 h-3.5 fill-current" />
          ) : (
            <Play className="w-3.5 h-3.5 fill-current ms-0.5" />
          )}
        </button>

        <span className="font-mono text-[11px] text-muted tabular">
          {formatElapsed(shownMs)}
        </span>

        <div
          ref={trackRef}
          role="slider"
          tabIndex={0}
          aria-label={t("meetings.player.seek")}
          aria-valuemin={0}
          aria-valuemax={Math.round(durationMs / 1000)}
          aria-valuenow={Math.round(shownMs / 1000)}
          aria-valuetext={formatElapsed(shownMs)}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            setDragMs(msAt(e.clientX));
          }}
          onPointerMove={(e) => {
            if (dragMs !== null) setDragMs(msAt(e.clientX));
          }}
          onPointerUp={(e) => {
            if (dragMs === null) return;
            void player.seek(msAt(e.clientX), player.playing);
            setDragMs(null);
          }}
          onPointerCancel={() => setDragMs(null)}
          onKeyDown={(e) => {
            const step =
              e.key === "ArrowRight"
                ? KEY_STEP_MS
                : e.key === "ArrowLeft"
                  ? -KEY_STEP_MS
                  : 0;
            if (step === 0) return;
            e.preventDefault();
            void player.seek(player.positionMs + step, player.playing);
          }}
          className="group relative flex-1 h-8 flex items-center cursor-pointer touch-none rounded-full focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
        >
          <div className="relative w-full h-1 rounded-full bg-line-strong">
            <div
              className="absolute inset-y-0 start-0 rounded-full bg-accent"
              style={{ width: percent(shownMs) }}
            />
          </div>
          <div
            className="absolute top-1/2 w-3 h-3 -translate-y-1/2 -translate-x-1/2 rtl:translate-x-1/2 rounded-full bg-text opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 transition-opacity"
            style={{ insetInlineStart: percent(shownMs) }}
          />
          {player.markers.map((m) => (
            <button
              key={m.id}
              type="button"
              aria-label={t("meetings.player.nudge", {
                kind: t(`meetings.intervention.kind.${m.kind}`, {
                  defaultValue: m.kind,
                }),
                time: formatElapsed(m.atMs),
              })}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={() => onNudge(m)}
              onPointerEnter={() => setHovered(m)}
              onPointerLeave={() => setHovered(null)}
              onFocus={() => setHovered(m)}
              onBlur={() => setHovered(null)}
              className="absolute top-1/2 w-4 h-6 -translate-y-1/2 -translate-x-1/2 rtl:translate-x-1/2 inline-flex items-center justify-center cursor-pointer rounded-sm focus-visible:outline-2 focus-visible:outline-accent"
              style={{ insetInlineStart: percent(m.atMs) }}
            >
              <span
                className={`w-[3px] h-3 rounded-full ring-2 ring-surface transition-transform ${
                  hovered?.id === m.id ? "bg-text scale-y-125" : "bg-warning"
                }`}
              />
            </button>
          ))}
          {hovered && (
            <div
              role="tooltip"
              className="pointer-events-none absolute bottom-full mb-2 w-64 -translate-x-1/2 rtl:translate-x-1/2 px-3 py-2 rounded-xl bg-surface border border-line-strong shadow-[0_8px_24px_-8px_rgb(0_0_0/0.45)]"
              style={{
                insetInlineStart: `clamp(8rem, ${percent(hovered.atMs)}, calc(100% - 8rem))`,
              }}
            >
              <div className="flex items-baseline gap-2 text-xs">
                <span className="font-medium text-text">
                  {t(`meetings.intervention.kind.${hovered.kind}`, {
                    defaultValue: hovered.kind,
                  })}
                </span>
                <span className="font-mono text-[11px] text-faint tabular">
                  {formatElapsed(hovered.atMs)}
                </span>
              </div>
              <p className="mt-0.5 text-[13px] leading-[1.4rem] text-muted line-clamp-3">
                {hovered.text}
              </p>
            </div>
          )}
        </div>

        <span className="font-mono text-[11px] text-faint tabular">
          {formatElapsed(durationMs)}
        </span>
      </div>
    </div>
  );
};
