// Keeps the 3D engine's walker in step with the app: the walk settings
// (viewerStore `walk`, remembered on this computer), the speed the wheel sets,
// the level the walker is on (for the minimap label), and clicks on switches
// while walking. Viewer3D calls it once, after it creates the engine.

import { useEffect, useMemo, type RefObject } from "react";
import { create } from "zustand";
import { useApp } from "../../state/store";
import type { ViewerEngine } from "../engine/ViewerEngine";
import { useViewer } from "../viewerStore";
import { switchKeysOf, walkSwitchClick } from "./switches";

/** Walk overlay state that only the overlay reads. */
export interface WalkUiState {
  /** The level the walker is on. */
  levelId: string | null;
}

export const useWalkUi = create<WalkUiState>(() => ({ levelId: null }));

export function useWalkSync(engineRef: RefObject<ViewerEngine | null>): void {
  const walk = useViewer((s) => s.walk);
  const catalog = useApp((s) => s.catalog);
  const switchKeys = useMemo(() => switchKeysOf(catalog), [catalog]);

  useEffect(() => {
    engineRef.current?.setWalkSettings(walk);
  }, [engineRef, walk]);

  useEffect(() => {
    const engine = engineRef.current;
    if (!engine) return;
    engine.setWalkHandlers({
      onSpeed: (speed) => useViewer.getState().setWalk({ speed }),
      onLevel: (levelId) => {
        if (useWalkUi.getState().levelId !== levelId) useWalkUi.setState({ levelId });
      },
      onWalkClick: (id) => walkSwitchClick(engine, useApp.getState().doc, id, switchKeys),
    });
    return () => engine.setWalkHandlers({});
  }, [engineRef, switchKeys]);
}
