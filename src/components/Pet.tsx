import { useEffect, useRef } from "react";
import { PixelPet } from "../pet/PixelPet";
import { isTauri } from "../hooks/useClaudeState";
import type { PetState } from "../types";

const DRAG_THRESHOLD = 4; // px before a press becomes a window drag

/**
 * Renders the PixiJS pet. A press that doesn't move is treated as a click
 * (toggles the panel); a press that moves drags the whole window (F1).
 */
export function Pet({
  state,
  onClick,
}: {
  state: PetState;
  onClick: () => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const petRef = useRef<PixelPet | null>(null);
  const down = useRef<{ x: number; y: number; moved: boolean } | null>(null);

  useEffect(() => {
    const pet = new PixelPet();
    petRef.current = pet;
    (async () => {
      try {
        if (hostRef.current) await pet.init(hostRef.current);
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error("pet init failed", err);
      }
    })();
    return () => {
      petRef.current = null;
      pet.destroy();
    };
  }, []);

  useEffect(() => {
    petRef.current?.setStatus(state.status, state.running);
  }, [state.status, state.running]);

  const onPointerDown = (e: React.PointerEvent) => {
    down.current = { x: e.clientX, y: e.clientY, moved: false };
  };

  const onPointerMove = async (e: React.PointerEvent) => {
    const d = down.current;
    if (!d || d.moved) return;
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > DRAG_THRESHOLD) {
      d.moved = true;
      if (isTauri) {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        getCurrentWindow().startDragging();
      }
    }
  };

  const onPointerUp = () => {
    const d = down.current;
    down.current = null;
    if (d && !d.moved) onClick();
  };

  return (
    <div
      ref={hostRef}
      className="pet-host"
      title="Claude Pet — click for status, drag to move"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    />
  );
}
