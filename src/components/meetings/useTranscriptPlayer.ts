import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { TranscriptLine } from "@/bindings";
import { commands } from "@/bindings";

/**
 * Plays the saved audio behind transcript lines. `play(id)` plays one line;
 * `playFrom(id)` keeps going through the following lines. Lines recorded
 * before audio was kept have no file and are skipped.
 */
export const useTranscriptPlayer = (lines: TranscriptLine[]) => {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const paths = useRef(new Map<number, string | null>());
  const linesRef = useRef(lines);
  linesRef.current = lines;
  const continuous = useRef(false);
  const [playingId, setPlayingId] = useState<number | null>(null);

  const stop = useCallback(() => {
    continuous.current = false;
    audioRef.current?.pause();
    setPlayingId(null);
  }, []);

  const pathFor = async (id: number) => {
    if (!paths.current.has(id)) {
      const res = await commands.getLineAudioPath(id);
      paths.current.set(id, res.status === "ok" ? res.data : null);
    }
    return paths.current.get(id) ?? null;
  };

  const start = useCallback(async (fromId: number, keepGoing: boolean) => {
    continuous.current = keepGoing;
    const all = linesRef.current;
    let i = all.findIndex((l) => l.id === fromId);
    while (i >= 0 && i < all.length) {
      const line = all[i];
      const path = await pathFor(line.id);
      if (path) {
        const audio = audioRef.current ?? new Audio();
        audioRef.current = audio;
        audio.onended = () => {
          const next = linesRef.current.findIndex((l) => l.id === line.id) + 1;
          if (continuous.current && next < linesRef.current.length)
            void start(linesRef.current[next].id, true);
          else setPlayingId(null);
        };
        audio.src = convertFileSrc(path);
        setPlayingId(line.id);
        await audio.play().catch(() => setPlayingId(null));
        return;
      }
      if (!keepGoing) break;
      i += 1;
    }
    setPlayingId(null);
  }, []);

  // Stop when leaving the page.
  useEffect(() => () => audioRef.current?.pause(), []);

  return {
    playingId,
    play: (id: number) => (playingId === id ? stop() : start(id, false)),
    playFrom: (id: number) => start(id, true),
    stop,
  };
};
